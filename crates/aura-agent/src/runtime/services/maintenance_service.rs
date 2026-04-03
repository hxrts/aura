use super::service_actor::{validate_actor_transition, ActorLifecyclePhase, ActorOwnedServiceRoot};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use super::{
    ceremony_runner::CeremonyRunner, CeremonyTracker, LanTransportService, ReconfigurationManager,
    RendezvousManager, SyncServiceManager, ThresholdSigningService,
};
use crate::core::AuthorityContext;
use crate::handlers::{
    device_epoch_rotation::DeviceEpochRotationService, InvitationHandler, RendezvousHandler,
};
use crate::runtime::system::{
    publish_lan_descriptor_with, register_bootstrap_candidate_with, sync_peer_reconcile_interval,
};
use crate::runtime::{AuraEffectSystem, RuntimeServiceLifecycleEvent, TaskGroup};
use async_trait::async_trait;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl MaintenanceServiceState {
    fn phase(self) -> ActorLifecyclePhase {
        match self {
            Self::Stopped => ActorLifecyclePhase::Stopped,
            Self::Starting => ActorLifecyclePhase::Starting,
            Self::Running => ActorLifecyclePhase::Running,
            Self::Stopping => ActorLifecyclePhase::Stopping,
            Self::Failed => ActorLifecyclePhase::Failed,
        }
    }
}

struct RuntimeMaintenanceShared {
    owner: ActorOwnedServiceRoot<RuntimeMaintenanceService, (), MaintenanceServiceState>,
    degraded_reasons: RwLock<Vec<String>>,
}

#[derive(Clone)]
#[aura_macros::actor_root(
    owner = "runtime_maintenance_service",
    domain = "runtime_maintenance",
    supervision = "maintenance_task_root",
    category = "actor_owned"
)]
pub struct RuntimeMaintenanceService {
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    device_id: DeviceId,
    ceremony_tracker: CeremonyTracker,
    ceremony_runner: CeremonyRunner,
    threshold_signing: ThresholdSigningService,
    reconfiguration: ReconfigurationManager,
    sync_manager: Option<SyncServiceManager>,
    rendezvous_manager: Option<RendezvousManager>,
    rendezvous_handler: Option<RendezvousHandler>,
    lan_transport: Option<Arc<LanTransportService>>,
    shared: Arc<RuntimeMaintenanceShared>,
}

impl RuntimeMaintenanceService {
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_id: AuthorityId,
        device_id: DeviceId,
        ceremony_tracker: CeremonyTracker,
        ceremony_runner: CeremonyRunner,
        threshold_signing: ThresholdSigningService,
        reconfiguration: ReconfigurationManager,
        sync_manager: Option<SyncServiceManager>,
        rendezvous_manager: Option<RendezvousManager>,
        rendezvous_handler: Option<RendezvousHandler>,
        lan_transport: Option<Arc<LanTransportService>>,
    ) -> Self {
        Self {
            effects,
            authority_id,
            device_id,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration,
            sync_manager,
            rendezvous_manager,
            rendezvous_handler,
            lan_transport,
            shared: Arc::new(RuntimeMaintenanceShared {
                owner: ActorOwnedServiceRoot::new(MaintenanceServiceState::Stopped),
                degraded_reasons: RwLock::new(Vec::new()),
            }),
        }
    }

    async fn mark_state(&self, next: MaintenanceServiceState) {
        self.shared.owner.set_state(next).await;
    }

    async fn record_degraded_reason(&self, reason: impl Into<String>) {
        let reason = reason.into();
        let mut degraded = self.shared.degraded_reasons.write().await;
        if !degraded.iter().any(|existing| existing == &reason) {
            degraded.push(reason);
        }
    }

    async fn clear_degraded_reason_contains(&self, marker: &str) {
        self.shared
            .degraded_reasons
            .write()
            .await
            .retain(|reason| !reason.contains(marker));
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.shared.owner.lifecycle().lock().await;
        let current = self.shared.owner.state().await;
        if current == MaintenanceServiceState::Running {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Starting)?;
        self.mark_state(MaintenanceServiceState::Starting).await;
        self.shared.degraded_reasons.write().await.clear();

        let tasks = context.tasks().group(self.name());
        self.spawn_loops(tasks.clone(), context.time_effects())
            .await?;
        self.shared.owner.install_tasks(tasks).await;
        self.mark_state(MaintenanceServiceState::Running).await;
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _guard = self.shared.owner.lifecycle().lock().await;
        let current = self.shared.owner.state().await;
        if current == MaintenanceServiceState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(MaintenanceServiceState::Stopping).await;

        let shutdown_error = if let Some(tasks) = self.shared.owner.take_tasks().await {
            tasks
                .shutdown_with_timeout(Duration::from_secs(2))
                .await
                .err()
                .map(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("failed to stop maintenance task group: {error}"),
                    )
                })
        } else {
            None
        };

        match shutdown_error {
            Some(error) => {
                self.mark_state(MaintenanceServiceState::Failed).await;
                Err(error)
            }
            None => {
                self.shared.degraded_reasons.write().await.clear();
                self.mark_state(MaintenanceServiceState::Stopped).await;
                Ok(())
            }
        }
    }

    async fn spawn_loops(
        &self,
        tasks: TaskGroup,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) -> Result<(), ServiceError> {
        let invitation_handler = InvitationHandler::new(AuthorityContext::new_with_device(
            self.authority_id,
            self.device_id,
        ))
        .map_err(|error| ServiceError::startup_failed(self.name(), error.to_string()))?;
        let device_rotation_service = DeviceEpochRotationService::new(
            self.authority_id,
            self.effects.clone(),
            self.ceremony_tracker.clone(),
            self.ceremony_runner.clone(),
            self.threshold_signing.clone(),
            self.reconfiguration.clone(),
        );
        let effects = self.effects.clone();
        let acceptance_service = self.clone();
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _acceptance_task_handle = tasks.spawn_local_interval_until_named(
                    "invitation_acceptance",
                    time_effects.clone(),
                    Duration::from_secs(2),
                    move || {
                        let effects = effects.clone();
                        let handler = invitation_handler.clone();
                        let acceptance_service = acceptance_service.clone();
                        async move {
                            if let Err(error) = handler
                                .process_contact_invitation_acceptances(effects.clone())
                                .await
                            {
                                acceptance_service
                                    .record_degraded_reason(format!(
                                        "invitation_acceptance: {error}"
                                    ))
                                    .await;
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process contact invitation acceptances"
                                );
                            } else {
                                acceptance_service
                                    .clear_degraded_reason_contains("invitation_acceptance")
                                    .await;
                            }
                            true
                        }
                    },
                );
            } else {
                let _acceptance_task_handle = tasks.spawn_interval_until_named(
                    "invitation_acceptance",
                    time_effects.clone(),
                    Duration::from_secs(2),
                    move || {
                        let effects = effects.clone();
                        let handler = invitation_handler.clone();
                        let acceptance_service = acceptance_service.clone();
                        async move {
                            if let Err(error) = handler
                                .process_contact_invitation_acceptances(effects.clone())
                                .await
                            {
                                acceptance_service
                                    .record_degraded_reason(format!(
                                        "invitation_acceptance: {error}"
                                    ))
                                    .await;
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process contact invitation acceptances"
                                );
                            } else {
                                acceptance_service
                                    .clear_degraded_reason_contains("invitation_acceptance")
                                    .await;
                            }
                            true
                        }
                    },
                );
            }
        }

        let device_ceremony_service = self.clone();
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _device_rotation_task_handle = tasks.spawn_local_interval_until_named(
                    "device_epoch_rotation",
                    time_effects.clone(),
                    Duration::from_millis(500),
                    move || {
                        let service = device_rotation_service.clone();
                        let device_ceremony_service = device_ceremony_service.clone();
                        async move {
                            if let Err(error) = service.process_pending_participant_sessions().await {
                                device_ceremony_service
                                    .record_degraded_reason(format!("device_epoch_rotation: {error}"))
                                    .await;
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process device epoch rotation sessions"
                                );
                            } else {
                                device_ceremony_service
                                    .clear_degraded_reason_contains("device_epoch_rotation")
                                    .await;
                            }
                            true
                        }
                    },
                );
            } else {
                let _device_rotation_task_handle = tasks.spawn_interval_until_named(
                    "device_epoch_rotation",
                    time_effects.clone(),
                    Duration::from_millis(500),
                    move || {
                        let service = device_rotation_service.clone();
                        let device_ceremony_service = device_ceremony_service.clone();
                        async move {
                            if let Err(error) = service.process_pending_participant_sessions().await {
                                device_ceremony_service
                                    .record_degraded_reason(format!("device_epoch_rotation: {error}"))
                                    .await;
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process device epoch rotation sessions"
                                );
                            } else {
                                device_ceremony_service
                                    .clear_degraded_reason_contains("device_epoch_rotation")
                                    .await;
                            }
                            true
                        }
                    },
                );
            }
        }

        if let Some(rendezvous_handler) = self.rendezvous_handler.clone() {
            let effects = self.effects.clone();
            let handshake_service = self.clone();
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let _handshake_task_handle = tasks.spawn_local_interval_until_named(
                        "rendezvous_handshakes",
                        time_effects.clone(),
                        Duration::from_secs(2),
                        move || {
                            let effects = effects.clone();
                            let handler = rendezvous_handler.clone();
                            let handshake_service = handshake_service.clone();
                            async move {
                                if let Err(error) =
                                    handler.process_handshake_envelopes(effects.clone()).await
                                {
                                    handshake_service
                                        .record_degraded_reason(format!(
                                            "rendezvous_handshakes: {error}"
                                        ))
                                        .await;
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to process rendezvous handshake envelopes"
                                    );
                                } else {
                                    handshake_service
                                        .clear_degraded_reason_contains("rendezvous_handshakes")
                                        .await;
                                }
                                true
                            }
                        },
                    );
                } else {
                    let _handshake_task_handle = tasks.spawn_interval_until_named(
                        "rendezvous_handshakes",
                        time_effects.clone(),
                        Duration::from_secs(2),
                        move || {
                            let effects = effects.clone();
                            let handler = rendezvous_handler.clone();
                            let handshake_service = handshake_service.clone();
                            async move {
                                if let Err(error) =
                                    handler.process_handshake_envelopes(effects.clone()).await
                                {
                                    handshake_service
                                        .record_degraded_reason(format!(
                                            "rendezvous_handshakes: {error}"
                                        ))
                                        .await;
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to process rendezvous handshake envelopes"
                                    );
                                } else {
                                    handshake_service
                                        .clear_degraded_reason_contains("rendezvous_handshakes")
                                        .await;
                                }
                                true
                            }
                        },
                    );
                }
            }
        }

        if let (Some(sync_manager), Some(rendezvous_manager)) =
            (self.sync_manager.clone(), self.rendezvous_manager.clone())
        {
            let interval = sync_peer_reconcile_interval(&sync_manager);
            let reconcile_service = self.clone();
            let _reconcile_task_handle = tasks.spawn_interval_until_named(
                "sync_peer_reconcile",
                time_effects.clone(),
                interval,
                move || {
                    let sync_manager = sync_manager.clone();
                    let rendezvous_manager = rendezvous_manager.clone();
                    let reconcile_service = reconcile_service.clone();
                    async move {
                        let desired_peers: HashSet<DeviceId> = rendezvous_manager
                            .list_reachable_peer_devices()
                            .await
                            .into_iter()
                            .collect();

                        let current_peers = sync_manager.peers().await;
                        for peer in &current_peers {
                            if !desired_peers.contains(peer) {
                                sync_manager.remove_peer(peer).await;
                            }
                        }

                        for peer in desired_peers {
                            sync_manager.add_peer(peer).await;
                        }

                        reconcile_service
                            .clear_degraded_reason_contains("sync_peer_reconcile")
                            .await;
                        true
                    }
                },
            );
        }

        if let (Some(rendezvous_manager), Some(lan_transport)) =
            (self.rendezvous_manager.clone(), self.lan_transport.clone())
        {
            if let Err(error) = self.publish_initial_lan_descriptor().await {
                self.record_degraded_reason(format!("initial_lan_descriptor: {error}"))
                    .await;
                tracing::warn!(
                    event = RuntimeServiceLifecycleEvent::PostStartFailed.as_event_name(),
                    service = self.name(),
                    error = %error,
                    "Maintenance service failed to publish the initial LAN descriptor"
                );
            } else {
                self.clear_degraded_reason_contains("initial_lan_descriptor")
                    .await;
            }

            let effects = self.effects.clone();
            let authority_id = self.authority_id;
            let device_id = self.device_id;
            let lan_refresh_service = self.clone();
            let descriptor_time_effects = time_effects.clone();
            let descriptor_rendezvous_manager = rendezvous_manager.clone();
            let descriptor_lan_transport = lan_transport.clone();
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let _descriptor_refresh_task_handle = tasks.spawn_local_interval_until_named(
                        "lan_descriptor_refresh",
                        descriptor_time_effects,
                        Duration::from_secs(60),
                        move || {
                            let rendezvous_manager = descriptor_rendezvous_manager.clone();
                            let lan_transport = descriptor_lan_transport.clone();
                            let effects = effects.clone();
                            let lan_refresh_service = lan_refresh_service.clone();
                            async move {
                                let now_ms = match effects.time_effects().physical_time().await {
                                    Ok(time) => time.ts_ms,
                                    Err(error) => {
                                        lan_refresh_service
                                            .record_degraded_reason(format!(
                                                "lan_descriptor_refresh_clock: {error}"
                                            ))
                                            .await;
                                        return true;
                                    }
                                };
                                let context_id =
                                    ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
                                if rendezvous_manager.needs_refresh(context_id, now_ms).await {
                                    if let Err(error) = publish_lan_descriptor_with(
                                        effects.clone(),
                                        authority_id,
                                        device_id,
                                        &rendezvous_manager,
                                        lan_transport.as_ref(),
                                    )
                                    .await
                                    {
                                        lan_refresh_service.record_degraded_reason(format!(
                                            "lan_descriptor_refresh: {error}"
                                        ))
                                        .await;
                                        tracing::debug!(
                                            error = %error,
                                            "Failed to refresh LAN descriptor"
                                        );
                                    } else {
                                        lan_refresh_service
                                            .clear_degraded_reason_contains(
                                                "lan_descriptor_refresh_clock",
                                            )
                                            .await;
                                        lan_refresh_service.clear_degraded_reason_contains(
                                            "lan_descriptor_refresh",
                                        )
                                        .await;
                                    }
                                }
                                true
                            }
                        },
                    );
                } else {
                    let _descriptor_refresh_task_handle = tasks.spawn_interval_until_named(
                        "lan_descriptor_refresh",
                        descriptor_time_effects,
                        Duration::from_secs(60),
                        move || {
                            let rendezvous_manager = descriptor_rendezvous_manager.clone();
                            let lan_transport = descriptor_lan_transport.clone();
                            let effects = effects.clone();
                            let lan_refresh_service = lan_refresh_service.clone();
                            async move {
                                let now_ms = match effects.time_effects().physical_time().await {
                                    Ok(time) => time.ts_ms,
                                    Err(error) => {
                                        lan_refresh_service
                                            .record_degraded_reason(format!(
                                                "lan_descriptor_refresh_clock: {error}"
                                            ))
                                            .await;
                                        return true;
                                    }
                                };
                                let context_id =
                                    ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
                                if rendezvous_manager.needs_refresh(context_id, now_ms).await {
                                    if let Err(error) = publish_lan_descriptor_with(
                                        effects.clone(),
                                        authority_id,
                                        device_id,
                                        &rendezvous_manager,
                                        lan_transport.as_ref(),
                                    )
                                    .await
                                    {
                                        lan_refresh_service.record_degraded_reason(format!(
                                            "lan_descriptor_refresh: {error}"
                                        ))
                                        .await;
                                        tracing::debug!(
                                            error = %error,
                                            "Failed to refresh LAN descriptor"
                                        );
                                    } else {
                                        lan_refresh_service
                                            .clear_degraded_reason_contains(
                                                "lan_descriptor_refresh_clock",
                                            )
                                            .await;
                                        lan_refresh_service.clear_degraded_reason_contains(
                                            "lan_descriptor_refresh",
                                        )
                                        .await;
                                    }
                                }
                                true
                            }
                        },
                    );
                }
            }

            let bootstrap_refresh_service = self.clone();
            let bootstrap_time_effects = time_effects.clone();
            let bootstrap_rendezvous_manager = rendezvous_manager.clone();
            let bootstrap_lan_transport = lan_transport.clone();
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let _bootstrap_refresh_task_handle = tasks.spawn_local_interval_until_named(
                        "bootstrap_candidate_refresh",
                        bootstrap_time_effects,
                        Duration::from_secs(5),
                        move || {
                            let rendezvous_manager = bootstrap_rendezvous_manager.clone();
                            let lan_transport = bootstrap_lan_transport.clone();
                            let bootstrap_refresh_service = bootstrap_refresh_service.clone();
                            async move {
                                if let Err(error) = register_bootstrap_candidate_with(
                                    &rendezvous_manager,
                                    lan_transport.as_ref(),
                                )
                                .await
                                {
                                    bootstrap_refresh_service
                                        .record_degraded_reason(format!(
                                            "bootstrap_candidate_refresh: {error}"
                                        ))
                                        .await;
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to refresh bootstrap candidate registration"
                                    );
                                } else {
                                    bootstrap_refresh_service
                                        .clear_degraded_reason_contains("bootstrap_candidate_refresh")
                                        .await;
                                }
                                true
                            }
                        },
                    );
                } else {
                    let _bootstrap_refresh_task_handle = tasks.spawn_interval_until_named(
                        "bootstrap_candidate_refresh",
                        bootstrap_time_effects,
                        Duration::from_secs(5),
                        move || {
                            let rendezvous_manager = bootstrap_rendezvous_manager.clone();
                            let lan_transport = bootstrap_lan_transport.clone();
                            let bootstrap_refresh_service = bootstrap_refresh_service.clone();
                            async move {
                                if let Err(error) = register_bootstrap_candidate_with(
                                    &rendezvous_manager,
                                    lan_transport.as_ref(),
                                )
                                .await
                                {
                                    bootstrap_refresh_service
                                        .record_degraded_reason(format!(
                                            "bootstrap_candidate_refresh: {error}"
                                        ))
                                        .await;
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to refresh bootstrap candidate registration"
                                    );
                                } else {
                                    bootstrap_refresh_service
                                        .clear_degraded_reason_contains("bootstrap_candidate_refresh")
                                        .await;
                                }
                                true
                            }
                        },
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn publish_initial_lan_descriptor(&self) -> Result<(), ServiceError> {
        let (Some(rendezvous_manager), Some(lan_transport)) = (
            self.rendezvous_manager.as_ref(),
            self.lan_transport.as_ref(),
        ) else {
            return Ok(());
        };

        publish_lan_descriptor_with(
            self.effects.clone(),
            self.authority_id,
            self.device_id,
            rendezvous_manager,
            lan_transport.as_ref(),
        )
        .await
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for RuntimeMaintenanceService {
    fn name(&self) -> &'static str {
        "runtime_maintenance"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["rendezvous_manager", "sync_manager"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        match self.shared.owner.state().await {
            MaintenanceServiceState::Stopped => ServiceHealth::Stopped,
            MaintenanceServiceState::Starting => ServiceHealth::Starting,
            MaintenanceServiceState::Stopping => ServiceHealth::Stopping,
            MaintenanceServiceState::Failed => ServiceHealth::Unhealthy {
                reason: "maintenance service entered failed lifecycle state".to_string(),
            },
            MaintenanceServiceState::Running => {
                if self.shared.owner.has_tasks().await {
                    let degraded = self.shared.degraded_reasons.read().await;
                    if degraded.is_empty() {
                        ServiceHealth::Healthy
                    } else {
                        ServiceHealth::Degraded {
                            reason: degraded.join("; "),
                        }
                    }
                } else {
                    ServiceHealth::Unhealthy {
                        reason: "maintenance task group missing".to_string(),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::TaskSupervisor;
    use aura_core::effects::time::PhysicalTimeEffects;

    #[tokio::test]
    async fn maintenance_service_reports_degraded_when_failures_are_recorded() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let config = crate::core::AgentConfig {
            device_id: DeviceId::new_from_entropy([2u8; 32]),
            ..Default::default()
        };
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&config, authority_id).unwrap(),
        );
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(effects.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects.clone());
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let threshold_signing = ThresholdSigningService::new(effects.clone());
        let reconfiguration = ReconfigurationManager::new();
        let service = RuntimeMaintenanceService::new(
            effects.clone(),
            authority_id,
            config.device_id,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration,
            None,
            None,
            None,
            None,
        );
        service
            .shared
            .owner
            .set_state(MaintenanceServiceState::Running)
            .await;
        let tasks = TaskSupervisor::new();
        service
            .shared
            .owner
            .install_tasks(tasks.group("maintenance_test"))
            .await;
        service
            .record_degraded_reason("initial_lan_descriptor: publish failed")
            .await;

        let health = service.health().await;
        assert_eq!(
            health,
            ServiceHealth::Degraded {
                reason: "initial_lan_descriptor: publish failed".to_string(),
            }
        );
    }
}

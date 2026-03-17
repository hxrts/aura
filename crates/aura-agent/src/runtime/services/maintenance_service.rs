use super::service_actor::{validate_actor_transition, ActorLifecyclePhase};
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use super::{LanTransportService, RendezvousManager, SyncServiceManager};
use crate::core::AuthorityContext;
use crate::handlers::{InvitationHandler, RendezvousHandler};
use crate::runtime::system::{publish_lan_descriptor_with, sync_peer_reconcile_interval};
use crate::runtime::AuraEffectSystem;
use crate::runtime::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

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
    tasks: RwLock<Option<TaskGroup>>,
    state: RwLock<MaintenanceServiceState>,
    lifecycle: Mutex<()>,
}

#[derive(Clone)]
pub struct RuntimeMaintenanceService {
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    device_id: DeviceId,
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
        sync_manager: Option<SyncServiceManager>,
        rendezvous_manager: Option<RendezvousManager>,
        rendezvous_handler: Option<RendezvousHandler>,
        lan_transport: Option<Arc<LanTransportService>>,
    ) -> Self {
        Self {
            effects,
            authority_id,
            device_id,
            sync_manager,
            rendezvous_manager,
            rendezvous_handler,
            lan_transport,
            shared: Arc::new(RuntimeMaintenanceShared {
                tasks: RwLock::new(None),
                state: RwLock::new(MaintenanceServiceState::Stopped),
                lifecycle: Mutex::new(()),
            }),
        }
    }

    async fn mark_state(&self, next: MaintenanceServiceState) {
        *self.shared.state.write().await = next;
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == MaintenanceServiceState::Running {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Starting)?;
        self.mark_state(MaintenanceServiceState::Starting).await;

        let tasks = context.tasks().group(self.name());
        self.spawn_loops(tasks.clone(), context.time_effects())
            .await?;
        *self.shared.tasks.write().await = Some(tasks);
        self.mark_state(MaintenanceServiceState::Running).await;
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == MaintenanceServiceState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(MaintenanceServiceState::Stopping).await;

        let shutdown_error = if let Some(tasks) = self.shared.tasks.write().await.take() {
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
        let effects = self.effects.clone();
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                tasks.spawn_local_interval_until_named(
                    "invitation_acceptance",
                    time_effects.clone(),
                    Duration::from_secs(2),
                    move || {
                        let effects = effects.clone();
                        let handler = invitation_handler.clone();
                        async move {
                            if let Err(error) = handler
                                .process_contact_invitation_acceptances(effects.clone())
                                .await
                            {
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process contact invitation acceptances"
                                );
                            }
                            true
                        }
                    },
                );
            } else {
                tasks.spawn_interval_until_named(
                    "invitation_acceptance",
                    time_effects.clone(),
                    Duration::from_secs(2),
                    move || {
                        let effects = effects.clone();
                        let handler = invitation_handler.clone();
                        async move {
                            if let Err(error) = handler
                                .process_contact_invitation_acceptances(effects.clone())
                                .await
                            {
                                tracing::debug!(
                                    error = %error,
                                    "Failed to process contact invitation acceptances"
                                );
                            }
                            true
                        }
                    },
                );
            }
        }

        if let Some(rendezvous_handler) = self.rendezvous_handler.clone() {
            let effects = self.effects.clone();
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    tasks.spawn_local_interval_until_named(
                        "rendezvous_handshakes",
                        time_effects.clone(),
                        Duration::from_secs(2),
                        move || {
                            let effects = effects.clone();
                            let handler = rendezvous_handler.clone();
                            async move {
                                if let Err(error) =
                                    handler.process_handshake_envelopes(effects.clone()).await
                                {
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to process rendezvous handshake envelopes"
                                    );
                                }
                                true
                            }
                        },
                    );
                } else {
                    tasks.spawn_interval_until_named(
                        "rendezvous_handshakes",
                        time_effects.clone(),
                        Duration::from_secs(2),
                        move || {
                            let effects = effects.clone();
                            let handler = rendezvous_handler.clone();
                            async move {
                                if let Err(error) =
                                    handler.process_handshake_envelopes(effects.clone()).await
                                {
                                    tracing::debug!(
                                        error = %error,
                                        "Failed to process rendezvous handshake envelopes"
                                    );
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
            tasks.spawn_interval_until_named(
                "sync_peer_reconcile",
                time_effects.clone(),
                interval,
                move || {
                    let sync_manager = sync_manager.clone();
                    let rendezvous_manager = rendezvous_manager.clone();
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

                        true
                    }
                },
            );
        }

        if let (Some(rendezvous_manager), Some(lan_transport)) =
            (self.rendezvous_manager.clone(), self.lan_transport.clone())
        {
            if let Err(error) = publish_lan_descriptor_with(
                self.effects.clone(),
                self.authority_id,
                self.device_id,
                &rendezvous_manager,
                lan_transport.as_ref(),
            )
            .await
            {
                tracing::warn!(
                    event = "runtime.service.lifecycle.post_start_failed",
                    service = self.name(),
                    error = %error,
                    "Maintenance service failed to publish the initial LAN descriptor"
                );
            }

            let effects = self.effects.clone();
            let authority_id = self.authority_id;
            let device_id = self.device_id;
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    tasks.spawn_local_interval_until_named(
                        "lan_descriptor_refresh",
                        time_effects,
                        Duration::from_secs(60),
                        move || {
                            let rendezvous_manager = rendezvous_manager.clone();
                            let lan_transport = lan_transport.clone();
                            let effects = effects.clone();
                            async move {
                                let now_ms = match effects.time_effects().physical_time().await {
                                    Ok(time) => time.ts_ms,
                                    Err(_) => return true,
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
                                        tracing::debug!(
                                            error = %error,
                                            "Failed to refresh LAN descriptor"
                                        );
                                    }
                                }
                                true
                            }
                        },
                    );
                } else {
                    tasks.spawn_interval_until_named(
                        "lan_descriptor_refresh",
                        time_effects,
                        Duration::from_secs(60),
                        move || {
                            let rendezvous_manager = rendezvous_manager.clone();
                            let lan_transport = lan_transport.clone();
                            let effects = effects.clone();
                            async move {
                                let now_ms = match effects.time_effects().physical_time().await {
                                    Ok(time) => time.ts_ms,
                                    Err(_) => return true,
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
                                        tracing::debug!(
                                            error = %error,
                                            "Failed to refresh LAN descriptor"
                                        );
                                    }
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
        match *self.shared.state.read().await {
            MaintenanceServiceState::Stopped => ServiceHealth::Stopped,
            MaintenanceServiceState::Starting => ServiceHealth::Starting,
            MaintenanceServiceState::Stopping => ServiceHealth::Stopping,
            MaintenanceServiceState::Failed => ServiceHealth::Unhealthy {
                reason: "maintenance service entered failed lifecycle state".to_string(),
            },
            MaintenanceServiceState::Running => {
                if self.shared.tasks.read().await.is_some() {
                    ServiceHealth::Healthy
                } else {
                    ServiceHealth::Unhealthy {
                        reason: "maintenance task group missing".to_string(),
                    }
                }
            }
        }
    }
}

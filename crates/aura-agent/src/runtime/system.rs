//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::ceremony_runner::CeremonyRunner;
use super::services::{
    AuthorityManager, AuthorityStatus, CeremonyTracker, ContextManager, FlowBudgetManager,
    LanTransportService, ReceiptManager, RendezvousManager, RuntimeService, RuntimeTaskRegistry,
    ServiceError, SocialManager, SyncServiceManager, ThresholdSigningService,
};
use super::{
    AuraEffectSystem, ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager,
};
use crate::core::{AgentConfig, AuthorityContext};
use crate::handlers::{InvitationHandler, RendezvousHandler};
use crate::reactive::{FactSource, ReactivePipeline, SchedulerConfig};
use aura_core::effects::task::TaskSpawner;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::DeviceId;
use aura_rendezvous::TransportHint;
use tokio::io::AsyncReadExt;
use std::sync::Arc;
use std::time::Duration;

/// Main runtime system for the agent
pub struct RuntimeSystem {
    /// Effect executor
    #[allow(dead_code)] // Will be used for effect dispatch
    effect_executor: EffectExecutor,

    /// Effect system (immutable after construction, handlers have internal mutability)
    effect_system: Arc<AuraEffectSystem>,

    /// Context manager
    context_manager: ContextManager,

    /// Authority manager
    authority_manager: AuthorityManager,

    /// Flow budget manager
    flow_budget_manager: FlowBudgetManager,

    /// Receipt manager
    receipt_manager: ReceiptManager,

    /// Choreography adapter
    choreography_adapter: ChoreographyAdapter,

    /// Lifecycle manager
    lifecycle_manager: LifecycleManager,

    /// Sync service manager (optional, for background journal synchronization)
    sync_manager: Option<SyncServiceManager>,

    /// Rendezvous manager (optional, for peer discovery and channel establishment)
    rendezvous_manager: Option<RendezvousManager>,

    /// Rendezvous handler (optional, for handshake processing)
    rendezvous_handler: Option<RendezvousHandler>,

    /// LAN transport service (optional, for TCP listener + advertise addrs)
    lan_transport: Option<Arc<LanTransportService>>,

    /// Social manager (optional, for social topology and relay selection)
    social_manager: Option<SocialManager>,

    /// Ceremony tracker (for guardian ceremony coordination)
    ceremony_tracker: CeremonyTracker,

    /// Ceremony runner (shared Category C orchestration API)
    ceremony_runner: CeremonyRunner,

    /// Threshold signing service (shared state across runtime operations)
    threshold_signing: ThresholdSigningService,

    /// Runtime task registry for background work
    runtime_tasks: Arc<RuntimeTaskRegistry>,

    /// Configuration
    #[allow(dead_code)] // Will be used for runtime configuration
    config: AgentConfig,

    /// Authority ID
    authority_id: AuthorityId,

    /// Reactive scheduler pipeline (facts → scheduler → view updates).
    ///
    /// This is optional while the single fact pipeline work is being completed.
    reactive_pipeline: Option<ReactivePipeline>,
}

impl RuntimeSystem {
    /// Create a new runtime system
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future runtime wiring
    pub(crate) fn new(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        authority_manager: AuthorityManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new();
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: None,
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
        }
    }

    /// Create a new runtime system with sync service
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future sync-enabled runtime
    pub(crate) fn new_with_sync(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        authority_manager: AuthorityManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        sync_manager: SyncServiceManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new();
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: Some(sync_manager),
            rendezvous_manager: None,
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
        }
    }

    /// Create a new runtime system with rendezvous service
    #[allow(clippy::too_many_arguments)]
    #[allow(dead_code)] // Factory retained for future rendezvous-enabled runtime
    pub(crate) fn new_with_rendezvous(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        authority_manager: AuthorityManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        rendezvous_manager: RendezvousManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new();
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: Some(rendezvous_manager),
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
        }
    }

    /// Create a new runtime system with all services
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_with_services(
        effect_executor: EffectExecutor,
        effect_system: Arc<AuraEffectSystem>,
        context_manager: ContextManager,
        authority_manager: AuthorityManager,
        flow_budget_manager: FlowBudgetManager,
        receipt_manager: ReceiptManager,
        choreography_adapter: ChoreographyAdapter,
        lifecycle_manager: LifecycleManager,
        sync_manager: Option<SyncServiceManager>,
        rendezvous_manager: Option<RendezvousManager>,
        rendezvous_handler: Option<RendezvousHandler>,
        lan_transport: Option<Arc<LanTransportService>>,
        social_manager: Option<SocialManager>,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new();
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            rendezvous_handler,
            lan_transport,
            social_manager,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            runtime_tasks: Arc::new(RuntimeTaskRegistry::new()),
            config,
            authority_id,
            reactive_pipeline: None,
        }
    }

    /// Get the ceremony tracker
    pub fn ceremony_tracker(&self) -> &CeremonyTracker {
        &self.ceremony_tracker
    }

    /// Get the ceremony runner
    pub fn ceremony_runner(&self) -> &CeremonyRunner {
        &self.ceremony_runner
    }

    /// Get the shared threshold signing service.
    pub fn threshold_signing(&self) -> ThresholdSigningService {
        self.threshold_signing.clone()
    }

    /// Get the runtime task registry.
    pub fn tasks(&self) -> Arc<RuntimeTaskRegistry> {
        self.runtime_tasks.clone()
    }

    /// Start background maintenance tasks (cleanup, pruning).
    pub fn start_maintenance_tasks(&self) {
        let tasks = self.runtime_tasks.clone();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        let ceremony_tracker = self.ceremony_tracker.clone();
        let sync_manager = self.sync_manager.clone();
        let rendezvous_manager = self.rendezvous_manager.clone();
        let rendezvous_handler = self.rendezvous_handler.clone();
        let lan_transport = self.lan_transport.clone();
        let effects = self.effect_system.clone();
        let authority_id = self.authority_id;
        let device_id = self.device_id();

        // Sync service maintains its own cleanup schedule.
        if let Some(sync_manager) = sync_manager.as_ref() {
            sync_manager.start_maintenance_task(tasks.clone(), time_effects.clone());
        }

        if let Ok(invitation_handler) =
            InvitationHandler::new(AuthorityContext::new_with_device(
                self.authority_id,
                self.device_id(),
            ))
        {
            let effects = self.effect_system.clone();
            let handler = invitation_handler.clone();
            let interval = Duration::from_secs(2);
            tasks.spawn_interval_until(time_effects.clone(), interval, move || {
                let effects = effects.clone();
                let handler = handler.clone();
                async move {
                    if let Err(e) = handler
                        .process_contact_invitation_acceptances(effects.clone())
                        .await
                    {
                        tracing::debug!(
                            error = %e,
                            "Failed to process contact invitation acceptances"
                        );
                    }
                    true
                }
            });
        }

        if let Some(rendezvous_handler) = rendezvous_handler {
            let effects = self.effect_system.clone();
            let handler = rendezvous_handler.clone();
            let interval = Duration::from_secs(2);
            tasks.spawn_interval_until(time_effects.clone(), interval, move || {
                let effects = effects.clone();
                let handler = handler.clone();
                async move {
                    if let Err(e) = handler
                        .process_handshake_envelopes(effects.clone())
                        .await
                    {
                        tracing::debug!(
                            error = %e,
                            "Failed to process rendezvous handshake envelopes"
                        );
                    }
                    true
                }
            });
        }

        if let (Some(sync_manager), Some(rendezvous_manager)) =
            (sync_manager.clone(), rendezvous_manager.clone())
        {
            let interval = Duration::from_secs(30);
            tasks.spawn_interval_until(time_effects.clone(), interval, move || {
                let sync_manager = sync_manager.clone();
                let rendezvous_manager = rendezvous_manager.clone();
                async move {
                    let desired_peers: std::collections::HashSet<DeviceId> = rendezvous_manager
                        .list_cached_peers()
                        .await
                        .into_iter()
                        .map(|peer| DeviceId::from_uuid(peer.uuid()))
                        .collect();

                    let current_peers = sync_manager.peers().await;
                    for peer in current_peers.iter() {
                        if !desired_peers.contains(peer) {
                            sync_manager.remove_peer(peer).await;
                        }
                    }

                    for peer in desired_peers {
                        sync_manager.add_peer(peer).await;
                    }

                    true
                }
            });
        }

        // General runtime maintenance loop (ceremony timeouts, etc.).
        let interval = Duration::from_secs(60);
        tasks.spawn_interval_until(time_effects.clone(), interval, move || {
            let ceremony_tracker = ceremony_tracker.clone();
            async move {
                let _ = ceremony_tracker.cleanup_timed_out().await;
                true
            }
        });

        if let (Some(rendezvous_manager), Some(lan_transport)) =
            (rendezvous_manager, lan_transport)
        {
            let interval = Duration::from_secs(60);
            tasks.spawn_interval_until(time_effects.clone(), interval, move || {
                let rendezvous_manager = rendezvous_manager.clone();
                let lan_transport = lan_transport.clone();
                let effects = effects.clone();
                async move {
                    let now_ms = match effects.time_effects().physical_time().await {
                        Ok(t) => t.ts_ms,
                        Err(_) => return true,
                    };
                    let context_id = ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
                    if rendezvous_manager.needs_refresh(context_id, now_ms).await {
                        if let Err(e) = publish_lan_descriptor_with(
                            effects.clone(),
                            authority_id,
                            device_id,
                            &rendezvous_manager,
                            lan_transport.as_ref(),
                        )
                        .await
                        {
                            tracing::debug!(error = %e, "Failed to refresh LAN descriptor");
                        }
                    }
                    true
                }
            });
        }
    }

    /// Get the runtime task spawner as a trait object.
    pub fn task_spawner(&self) -> Arc<dyn TaskSpawner> {
        self.runtime_tasks.clone()
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Device id for this runtime instance.
    pub fn device_id(&self) -> DeviceId {
        self.config.device_id
    }

    /// Get the effect system
    ///
    /// Returns a shared reference to the effect system. The effect system is
    /// immutable after construction; individual handlers manage their own
    /// internal state as needed.
    pub fn effects(&self) -> Arc<AuraEffectSystem> {
        self.effect_system.clone()
    }

    /// Start the reactive scheduler pipeline (facts → scheduler → view updates).
    ///
    /// This is a best-effort wiring step; the pipeline becomes fully useful once
    /// the runtime publishes typed facts into it (work/002.md C2).
    pub async fn start_reactive_pipeline(&mut self) -> Result<(), String> {
        if self.reactive_pipeline.is_some() {
            return Ok(());
        }

        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(self.effect_system.time_effects().clone());

        let pipeline = ReactivePipeline::start(
            SchedulerConfig::default(),
            self.effect_system.fact_registry(),
            time_effects,
            self.effect_system.clone(),
            self.authority_id,
            self.effect_system.reactive_handler(),
        );

        // Attach the scheduler ingestion channel to the effect system so all journal commits
        // go through the single typed-fact pipeline (work/002.md C2).
        self.effect_system.attach_fact_sink(pipeline.fact_sender());

        // Attach the view update sender so callers can await fact processing completion.
        self.effect_system
            .attach_view_update_sender(pipeline.update_sender());

        // Replay existing persisted facts to seed scheduler-driven UI signals.
        let existing = self
            .effect_system
            .load_committed_facts(self.authority_id)
            .await
            .map_err(|e| e.to_string())?;
        if !existing.is_empty() {
            let _ = pipeline
                .fact_sender()
                .send(FactSource::Journal(existing))
                .await;
        }

        self.reactive_pipeline = Some(pipeline);
        Ok(())
    }

    /// Access the running reactive pipeline (if started).
    pub fn reactive_pipeline(&self) -> Option<&ReactivePipeline> {
        self.reactive_pipeline.as_ref()
    }

    /// Start runtime services using the RuntimeService trait.
    pub async fn start_services(&self) -> Result<(), ServiceError> {
        let now_ms = self
            .effect_system
            .time_effects()
            .physical_time()
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?
            .ts_ms;
        self.authority_manager
            .ensure_authority(self.authority_id, now_ms)
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?;
        self.authority_manager
            .set_status(self.authority_id, AuthorityStatus::Active, now_ms)
            .await
            .map_err(|e| ServiceError::startup_failed("authority_manager", e.to_string()))?;

        self.flow_budget_manager
            .start(self.runtime_tasks.clone())
            .await?;
        self.receipt_manager
            .start(self.runtime_tasks.clone())
            .await?;
        self.ceremony_tracker
            .start(self.runtime_tasks.clone())
            .await?;
        self.threshold_signing
            .start(self.runtime_tasks.clone())
            .await?;

        if let Some(social_manager) = &self.social_manager {
            social_manager.start(self.runtime_tasks.clone()).await?;
        }
        if let Some(rendezvous_manager) = &self.rendezvous_manager {
            RuntimeService::start(rendezvous_manager, self.runtime_tasks.clone()).await?;
            if let Err(e) = self.publish_lan_descriptor().await {
                tracing::warn!(error = %e, "Failed to publish LAN descriptor");
            }
        }
        if self.lan_transport.is_some() {
            self.start_lan_transport_listener();
        }
        if let Some(sync_manager) = &self.sync_manager {
            let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
                Arc::new(self.effect_system.time_effects().clone());
            sync_manager
                .start(time_effects)
                .await
                .map_err(|e| ServiceError::startup_failed("sync_service", e))?;
        }

        Ok(())
    }

    async fn publish_lan_descriptor(&self) -> Result<(), ServiceError> {
        let Some(rendezvous_manager) = &self.rendezvous_manager else {
            return Ok(());
        };
        let Some(lan_transport) = &self.lan_transport else {
            return Ok(());
        };
        publish_lan_descriptor_with(
            self.effect_system.clone(),
            self.authority_id,
            self.device_id(),
            rendezvous_manager,
            lan_transport.as_ref(),
        )
        .await
    }

    fn start_lan_transport_listener(&self) {
        let Some(lan_transport) = &self.lan_transport else {
            return;
        };

        let listener = lan_transport.listener();
        let effects = self.effect_system.clone();
        let metrics = lan_transport.metrics_handle();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(effects.time_effects().clone());
        self.runtime_tasks.spawn_cancellable(async move {
            loop {
                let (mut stream, addr) = match listener.accept().await {
                    Ok((stream, addr)) => (stream, addr),
                    Err(err) => {
                        tracing::warn!(error = %err, "LAN transport accept failed");
                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .ok()
                            .map(|t| t.ts_ms)
                            .unwrap_or(0);
                        let mut metrics = metrics.write().await;
                        metrics.accept_errors = metrics.accept_errors.saturating_add(1);
                        if now_ms > 0 {
                            metrics.last_error_ms = now_ms;
                        }
                        continue;
                    }
                };

                let effects = effects.clone();
                let metrics = metrics.clone();
                let time_effects = time_effects.clone();
                let now_ms = time_effects
                    .physical_time()
                    .await
                    .ok()
                    .map(|t| t.ts_ms)
                    .unwrap_or(0);
                {
                    let mut metrics = metrics.write().await;
                    metrics.connections_accepted = metrics.connections_accepted.saturating_add(1);
                    if now_ms > 0 {
                        metrics.last_accept_ms = now_ms;
                    }
                }
                tokio::spawn(async move {
                    let mut len_buf = [0u8; 4];
                    if let Err(err) = stream.read_exact(&mut len_buf).await {
                        tracing::debug!(error = %err, addr = %addr, "LAN transport read len failed");
                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .ok()
                            .map(|t| t.ts_ms)
                            .unwrap_or(0);
                        let mut metrics = metrics.write().await;
                        metrics.read_errors = metrics.read_errors.saturating_add(1);
                        if now_ms > 0 {
                            metrics.last_error_ms = now_ms;
                        }
                        return;
                    }
                    let len = u32::from_be_bytes(len_buf) as usize;
                    if len == 0 || len > 1024 * 1024 {
                        tracing::debug!(addr = %addr, len = len, "LAN transport invalid frame size");
                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .ok()
                            .map(|t| t.ts_ms)
                            .unwrap_or(0);
                        let mut metrics = metrics.write().await;
                        metrics.decode_errors = metrics.decode_errors.saturating_add(1);
                        if now_ms > 0 {
                            metrics.last_error_ms = now_ms;
                        }
                        return;
                    }
                    let mut payload = vec![0u8; len];
                    if let Err(err) = stream.read_exact(&mut payload).await {
                        tracing::debug!(error = %err, addr = %addr, "LAN transport read payload failed");
                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .ok()
                            .map(|t| t.ts_ms)
                            .unwrap_or(0);
                        let mut metrics = metrics.write().await;
                        metrics.read_errors = metrics.read_errors.saturating_add(1);
                        if now_ms > 0 {
                            metrics.last_error_ms = now_ms;
                        }
                        return;
                    }

                    let envelope: aura_core::effects::transport::TransportEnvelope =
                        match aura_core::util::serialization::from_slice(&payload) {
                            Ok(envelope) => envelope,
                            Err(err) => {
                                tracing::debug!(error = %err, addr = %addr, "LAN transport decode failed");
                                let now_ms = time_effects
                                    .physical_time()
                                    .await
                                    .ok()
                                    .map(|t| t.ts_ms)
                                    .unwrap_or(0);
                                let mut metrics = metrics.write().await;
                                metrics.decode_errors = metrics.decode_errors.saturating_add(1);
                                if now_ms > 0 {
                                    metrics.last_error_ms = now_ms;
                                }
                                return;
                            }
                        };
                    let now_ms = time_effects
                        .physical_time()
                        .await
                        .ok()
                        .map(|t| t.ts_ms)
                        .unwrap_or(0);
                    {
                        let mut metrics = metrics.write().await;
                        metrics.frames_received = metrics.frames_received.saturating_add(1);
                        metrics.bytes_received = metrics.bytes_received.saturating_add(len as u64);
                        if now_ms > 0 {
                            metrics.last_frame_ms = now_ms;
                        }
                    }
                    effects.requeue_envelope(envelope);
                });
            }
        });
    }

    async fn stop_services(&self) -> Result<(), ServiceError> {
        let now_ms = self
            .effect_system
            .time_effects()
            .physical_time()
            .await
            .map_err(|e| ServiceError::shutdown_failed("authority_manager", e.to_string()))?
            .ts_ms;
        self.authority_manager
            .set_status(self.authority_id, AuthorityStatus::Terminated, now_ms)
            .await
            .map_err(|e| ServiceError::shutdown_failed("authority_manager", e.to_string()))?;

        if let Some(sync_manager) = &self.sync_manager {
            RuntimeService::stop(sync_manager).await?;
        }
        if let Some(rendezvous_manager) = &self.rendezvous_manager {
            RuntimeService::stop(rendezvous_manager).await?;
        }
        if let Some(social_manager) = &self.social_manager {
            RuntimeService::stop(social_manager).await?;
        }
        self.threshold_signing.stop().await?;
        self.ceremony_tracker.stop().await?;
        self.receipt_manager.stop().await?;
        self.flow_budget_manager.stop().await?;

        Ok(())
    }

    /// Get the context manager
    pub fn contexts(&self) -> &ContextManager {
        &self.context_manager
    }

    /// Get the authority manager
    pub fn authorities(&self) -> &AuthorityManager {
        &self.authority_manager
    }

    /// Get the flow budget manager
    pub fn flow_budgets(&self) -> &FlowBudgetManager {
        &self.flow_budget_manager
    }

    /// Get the receipt manager
    pub fn receipts(&self) -> &ReceiptManager {
        &self.receipt_manager
    }

    /// Get the choreography adapter
    pub fn choreography(&self) -> &ChoreographyAdapter {
        &self.choreography_adapter
    }

    /// Get the lifecycle manager
    pub fn lifecycle(&self) -> &LifecycleManager {
        &self.lifecycle_manager
    }

    /// Get the sync service manager (if enabled)
    pub fn sync(&self) -> Option<&SyncServiceManager> {
        self.sync_manager.as_ref()
    }

    /// Check if sync service is enabled
    pub fn has_sync(&self) -> bool {
        self.sync_manager.is_some()
    }

    /// Get the rendezvous manager (if enabled)
    pub fn rendezvous(&self) -> Option<&RendezvousManager> {
        self.rendezvous_manager.as_ref()
    }

    /// Check if rendezvous service is enabled
    pub fn has_rendezvous(&self) -> bool {
        self.rendezvous_manager.is_some()
    }

    /// Get the social manager (if enabled)
    pub fn social(&self) -> Option<&SocialManager> {
        self.social_manager.as_ref()
    }

    /// Check if social service is enabled
    pub fn has_social(&self) -> bool {
        self.social_manager.is_some()
    }

    /// Shutdown the runtime system
    pub async fn shutdown(self, ctx: &EffectContext) -> Result<(), String> {
        // Stop services (best-effort) before tearing down the runtime.
        if let Err(e) = self.stop_services().await {
            tracing::warn!("Failed to stop runtime services during shutdown: {}", e);
        }

        let RuntimeSystem {
            lifecycle_manager,
            sync_manager: _sync_manager,
            rendezvous_manager: _rendezvous_manager,
            reactive_pipeline,
            runtime_tasks,
            ..
        } = self;

        // Stop reactive pipeline (scheduler task) if running.
        if let Some(pipeline) = reactive_pipeline {
            pipeline.shutdown().await;
        }

        // Stop background runtime tasks (invitation monitors, subscriptions).
        runtime_tasks.shutdown();

        lifecycle_manager.shutdown(ctx).await
    }
}

async fn publish_lan_descriptor_with(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    device_id: DeviceId,
    rendezvous_manager: &RendezvousManager,
    lan_transport: &LanTransportService,
) -> Result<(), ServiceError> {
    let authority_context = AuthorityContext::new_with_device(authority_id, device_id);
    let handler = RendezvousHandler::new(authority_context.clone())
        .map_err(|e| ServiceError::startup_failed("rendezvous_handler", e.to_string()))?;
    let context_id = authority_context.default_context_id();

    let mut hints = Vec::new();
    for addr in lan_transport.advertised_addrs() {
        match TransportHint::tcp_direct(addr) {
            Ok(hint) => hints.push(hint),
            Err(err) => {
                tracing::warn!(addr = %addr, error = %err, "Skipping invalid LAN transport hint");
            }
        }
    }

    if hints.is_empty() {
        tracing::warn!("No valid LAN transport addresses to advertise");
        return Ok(());
    }

    let result = handler
        .publish_descriptor(&effects, context_id, hints, [0u8; 32], 0)
        .await
        .map_err(|e| ServiceError::startup_failed("rendezvous_publish", e.to_string()))?;

    if result.success {
        if let Some(descriptor) = result.descriptor {
            rendezvous_manager
                .cache_descriptor(descriptor.clone())
                .await
                .map_err(|e| ServiceError::startup_failed("rendezvous_cache", e))?;
            rendezvous_manager.set_lan_descriptor(descriptor).await;
        } else {
            tracing::warn!("LAN descriptor publish succeeded without descriptor payload");
        }
    } else {
        tracing::warn!(
            error = %result.error.unwrap_or_else(|| "unknown error".to_string()),
            "LAN descriptor publish failed"
        );
    }

    Ok(())
}

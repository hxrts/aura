//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::ceremony_runner::CeremonyRunner;
use super::services::{
    AuthorityManager, AuthorityStatus, CeremonyTracker, ContextManager, FlowBudgetManager,
    LanTransportService, ReceiptManager, ReconfigurationManager, RendezvousManager, RuntimeService,
    RuntimeServiceContext, RuntimeTaskRegistry, ServiceError, ServiceErrorKind, ServiceHealth,
    SocialManager, SyncServiceManager, ThresholdSigningService,
};
use super::{
    AuraEffectSystem, EffectContext, EffectExecutor, LifecycleManager, RuntimeDiagnosticSink,
};
use crate::core::{AgentConfig, AuthorityContext};
use crate::handlers::{InvitationHandler, RendezvousHandler};
use crate::reactive::{ReactivePipeline, SchedulerConfig};
use crate::task_registry::TaskSupervisionError;
#[cfg(not(target_arch = "wasm32"))]
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::effects::task::TaskSpawner;
use aura_core::effects::time::PhysicalTimeEffects;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::transport::TransportEnvelope;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::{AmpChannelEffects, ChannelCreateParams, ChannelJoinParams};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
#[cfg(not(target_arch = "wasm32"))]
use aura_core::util::serialization::from_slice;
use aura_core::DeviceId;
#[cfg(not(target_arch = "wasm32"))]
use aura_guards::GuardContextProvider;
#[cfg(not(target_arch = "wasm32"))]
use aura_journal::fact::{FactContent, RelationalFact};
#[cfg(not(target_arch = "wasm32"))]
use aura_journal::DomainFact;
#[cfg(not(target_arch = "wasm32"))]
use aura_protocol::amp::get_channel_state;
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
#[cfg(not(target_arch = "wasm32"))]
use futures::{SinkExt, StreamExt};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncReadExt;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::accept_async;

const MIN_SYNC_PEER_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_SYNC_PEER_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
#[cfg(not(target_arch = "wasm32"))]
const CHAT_FACT_CONTENT_TYPE: &str = "application/aura-chat-fact";
#[cfg(not(target_arch = "wasm32"))]
const FACT_SYNC_REQUEST_CONTENT_TYPE: &str = "application/aura-fact-sync-request";
#[cfg(not(target_arch = "wasm32"))]
const FACT_SYNC_RESPONSE_CONTENT_TYPE: &str = "application/aura-fact-sync-response";

fn sync_peer_reconcile_interval(sync_manager: &SyncServiceManager) -> Duration {
    sync_manager.config().auto_sync_interval.clamp(
        MIN_SYNC_PEER_RECONCILE_INTERVAL,
        MAX_SYNC_PEER_RECONCILE_INTERVAL,
    )
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeShutdownError {
    #[error("runtime task tree shutdown failed: {0}")]
    TaskTree(#[from] TaskSupervisionError),
    #[error("runtime service teardown failed: {0}")]
    Service(#[from] ServiceError),
    #[error("lifecycle shutdown failed: {0}")]
    Lifecycle(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeActivityState {
    Running,
    Stopping,
    Stopped,
}

impl RuntimeActivityState {
    fn as_u8(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::Stopping => 1,
            Self::Stopped => 2,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Stopping,
            2 => Self::Stopped,
            _ => Self::Stopped,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimePublicOperationError {
    #[error("runtime is {state:?} and no longer accepts new public operations")]
    NotAccepting { state: RuntimeActivityState },
}

#[derive(Debug, Default)]
pub struct RuntimeActivityGate {
    state: AtomicU8,
}

impl RuntimeActivityGate {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(RuntimeActivityState::Running.as_u8()),
        }
    }

    pub fn state(&self) -> RuntimeActivityState {
        RuntimeActivityState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn begin_shutdown(&self) -> RuntimeActivityState {
        match self.state.compare_exchange(
            RuntimeActivityState::Running.as_u8(),
            RuntimeActivityState::Stopping.as_u8(),
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => RuntimeActivityState::Running,
            Err(previous) => RuntimeActivityState::from_u8(previous),
        }
    }

    pub fn mark_stopped(&self) {
        self.state
            .store(RuntimeActivityState::Stopped.as_u8(), Ordering::SeqCst);
    }

    pub fn ensure_accepting_public_operations(
        &self,
    ) -> Result<(), RuntimePublicOperationError> {
        match self.state() {
            RuntimeActivityState::Running => Ok(()),
            state => Err(RuntimePublicOperationError::NotAccepting { state }),
        }
    }
}

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

    /// Reconfiguration manager for link/delegate operations.
    reconfiguration_manager: ReconfigurationManager,

    /// Runtime task registry for background work
    runtime_tasks: Arc<RuntimeTaskRegistry>,

    /// Shared runtime activity gate used to reject new public work during shutdown.
    activity_gate: Arc<RuntimeActivityGate>,

    /// Shared diagnostics sink for surfaced async/runtime failures.
    diagnostics: Arc<RuntimeDiagnosticSink>,

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
        lifecycle_manager: LifecycleManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let runtime_tasks = Arc::new(RuntimeTaskRegistry::with_diagnostics(diagnostics.clone()));
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: None,
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
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
        lifecycle_manager: LifecycleManager,
        sync_manager: SyncServiceManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let runtime_tasks = Arc::new(RuntimeTaskRegistry::with_diagnostics(diagnostics.clone()));
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: Some(sync_manager),
            rendezvous_manager: None,
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
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
        lifecycle_manager: LifecycleManager,
        rendezvous_manager: RendezvousManager,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let runtime_tasks = Arc::new(RuntimeTaskRegistry::with_diagnostics(diagnostics.clone()));
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: Some(rendezvous_manager),
            rendezvous_handler: None,
            lan_transport: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
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
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let runtime_tasks = Arc::new(RuntimeTaskRegistry::with_diagnostics(diagnostics.clone()));
        Self {
            effect_executor,
            effect_system,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            rendezvous_handler,
            lan_transport,
            social_manager,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
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

    /// Get runtime reconfiguration manager.
    pub fn reconfiguration(&self) -> &ReconfigurationManager {
        &self.reconfiguration_manager
    }

    /// Get the runtime task registry.
    pub fn tasks(&self) -> Arc<RuntimeTaskRegistry> {
        self.runtime_tasks.clone()
    }

    pub fn activity_gate(&self) -> Arc<RuntimeActivityGate> {
        self.activity_gate.clone()
    }

    pub fn runtime_activity_state(&self) -> RuntimeActivityState {
        self.activity_gate.state()
    }

    pub fn diagnostics(&self) -> Arc<RuntimeDiagnosticSink> {
        self.diagnostics.clone()
    }

    /// Start background maintenance tasks (cleanup, pruning).
    pub fn start_maintenance_tasks(&self) {
        let tasks = self.runtime_tasks.clone();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        let sync_manager = self.sync_manager.clone();
        let rendezvous_manager = self.rendezvous_manager.clone();
        let rendezvous_handler = self.rendezvous_handler.clone();
        let lan_transport = self.lan_transport.clone();
        let effects = self.effect_system.clone();
        let authority_id = self.authority_id;
        let device_id = self.device_id();

        let harness_mode = std::env::var_os("AURA_HARNESS_MODE").is_some();

        if !harness_mode {
            if let Ok(invitation_handler) = InvitationHandler::new(
                AuthorityContext::new_with_device(self.authority_id, self.device_id()),
            ) {
                let effects = self.effect_system.clone();
                let handler = invitation_handler.clone();
                let interval = Duration::from_secs(2);
                tasks.spawn_interval_until_named(
                    "maintenance.invitation_acceptance",
                    time_effects.clone(),
                    interval,
                    move || {
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
                    },
                );
            }

            if let Some(rendezvous_handler) = rendezvous_handler {
                let effects = self.effect_system.clone();
                let handler = rendezvous_handler.clone();
                let interval = Duration::from_secs(2);
                tasks.spawn_interval_until_named(
                    "maintenance.rendezvous_handshakes",
                    time_effects.clone(),
                    interval,
                    move || {
                        let effects = effects.clone();
                        let handler = handler.clone();
                        async move {
                            if let Err(e) =
                                handler.process_handshake_envelopes(effects.clone()).await
                            {
                                tracing::debug!(
                                    error = %e,
                                    "Failed to process rendezvous handshake envelopes"
                                );
                            }
                            true
                        }
                    },
                );
            }
        }

        if let (Some(sync_manager), Some(rendezvous_manager)) =
            (sync_manager.clone(), rendezvous_manager.clone())
        {
            let interval = sync_peer_reconcile_interval(&sync_manager);
            tasks.spawn_interval_until_named(
                "maintenance.sync_peer_reconcile",
                time_effects.clone(),
                interval,
                move || {
                    let sync_manager = sync_manager.clone();
                    let rendezvous_manager = rendezvous_manager.clone();
                    async move {
                        let desired_peers: std::collections::HashSet<DeviceId> = rendezvous_manager
                            .list_reachable_peer_devices()
                            .await
                            .into_iter()
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
                },
            );
        }

        if let (Some(rendezvous_manager), Some(lan_transport)) = (rendezvous_manager, lan_transport)
        {
            let interval = Duration::from_secs(60);
            tasks.spawn_interval_until_named(
                "maintenance.lan_descriptor_refresh",
                time_effects.clone(),
                interval,
                move || {
                    let rendezvous_manager = rendezvous_manager.clone();
                    let lan_transport = lan_transport.clone();
                    let effects = effects.clone();
                    async move {
                        let now_ms = match effects.time_effects().physical_time().await {
                            Ok(t) => t.ts_ms,
                            Err(_) => return true,
                        };
                        let context_id =
                            ContextId::new_from_entropy(hash(&authority_id.to_bytes()));
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
                },
            );
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
    /// the runtime publishes typed facts into it.
    pub async fn start_reactive_pipeline(&mut self) -> Result<(), String> {
        if self.reactive_pipeline.is_some() {
            return Ok(());
        }

        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(self.effect_system.time_effects().clone());
        let pipeline_tasks = self.runtime_tasks.group("reactive_pipeline");

        let pipeline = ReactivePipeline::start(
            pipeline_tasks,
            SchedulerConfig::default(),
            self.effect_system.fact_registry(),
            time_effects,
            self.effect_system.clone(),
            self.authority_id,
            self.effect_system.reactive_handler(),
            self.diagnostics.clone(),
        );

        // Attach the scheduler ingestion channel to the effect system so all journal commits
        // go through the single typed-fact pipeline.
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
            pipeline
                .publish_journal_facts(existing)
                .await
                .map_err(|error| error.to_string())?;
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

        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        let service_context = RuntimeServiceContext::new(self.runtime_tasks.clone(), time_effects);

        for service in self.runtime_services_in_start_order()? {
            self.start_runtime_service(service, &service_context)
                .await?;
            if service.name() == "rendezvous_manager" {
                if let Err(error) = self.publish_lan_descriptor().await {
                    tracing::warn!(
                        event = "runtime.service.lifecycle.post_start_failed",
                        service = service.name(),
                        error = %error,
                        "Service-specific post-start action failed"
                    );
                }
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        if self.lan_transport.is_some() {
            self.start_lan_transport_listener();
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

    #[cfg(not(target_arch = "wasm32"))]
    fn start_lan_transport_listener(&self) {
        let Some(lan_transport) = &self.lan_transport else {
            return;
        };

        let tcp_accept_group = self.runtime_tasks.group("lan_transport.tcp");
        let tcp_connection_group = tcp_accept_group.group("connections");
        let listener = lan_transport.listener();
        let websocket_listener = lan_transport.websocket_listener();
        let effects = self.effect_system.clone();
        let metrics = lan_transport.metrics_handle();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(effects.time_effects().clone());
        tcp_accept_group.spawn_cancellable_named("accept_loop", async move {
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
                let connection_group = tcp_connection_group.clone();
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
                connection_group.spawn_named(format!("connection.{addr}"), async move {
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

                    let envelope = match aura_core::util::serialization::from_slice(&payload) {
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

                    let _ = handle_inbound_transport_envelope(effects, envelope).await;
                });
            }
        });

        let websocket_accept_group = self.runtime_tasks.group("lan_transport.websocket");
        let websocket_connection_group = websocket_accept_group.group("connections");
        let effects = self.effect_system.clone();
        let metrics = lan_transport.metrics_handle();
        let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
            Arc::new(self.effect_system.time_effects().clone());
        websocket_accept_group.spawn_cancellable_named("accept_loop", async move {
            loop {
                let (stream, addr) = match websocket_listener.accept().await {
                    Ok((stream, addr)) => (stream, addr),
                    Err(err) => {
                        tracing::warn!(error = %err, "LAN websocket accept failed");
                        continue;
                    }
                };

                let effects = effects.clone();
                let metrics = metrics.clone();
                let time_effects = time_effects.clone();
                let connection_group = websocket_connection_group.clone();
                connection_group.spawn_named(format!("connection.{addr}"), async move {
                    let websocket = match accept_async(stream).await {
                        Ok(websocket) => websocket,
                        Err(err) => {
                            tracing::debug!(error = %err, addr = %addr, "LAN websocket handshake failed");
                            return;
                        }
                    };
                    let (mut sink, mut stream) = websocket.split();
                    while let Some(message) = stream.next().await {
                        let message = match message {
                            Ok(message) => message,
                            Err(err) => {
                                tracing::debug!(error = %err, addr = %addr, "LAN websocket read failed");
                                return;
                            }
                        };

                        if !message.is_binary() {
                            continue;
                        }

                        let payload = message.into_data();
                        let envelope = match aura_core::util::serialization::from_slice::<TransportEnvelope>(&payload) {
                            Ok(envelope) => envelope,
                            Err(err) => {
                                tracing::debug!(error = %err, addr = %addr, "LAN websocket decode failed");
                                let mut metrics = metrics.write().await;
                                metrics.decode_errors = metrics.decode_errors.saturating_add(1);
                                continue;
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
                            metrics.bytes_received =
                                metrics.bytes_received.saturating_add(payload.len() as u64);
                            if now_ms > 0 {
                                metrics.last_frame_ms = now_ms;
                            }
                        }

                        if let Some(response) =
                            handle_inbound_transport_envelope(effects.clone(), envelope).await
                        {
                            match aura_core::util::serialization::to_vec(&response) {
                                Ok(bytes) => {
                                    if let Err(err) =
                                        sink.send(tokio_tungstenite::tungstenite::Message::Binary(bytes)).await
                                    {
                                        tracing::debug!(
                                            error = %err,
                                            addr = %addr,
                                            "LAN websocket response send failed"
                                        );
                                        return;
                                    }
                                }
                                Err(err) => {
                                    tracing::debug!(
                                        error = %err,
                                        addr = %addr,
                                        "LAN websocket response encode failed"
                                    );
                                }
                            }
                        }
                    }
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

        for service in self.runtime_services_in_stop_order()? {
            self.stop_runtime_service(service).await?;
        }

        Ok(())
    }

    fn runtime_services(&self) -> Vec<&dyn RuntimeService> {
        let mut services: Vec<&dyn RuntimeService> = vec![
            &self.flow_budget_manager,
            &self.receipt_manager,
            &self.ceremony_tracker,
            &self.threshold_signing,
        ];
        if let Some(social_manager) = &self.social_manager {
            services.push(social_manager);
        }
        if let Some(rendezvous_manager) = &self.rendezvous_manager {
            services.push(rendezvous_manager);
        }
        if let Some(sync_manager) = &self.sync_manager {
            services.push(sync_manager);
        }
        services
    }

    fn runtime_services_in_start_order(&self) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
        sort_runtime_services_by_dependencies(self.runtime_services())
    }

    fn runtime_services_in_stop_order(&self) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
        let mut services = self.runtime_services_in_start_order()?;
        services.reverse();
        Ok(services)
    }

    async fn start_runtime_service(
        &self,
        service: &dyn RuntimeService,
        context: &RuntimeServiceContext,
    ) -> Result<(), ServiceError> {
        tracing::info!(
            event = "runtime.service.lifecycle.transition",
            service = service.name(),
            phase = "start_requested",
            "Starting runtime service"
        );
        service.start(context).await?;
        let health = service.health().await;
        match health {
            ServiceHealth::Healthy | ServiceHealth::Degraded { .. } => {
                tracing::info!(
                    event = "runtime.service.lifecycle.transition",
                    service = service.name(),
                    phase = "running",
                    health = %health,
                    "Runtime service started"
                );
                Ok(())
            }
            other => Err(ServiceError::startup_failed(
                service.name(),
                format!("service entered non-operational state after start: {other}"),
            )),
        }
    }

    async fn stop_runtime_service(&self, service: &dyn RuntimeService) -> Result<(), ServiceError> {
        const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(5);

        tracing::info!(
            event = "runtime.service.lifecycle.transition",
            service = service.name(),
            phase = "stop_requested",
            "Stopping runtime service"
        );
        tokio::time::timeout(SERVICE_STOP_TIMEOUT, service.stop())
            .await
            .map_err(|_| {
                tracing::warn!(
                    event = "runtime.shutdown.service_timeout",
                    service = service.name(),
                    timeout_ms = SERVICE_STOP_TIMEOUT.as_millis() as u64,
                    "Runtime service stop timed out"
                );
                ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Timeout,
                    format!(
                        "service stop timed out after {}ms",
                        SERVICE_STOP_TIMEOUT.as_millis()
                    ),
                )
            })??;
        let health = service.health().await;
        match health {
            ServiceHealth::Stopped | ServiceHealth::NotStarted => {
                tracing::info!(
                    event = "runtime.service.lifecycle.transition",
                    service = service.name(),
                    phase = "stopped",
                    health = %health,
                    "Runtime service stopped"
                );
                Ok(())
            }
            other => Err(ServiceError::shutdown_failed(
                service.name(),
                format!("service remained active after stop: {other}"),
            )),
        }
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

    pub async fn shutdown_typed(mut self, ctx: &EffectContext) -> Result<(), RuntimeShutdownError> {
        let prior_state = self.activity_gate.begin_shutdown();
        if prior_state != RuntimeActivityState::Running {
            tracing::info!(
                event = "runtime.shutdown.already_in_progress",
                previous_state = ?prior_state,
                "Runtime shutdown requested after shutdown had already started"
            );
            self.activity_gate.mark_stopped();
            return Ok(());
        }

        let runtime_tasks = self.runtime_tasks.clone();
        let mut shutdown_error: Option<RuntimeShutdownError> = None;

        // Drain the reactive scheduler before cancelling the broader runtime task tree.
        tracing::info!(
            event = "runtime.shutdown.stage",
            stage = "reactive_pipeline",
            "Starting runtime shutdown"
        );
        let reactive_pipeline = self.reactive_pipeline.take();
        if let Some(pipeline) = reactive_pipeline {
            if let Err(error) = pipeline.shutdown().await {
                tracing::warn!(
                    event = "runtime.shutdown.reactive_pipeline_signal_failed",
                    error = %error,
                    "Reactive pipeline shutdown signal was unavailable during runtime shutdown"
                );
            }
        }

        tracing::info!(
            event = "runtime.shutdown.stage",
            stage = "task_tree",
            "Cancelling runtime task tree"
        );
        if let Err(error) = runtime_tasks
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
        {
            tracing::warn!(
                event = "runtime.shutdown.task_tree_escalated",
                error = %error,
                "Runtime task tree required forced shutdown"
            );
            shutdown_error.get_or_insert(RuntimeShutdownError::TaskTree(error));
        }

        // Stop services after background runtime work has been cancelled.
        tracing::info!(
            event = "runtime.shutdown.stage",
            stage = "services",
            "Stopping runtime services"
        );
        if let Err(e) = self.stop_services().await {
            tracing::warn!(
                event = "runtime.shutdown.services_failed",
                error = %e,
                "Failed to stop runtime services during shutdown"
            );
            shutdown_error.get_or_insert(RuntimeShutdownError::Service(e));
        }

        let RuntimeSystem {
            lifecycle_manager,
            sync_manager: _sync_manager,
            rendezvous_manager: _rendezvous_manager,
            ..
        } = self;

        tracing::info!(
            event = "runtime.shutdown.stage",
            stage = "lifecycle_manager",
            "Shutting down lifecycle manager"
        );
        if let Err(error) = lifecycle_manager.shutdown(ctx).await {
            tracing::warn!(
                event = "runtime.shutdown.lifecycle_failed",
                error = %error,
                "Lifecycle manager shutdown failed"
            );
            shutdown_error.get_or_insert(RuntimeShutdownError::Lifecycle(error));
        }

        self.activity_gate.mark_stopped();

        match shutdown_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn sort_runtime_services_by_dependencies(
    services: Vec<&dyn RuntimeService>,
) -> Result<Vec<&dyn RuntimeService>, ServiceError> {
    let mut service_by_name = BTreeMap::new();
    for service in &services {
        service_by_name.insert(service.name(), *service);
    }

    let mut indegree = BTreeMap::<&'static str, usize>::new();
    let mut dependents = BTreeMap::<&'static str, Vec<&'static str>>::new();
    for service in &services {
        indegree.entry(service.name()).or_insert(0);
        for dependency in service.dependencies() {
            if !service_by_name.contains_key(dependency) {
                continue;
            }
            *indegree.entry(service.name()).or_insert(0) += 1;
            dependents
                .entry(*dependency)
                .or_default()
                .push(service.name());
        }
    }

    let mut ready = VecDeque::new();
    for service in &services {
        if indegree.get(service.name()).copied().unwrap_or_default() == 0 {
            ready.push_back(service.name());
        }
    }

    let mut ordered = Vec::with_capacity(services.len());
    while let Some(name) = ready.pop_front() {
        let Some(service) = service_by_name.get(name).copied() else {
            continue;
        };
        ordered.push(service);
        if let Some(children) = dependents.get(name) {
            for child in children {
                if let Some(entry) = indegree.get_mut(child) {
                    *entry = entry.saturating_sub(1);
                    if *entry == 0 {
                        ready.push_back(child);
                    }
                }
            }
        }
    }

    if ordered.len() != services.len() {
        let blocked = indegree
            .into_iter()
            .filter_map(|(name, count)| (count > 0).then_some(name))
            .collect::<Vec<_>>();
        return Err(ServiceError::new(
            "runtime_services",
            ServiceErrorKind::DependencyUnavailable,
            format!(
                "runtime service dependency graph contains a cycle or unsatisfied internal dependencies: {}",
                blocked.join(", ")
            ),
        ));
    }

    Ok(ordered)
}

#[cfg(not(target_arch = "wasm32"))]
async fn handle_inbound_transport_envelope(
    effects: Arc<AuraEffectSystem>,
    envelope: TransportEnvelope,
) -> Option<TransportEnvelope> {
    if envelope
        .metadata
        .get("content-type")
        .is_some_and(|content_type| content_type == FACT_SYNC_REQUEST_CONTENT_TYPE)
    {
        let local_authority = GuardContextProvider::authority_id(effects.as_ref());
        let facts = match effects.load_committed_facts(local_authority).await {
            Ok(facts) => facts
                .into_iter()
                .filter_map(|fact| match fact.content {
                    FactContent::Relational(rel) => Some(rel),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            Err(err) => {
                tracing::debug!(
                    error = %err,
                    "Failed to load committed facts for fact sync response"
                );
                Vec::new()
            }
        };

        let payload = match aura_core::util::serialization::to_vec(&facts) {
            Ok(payload) => payload,
            Err(err) => {
                tracing::debug!(
                    error = %err,
                    "Failed to encode fact sync response payload"
                );
                return None;
            }
        };

        let mut metadata = std::collections::HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            FACT_SYNC_RESPONSE_CONTENT_TYPE.to_string(),
        );

        return Some(TransportEnvelope {
            destination: envelope.source,
            source: envelope.destination,
            context: envelope.context,
            payload,
            metadata,
            receipt: None,
        });
    }

    if envelope
        .metadata
        .get("content-type")
        .is_some_and(|content_type| content_type == CHAT_FACT_CONTENT_TYPE)
    {
        eprintln!(
            "[recv-chat-fact] source={};destination={};context={}",
            envelope.source, envelope.destination, envelope.context
        );
        match from_slice::<RelationalFact>(&envelope.payload) {
            Ok(fact) => {
                if let RelationalFact::Generic {
                    envelope: chat_envelope,
                    ..
                } = &fact
                {
                    if chat_envelope.type_id.as_str() == CHAT_FACT_TYPE_ID {
                        if let Some(ChatFact::ChannelCreated {
                            context_id,
                            channel_id,
                            creator_id,
                            ..
                        }) = ChatFact::from_envelope(chat_envelope)
                        {
                            let local_authority = envelope.destination;
                            if get_channel_state(effects.as_ref(), context_id, channel_id)
                                .await
                                .is_err()
                            {
                                if let Err(err) = effects
                                    .create_channel(ChannelCreateParams {
                                        context: context_id,
                                        channel: Some(channel_id),
                                        skip_window: None,
                                        topic: None,
                                    })
                                    .await
                                {
                                    let lowered = err.to_string().to_ascii_lowercase();
                                    if !lowered.contains("already") && !lowered.contains("exists") {
                                        tracing::warn!(
                                            context_id = %context_id,
                                            channel_id = %channel_id,
                                            error = %err,
                                            "Failed to provision AMP channel checkpoint from inbound chat fact"
                                        );
                                    }
                                }

                                let mut participants = vec![local_authority];
                                if creator_id != local_authority {
                                    participants.push(creator_id);
                                }

                                for participant in participants {
                                    if let Err(err) = effects
                                        .join_channel(ChannelJoinParams {
                                            context: context_id,
                                            channel: channel_id,
                                            participant,
                                        })
                                        .await
                                    {
                                        tracing::debug!(
                                            context_id = %context_id,
                                            channel_id = %channel_id,
                                            participant = %participant,
                                            error = %err,
                                            "AMP join provisioning from inbound chat fact failed"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                if let Err(err) = effects.commit_relational_facts(vec![fact]).await {
                    tracing::debug!(
                        error = %err,
                        "LAN transport failed to commit incoming chat fact envelope"
                    );
                } else {
                    effects.await_next_view_update().await;
                }
                return None;
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "LAN transport received invalid chat fact envelope payload"
                );
                return None;
            }
        }
    }

    effects.requeue_envelope(envelope);
    None
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
    tracing::info!(
        authority = %authority_id,
        tcp_addrs = ?lan_transport.advertised_addrs(),
        websocket_addrs = ?lan_transport.websocket_addrs(),
        "publish_lan_descriptor_with transport addresses"
    );
    for addr in lan_transport.advertised_addrs() {
        match TransportHint::tcp_direct(addr) {
            Ok(hint) => hints.push(hint),
            Err(err) => {
                tracing::warn!(addr = %addr, error = %err, "Skipping invalid LAN transport hint");
            }
        }
    }
    for addr in lan_transport.websocket_addrs() {
        match TransportHint::websocket_direct(addr) {
            Ok(hint) => hints.push(hint),
            Err(err) => {
                tracing::warn!(
                    addr = %addr,
                    error = %err,
                    "Skipping invalid LAN websocket transport hint"
                );
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
            let descriptor = RendezvousDescriptor {
                device_id: Some(device_id),
                ..descriptor
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::services::SyncManagerConfig;

    #[test]
    fn runtime_activity_gate_transitions_and_rejects_new_public_work() {
        let gate = RuntimeActivityGate::new();
        assert_eq!(gate.state(), RuntimeActivityState::Running);
        assert!(gate.ensure_accepting_public_operations().is_ok());

        assert_eq!(gate.begin_shutdown(), RuntimeActivityState::Running);
        assert_eq!(gate.state(), RuntimeActivityState::Stopping);
        assert!(matches!(
            gate.ensure_accepting_public_operations(),
            Err(RuntimePublicOperationError::NotAccepting {
                state: RuntimeActivityState::Stopping
            })
        ));

        assert_eq!(gate.begin_shutdown(), RuntimeActivityState::Stopping);
        gate.mark_stopped();
        assert_eq!(gate.state(), RuntimeActivityState::Stopped);
    }

    #[test]
    fn sync_peer_reconcile_interval_follows_fast_sync_config() {
        let manager = SyncServiceManager::new(SyncManagerConfig {
            auto_sync_interval: Duration::from_secs(2),
            ..SyncManagerConfig::default()
        });

        assert_eq!(
            sync_peer_reconcile_interval(&manager),
            Duration::from_secs(2)
        );
    }

    #[test]
    fn sync_peer_reconcile_interval_clamps_large_values() {
        let manager = SyncServiceManager::new(SyncManagerConfig {
            auto_sync_interval: Duration::from_secs(120),
            ..SyncManagerConfig::default()
        });

        assert_eq!(
            sync_peer_reconcile_interval(&manager),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn sync_peer_reconcile_interval_clamps_small_values() {
        let manager = SyncServiceManager::new(SyncManagerConfig {
            auto_sync_interval: Duration::from_millis(100),
            ..SyncManagerConfig::default()
        });

        assert_eq!(
            sync_peer_reconcile_interval(&manager),
            Duration::from_secs(1)
        );
    }
}

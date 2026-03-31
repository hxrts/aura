//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::ceremony_runner::CeremonyRunner;
use super::services::{
    AuthorityManager, AuthorityStatus, CeremonyTracker, ContextManager, FlowBudgetManager,
    HoldManager, LanTransportListenerService, LanTransportService, MoveManager,
    ReactivePipelineService, ReceiptManager, ReconfigurationManager, RendezvousManager,
    RuntimeMaintenanceService, RuntimeService, RuntimeServiceContext, ServiceError,
    ServiceErrorKind, ServiceHealth, SocialManager, SyncServiceManager, ThresholdSigningService,
};
use super::{
    AuraEffectSystem, EffectContext, EffectExecutor, LifecycleManager, RuntimeDiagnosticSink,
    RuntimeShutdownEvent, TaskSupervisor,
};
use crate::core::{AgentConfig, AuthorityContext};
use crate::handlers::RendezvousHandler;
#[cfg(not(target_arch = "wasm32"))]
use crate::task_registry::TaskGroup;
use crate::task_registry::TaskSupervisionError;
#[cfg(not(target_arch = "wasm32"))]
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_core::effects::time::PhysicalTimeEffects;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::transport::TransportEnvelope;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::{AmpChannelEffects, ChannelCreateParams, ChannelJoinParams};
use aura_core::types::identifiers::AuthorityId;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::util::serialization::from_slice;
use aura_core::DeviceId;
use aura_core::{
    execute_with_timeout_budget, OwnedShutdownToken, OwnedTaskSpawner, TimeoutBudget,
    TimeoutRunError,
};
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
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
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

pub(crate) fn sync_peer_reconcile_interval(sync_manager: &SyncServiceManager) -> Duration {
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
    Lifecycle(crate::AgentError),
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

    pub fn ensure_accepting_public_operations(&self) -> Result<(), RuntimePublicOperationError> {
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

    /// Move manager for bounded movement planning and delivery.
    move_manager: Option<MoveManager>,

    /// Hold manager for shared custody and selector-based retrieval.
    hold_manager: Option<HoldManager>,

    /// Social manager (optional, for social topology and relay selection)
    social_manager: Option<SocialManager>,

    /// Ceremony tracker (for guardian ceremony coordination)
    ceremony_tracker: CeremonyTracker,

    /// Ceremony runner (shared Category C orchestration API)
    ceremony_runner: CeremonyRunner,

    /// Threshold signing service (shared state across runtime operations)
    threshold_signing: ThresholdSigningService,

    /// Service-owned reactive pipeline.
    reactive_pipeline_service: ReactivePipelineService,

    /// Service-owned LAN transport listeners.
    lan_listener_service: Option<LanTransportListenerService>,

    /// Service-owned runtime maintenance loops.
    maintenance_service: RuntimeMaintenanceService,

    /// Reconfiguration manager for link/delegate operations.
    reconfiguration_manager: ReconfigurationManager,

    /// Runtime task registry for background work
    runtime_tasks: Arc<TaskSupervisor>,

    /// Shared runtime activity gate used to reject new public work during shutdown.
    activity_gate: Arc<RuntimeActivityGate>,

    /// Shared diagnostics sink for surfaced async/runtime failures.
    diagnostics: Arc<RuntimeDiagnosticSink>,

    /// Configuration
    #[allow(dead_code)] // Will be used for runtime configuration
    config: AgentConfig,

    /// Authority ID
    authority_id: AuthorityId,
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
        let device_id = config.device_id;
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            None,
            None,
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
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
            move_manager: None,
            hold_manager: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
            config,
            authority_id,
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
        let device_id = config.device_id;
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            Some(sync_manager.clone()),
            None,
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
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
            move_manager: None,
            hold_manager: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
            config,
            authority_id,
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
        let device_id = config.device_id;
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            None,
            Some(rendezvous_manager.clone()),
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
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
            move_manager: None,
            hold_manager: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
            config,
            authority_id,
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
        move_manager: Option<MoveManager>,
        hold_manager: Option<HoldManager>,
        rendezvous_handler: Option<RendezvousHandler>,
        lan_transport: Option<Arc<LanTransportService>>,
        social_manager: Option<SocialManager>,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let device_id = config.device_id;
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let time_effects: Arc<dyn PhysicalTimeEffects> =
            Arc::new(effect_system.time_effects().clone());
        let ceremony_tracker = CeremonyTracker::new(time_effects);
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let lan_listener_service = lan_transport.clone().map(|lan_transport| {
            LanTransportListenerService::new(effect_system.clone(), lan_transport)
        });
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            sync_manager.clone(),
            rendezvous_manager.clone(),
            rendezvous_handler.clone(),
            lan_transport.clone(),
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
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
            move_manager,
            hold_manager,
            social_manager,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service,
            maintenance_service,
            reconfiguration_manager: ReconfigurationManager::new(),
            runtime_tasks,
            activity_gate: Arc::new(RuntimeActivityGate::new()),
            diagnostics,
            config,
            authority_id,
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
    pub fn tasks(&self) -> Arc<TaskSupervisor> {
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

    /// Get the runtime task spawner through the sanctioned owned wrapper.
    pub fn task_spawner(&self) -> OwnedTaskSpawner {
        OwnedTaskSpawner::new(
            self.runtime_tasks.clone(),
            OwnedShutdownToken::attached(self.runtime_tasks.cancellation_token()),
        )
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

    /// Check whether the service-owned reactive pipeline is running.
    pub async fn reactive_pipeline_running(&self) -> bool {
        self.reactive_pipeline_service.is_running().await
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
        }

        if let Err(error) = self
            .maintenance_service
            .publish_initial_lan_descriptor()
            .await
        {
            tracing::warn!(
                event = "runtime.service.lifecycle.reconcile_failed",
                service = self.maintenance_service.name(),
                error = %error,
                "Runtime startup reconciliation failed to republish the initial LAN descriptor"
            );
        }

        Ok(())
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
            &self.reactive_pipeline_service,
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
        if let Some(move_manager) = &self.move_manager {
            services.push(move_manager);
        }
        if let Some(hold_manager) = &self.hold_manager {
            services.push(hold_manager);
        }
        if let Some(sync_manager) = &self.sync_manager {
            services.push(sync_manager);
        }
        if let Some(lan_listener_service) = &self.lan_listener_service {
            services.push(lan_listener_service);
        }
        services.push(&self.maintenance_service);
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
        let started_at = self.effect_system.physical_time().await.map_err(|error| {
            ServiceError::new(
                service.name(),
                ServiceErrorKind::Internal,
                format!("could not read physical time for service stop budget: {error}"),
            )
        })?;
        let budget = TimeoutBudget::from_start_and_timeout(&started_at, SERVICE_STOP_TIMEOUT)
            .map_err(|error| {
                ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Internal,
                    format!("invalid service stop timeout budget: {error}"),
                )
            })?;

        execute_with_timeout_budget(self.effect_system.as_ref(), &budget, || service.stop())
            .await
            .map_err(|error| match error {
                TimeoutRunError::Timeout(_) => {
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
                }
                TimeoutRunError::Operation(error) => ServiceError::new(
                    service.name(),
                    ServiceErrorKind::Internal,
                    error.to_string(),
                ),
            })?;
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

    /// Get the hold service manager (if enabled).
    pub fn hold(&self) -> Option<&HoldManager> {
        self.hold_manager.as_ref()
    }

    /// Check if hold service is enabled.
    pub fn has_hold(&self) -> bool {
        self.hold_manager.is_some()
    }

    /// Check if social service is enabled
    pub fn has_social(&self) -> bool {
        self.social_manager.is_some()
    }

    pub async fn shutdown_typed(self, ctx: &EffectContext) -> Result<(), RuntimeShutdownError> {
        let prior_state = self.activity_gate.begin_shutdown();
        if prior_state != RuntimeActivityState::Running {
            tracing::info!(
                event = RuntimeShutdownEvent::AlreadyInProgress.as_event_name(),
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
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "reactive_pipeline",
            "Starting runtime shutdown"
        );
        if let Err(error) = self.reactive_pipeline_service.stop().await {
            tracing::warn!(
                event = RuntimeShutdownEvent::ReactivePipelineSignalFailed.as_event_name(),
                error = %error,
                "Reactive pipeline shutdown failed during runtime shutdown"
            );
        }

        tracing::info!(
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "task_tree",
            "Cancelling runtime task tree"
        );
        if let Err(error) = runtime_tasks
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
        {
            tracing::warn!(
                event = RuntimeShutdownEvent::TaskTreeEscalated.as_event_name(),
                error = %error,
                "Runtime task tree required forced shutdown"
            );
            shutdown_error.get_or_insert(RuntimeShutdownError::TaskTree(error));
        }

        // Stop services after background runtime work has been cancelled.
        tracing::info!(
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "services",
            "Stopping runtime services"
        );
        if let Err(e) = self.stop_services().await {
            tracing::warn!(
                event = RuntimeShutdownEvent::ServicesFailed.as_event_name(),
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
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "lifecycle_manager",
            "Shutting down lifecycle manager"
        );
        if let Err(error) = lifecycle_manager.shutdown(ctx).await {
            tracing::warn!(
                event = RuntimeShutdownEvent::LifecycleFailed.as_event_name(),
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
pub(crate) fn spawn_lan_transport_listener_tasks(
    parent_tasks: TaskGroup,
    effects: Arc<AuraEffectSystem>,
    lan_transport: Arc<LanTransportService>,
) {
    let listener = lan_transport.listener();
    let websocket_listener = lan_transport.websocket_listener();
    let metrics = lan_transport.metrics_handle();
    let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
        Arc::new(effects.time_effects().clone());
    let websocket_effects = effects.clone();
    let tcp_accept_group = parent_tasks.clone();
    let tcp_connection_group = tcp_accept_group.clone();
    let _tcp_accept_task_handle =
        tcp_accept_group.spawn_cancellable_named("tcp_accept_loop", async move {
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
                let _connection_task =
                connection_group.spawn_named(format!("tcp_connection.{addr}"), async move {
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
                    tracing::debug!(
                        error = %err,
                        addr = %addr,
                        "LAN transport read payload failed"
                    );
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

    let metrics = lan_transport.metrics_handle();
    let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
        Arc::new(websocket_effects.time_effects().clone());
    let websocket_accept_group = parent_tasks.clone();
    let websocket_connection_group = websocket_accept_group.clone();
    let _websocket_accept_task_handle =
        websocket_accept_group.spawn_cancellable_named("websocket_accept_loop", async move {
            loop {
                let (stream, addr) = match websocket_listener.accept().await {
                    Ok((stream, addr)) => (stream, addr),
                    Err(err) => {
                        tracing::warn!(error = %err, "LAN websocket accept failed");
                        continue;
                    }
                };

                let effects = websocket_effects.clone();
                let metrics = metrics.clone();
                let time_effects = time_effects.clone();
                let connection_group = websocket_connection_group.clone();
                let _connection_task = connection_group.spawn_named(
                    format!("websocket_connection.{addr}"),
                    async move {
                        let websocket = match accept_async(stream).await {
                            Ok(websocket) => websocket,
                            Err(err) => {
                                tracing::debug!(
                                    error = %err,
                                    addr = %addr,
                                    "LAN websocket handshake failed"
                                );
                                return;
                            }
                        };
                        let (mut sink, mut stream) = websocket.split();
                        while let Some(message) = stream.next().await {
                            let message = match message {
                                Ok(message) => message,
                                Err(err) => {
                                    tracing::debug!(
                                        error = %err,
                                        addr = %addr,
                                        "LAN websocket read failed"
                                    );
                                    return;
                                }
                            };

                            if !message.is_binary() {
                                continue;
                            }

                            let payload = message.into_data();
                            let envelope = match aura_core::util::serialization::from_slice::<
                                TransportEnvelope,
                            >(&payload)
                            {
                                Ok(envelope) => envelope,
                                Err(err) => {
                                    tracing::debug!(
                                        error = %err,
                                        addr = %addr,
                                        "LAN websocket decode failed"
                                    );
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
                                        if let Err(err) = sink
                                            .send(tokio_tungstenite::tungstenite::Message::Binary(
                                                bytes,
                                            ))
                                            .await
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
                    },
                );
            }
        });
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
        tracing::debug!(
            source = %envelope.source,
            destination = %envelope.destination,
            context = %envelope.context,
            "recv-chat-fact"
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
                                    if get_channel_state(effects.as_ref(), context_id, channel_id)
                                        .await
                                        .is_err()
                                    {
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

pub(crate) async fn publish_lan_descriptor_with(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    device_id: DeviceId,
    rendezvous_manager: &RendezvousManager,
    lan_transport: &LanTransportService,
) -> Result<(), ServiceError> {
    async fn install_lan_descriptor(
        rendezvous_manager: &RendezvousManager,
        descriptor: RendezvousDescriptor,
    ) -> Result<(), ServiceError> {
        rendezvous_manager
            .cache_descriptor(descriptor.clone())
            .await
            .map_err(|error| ServiceError::startup_failed("rendezvous_cache", error.to_string()))?;
        rendezvous_manager.set_lan_descriptor(descriptor).await;
        Ok(())
    }

    let authority_context = AuthorityContext::new_with_device(authority_id, device_id);
    let handler = RendezvousHandler::new(authority_context.clone())
        .map_err(|e| ServiceError::startup_failed("rendezvous_handler", e.to_string()))?;
    let context_id = authority_context.default_context_id();

    let mut hints = Vec::new();
    let tcp_addrs = lan_transport.advertised_addrs();
    let websocket_addrs = lan_transport.websocket_addrs();
    tracing::info!(
        authority = %authority_id,
        tcp_addrs = ?tcp_addrs,
        websocket_addrs = ?websocket_addrs,
        "publish_lan_descriptor_with transport addresses"
    );
    let mut invalid_tcp_hints = 0usize;
    for addr in tcp_addrs {
        match TransportHint::tcp_direct(addr) {
            Ok(hint) => hints.push(hint),
            Err(err) => {
                invalid_tcp_hints += 1;
                tracing::warn!(addr = %addr, error = %err, "Skipping invalid LAN transport hint");
            }
        }
    }
    let mut invalid_websocket_hints = 0usize;
    for addr in websocket_addrs {
        match TransportHint::websocket_direct(addr) {
            Ok(hint) => hints.push(hint),
            Err(err) => {
                invalid_websocket_hints += 1;
                tracing::warn!(
                    addr = %addr,
                    error = %err,
                    "Skipping invalid LAN websocket transport hint"
                );
            }
        }
    }

    if hints.is_empty() {
        tracing::warn!(
            authority = %authority_id,
            tcp_addrs = ?tcp_addrs,
            websocket_addrs = ?websocket_addrs,
            invalid_tcp_hints,
            invalid_websocket_hints,
            "LAN listeners are bound, but no rendezvous descriptor was published because every advertised address was rejected as an invalid direct transport hint; direct LAN discovery will be unavailable until at least one valid address is advertisable"
        );
        return Ok(());
    }

    let result = handler
        .publish_descriptor(&effects, context_id, hints.clone(), [0u8; 32], 0)
        .await
        .map_err(|error| ServiceError::startup_failed("rendezvous_publish", error.to_string()))?;
    let descriptor = require_published_lan_descriptor(result, device_id)?;
    install_lan_descriptor(rendezvous_manager, descriptor).await?;

    Ok(())
}

fn require_published_lan_descriptor(
    result: crate::handlers::rendezvous::RendezvousResult,
    device_id: DeviceId,
) -> Result<RendezvousDescriptor, ServiceError> {
    if !result.success {
        return Err(ServiceError::startup_failed(
            "rendezvous_publish",
            result
                .error
                .unwrap_or_else(|| "LAN descriptor publish failed".to_string()),
        ));
    }

    let descriptor = result.descriptor.ok_or_else(|| {
        ServiceError::startup_failed(
            "rendezvous_publish",
            "LAN descriptor publish succeeded without descriptor payload".to_string(),
        )
    })?;

    Ok(RendezvousDescriptor {
        device_id: Some(device_id),
        ..descriptor
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::builder::EffectSystemBuilder;
    use crate::runtime::services::SyncManagerConfig;
    use aura_core::ContextId;

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

    #[test]
    fn runtime_services_include_runtime_maintenance() {
        let authority_id = AuthorityId::new_from_entropy([11u8; 32]);
        let runtime = EffectSystemBuilder::testing()
            .with_authority(authority_id)
            .build_sync()
            .expect("build_sync should succeed in testing mode");

        let service_names = runtime
            .runtime_services_in_start_order()
            .expect("runtime services should sort cleanly")
            .into_iter()
            .map(RuntimeService::name)
            .collect::<Vec<_>>();

        assert!(service_names.contains(&"runtime_maintenance"));
    }

    #[test]
    fn lan_descriptor_publish_requires_descriptor_payload() {
        let context_id = ContextId::new_from_entropy([12u8; 32]);
        let device_id = DeviceId::new_from_entropy([14u8; 32]);
        let result = crate::handlers::rendezvous::RendezvousResult {
            success: true,
            context_id,
            peer: None,
            descriptor: None,
            error: None,
        };

        let error = require_published_lan_descriptor(result, device_id)
            .expect_err("missing descriptor payload must fail closed");

        assert!(error.to_string().contains("descriptor payload"));
        assert!(error.to_string().contains("rendezvous_publish"));
    }

    #[test]
    fn lan_descriptor_publish_requires_success_result() {
        let context_id = ContextId::new_from_entropy([15u8; 32]);
        let device_id = DeviceId::new_from_entropy([16u8; 32]);
        let result = crate::handlers::rendezvous::RendezvousResult {
            success: false,
            context_id,
            peer: None,
            descriptor: None,
            error: Some("guard denied".to_string()),
        };

        let error = require_published_lan_descriptor(result, device_id)
            .expect_err("failed publication must stay terminal");

        assert!(error.to_string().contains("guard denied"));
        assert!(error.to_string().contains("rendezvous_publish"));
    }

    #[test]
    fn lan_descriptor_publish_preserves_device_binding() {
        let authority_id = AuthorityId::new_from_entropy([17u8; 32]);
        let context_id = ContextId::new_from_entropy([18u8; 32]);
        let device_id = DeviceId::new_from_entropy([19u8; 32]);
        let result = crate::handlers::rendezvous::RendezvousResult {
            success: true,
            context_id,
            peer: None,
            descriptor: Some(RendezvousDescriptor {
                authority_id,
                device_id: None,
                context_id,
                transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:7000").unwrap()],
                handshake_psk_commitment: [0u8; 32],
                public_key: [0u8; 32],
                valid_from: 1,
                valid_until: 2,
                nonce: [0u8; 32],
                nickname_suggestion: None,
            }),
            error: None,
        };

        let descriptor = require_published_lan_descriptor(result, device_id)
            .expect("successful publish with payload should keep device binding");

        assert_eq!(descriptor.device_id, Some(device_id));
        assert_eq!(descriptor.context_id, context_id);
        assert_eq!(descriptor.authority_id, authority_id);
    }
}

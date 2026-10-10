//! Runtime System
//!
//! Main runtime system that orchestrates all agent operations.

use super::services::ceremony_runner::CeremonyRunner;
#[cfg(not(target_arch = "wasm32"))]
use super::services::lan_transport::LanTransportMetrics;
use super::services::rendezvous_manager::RendezvousManagerError;
use super::services::AnonymousPathManager;
use super::services::{
    AuthorityManager, AuthorityStatus, CeremonyTracker, ContextManager, CoverTrafficGenerator,
    FlowBudgetManager, HoldManager, LanTransportListenerService, LanTransportService,
    LocalHealthObserver, MoveManager, ReactivePipelineService, ReceiptManager,
    ReconfigurationManager, RendezvousManager, RuntimeMaintenanceService, RuntimeService,
    RuntimeServiceContext, SelectionManager, ServiceError, ServiceErrorKind, ServiceHealth,
    SocialManager, SyncServiceManager, ThresholdSigningService,
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
use aura_core::effects::time::PhysicalTimeEffects;
#[cfg(not(target_arch = "wasm32"))]
use aura_core::effects::transport::{TransportEnvelope, MAX_TRANSPORT_SIGNATURE_BYTES};
use aura_core::types::identifiers::AuthorityId;
use aura_core::DeviceId;
use aura_core::{
    execute_with_timeout_budget, OwnedShutdownToken, OwnedTaskSpawner, TimeoutBudget,
    TimeoutRunError,
};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
#[cfg(not(target_arch = "wasm32"))]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(not(target_arch = "wasm32"))]
use futures::{SinkExt, StreamExt};
#[cfg(not(target_arch = "wasm32"))]
use serde::Deserialize;
#[cfg(not(target_arch = "wasm32"))]
use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::net::IpAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use tokio::io::AsyncReadExt;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::accept_async_with_config;
#[cfg(not(target_arch = "wasm32"))]
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

const MIN_SYNC_PEER_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);
const MAX_SYNC_PEER_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
#[cfg(not(target_arch = "wasm32"))]
const MAX_LAN_FRAME_BYTES: usize = 128 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_LAN_CONCURRENT_CONNECTIONS: usize = 64;
#[cfg(not(target_arch = "wasm32"))]
const MAX_LAN_FRAMES_PER_PEER_WINDOW: u32 = 32;
#[cfg(not(target_arch = "wasm32"))]
const LAN_PEER_RATE_WINDOW_MS: u64 = 1_000;

mod lifecycle;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy)]
struct PeerIngressWindow {
    window_started_ms: u64,
    frames_seen: u32,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Deserialize)]
struct HarnessTransportEnvelopeMessage {
    kind: String,
    envelope_b64: String,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct LanIngressController {
    active_connections: Arc<Semaphore>,
    peer_windows: Arc<AsyncMutex<HashMap<IpAddr, PeerIngressWindow>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl LanIngressController {
    fn new() -> Self {
        Self {
            active_connections: Arc::new(Semaphore::new(MAX_LAN_CONCURRENT_CONNECTIONS)),
            peer_windows: Arc::new(AsyncMutex::new(HashMap::new())),
        }
    }

    fn try_acquire_connection(&self) -> Option<OwnedSemaphorePermit> {
        self.active_connections.clone().try_acquire_owned().ok()
    }

    async fn allow_frame(&self, peer: IpAddr, now_ms: u64) -> bool {
        let mut windows = self.peer_windows.lock().await;
        let entry = windows.entry(peer).or_insert(PeerIngressWindow {
            window_started_ms: now_ms,
            frames_seen: 0,
        });
        if now_ms.saturating_sub(entry.window_started_ms) >= LAN_PEER_RATE_WINDOW_MS {
            entry.window_started_ms = now_ms;
            entry.frames_seen = 0;
        }
        if entry.frames_seen >= MAX_LAN_FRAMES_PER_PEER_WINDOW {
            return false;
        }
        entry.frames_seen = entry.frames_seen.saturating_add(1);
        true
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn lan_websocket_config() -> WebSocketConfig {
    WebSocketConfig {
        max_message_size: Some(MAX_LAN_FRAME_BYTES),
        max_frame_size: Some(MAX_LAN_FRAME_BYTES),
        ..WebSocketConfig::default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn decode_lan_websocket_message(
    message: tokio_tungstenite::tungstenite::Message,
) -> Result<Vec<u8>, String> {
    if message.is_binary() {
        return Ok(message.into_data().clone());
    }

    if !message.is_text() {
        return Err("unsupported websocket message kind".to_string());
    }

    let payload = message
        .into_text()
        .map_err(|error| format!("decode websocket text frame: {error}"))?;
    let wrapped: HarnessTransportEnvelopeMessage = serde_json::from_str(&payload)
        .map_err(|error| format!("decode harness transport wrapper: {error}"))?;
    if wrapped.kind != "transport_envelope" {
        return Err(format!(
            "unsupported harness transport wrapper kind: {}",
            wrapped.kind
        ));
    }

    STANDARD
        .decode(wrapped.envelope_b64.as_bytes())
        .map_err(|error| format!("decode harness transport payload: {error}"))
}

pub(crate) fn sync_peer_reconcile_interval(sync_manager: &SyncServiceManager) -> Duration {
    sync_manager.config().auto_sync_interval.clamp(
        MIN_SYNC_PEER_RECONCILE_INTERVAL,
        MAX_SYNC_PEER_RECONCILE_INTERVAL,
    )
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeShutdownError {
    #[error("runtime shutdown cannot claim completion from {state:?} admission state")]
    AdmissionClosed { state: RuntimeActivityState },
    #[error("shutdown window belongs to another actual runtime owner")]
    ForeignWindow(#[source] RuntimePublicOperationError),
    #[error("runtime task tree shutdown failed: {0}")]
    TaskTree(#[from] TaskSupervisionError),
    #[error("runtime service teardown failed: {0}")]
    Service(#[from] ServiceError),
    #[error("lifecycle shutdown failed: {0}")]
    Lifecycle(crate::AgentError),
    #[error("original runtime shutdown window failed: {0}")]
    Budget(#[source] aura_core::TimeoutBudgetError),
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
    #[error("runtime operation admission capacity exhausted")]
    AdmissionCapacity,
    #[error("runtime handoff lease belongs to a different activity owner")]
    ForeignHandoff,
}

// Runtime-private. No Clone/Deserialize constructors for leases or drained proof.
// Resource policy: at most 256 simultaneously admitted public operations.
// This is a local ingress limit, not a signed protocol window or quorum.
const MAX_ADMITTED_PUBLIC_OPERATIONS: u64 = 256;
const ACTIVITY_STATE_SHIFT: u32 = 62;
const ACTIVITY_COUNT_MASK: u64 = (1_u64 << ACTIVITY_STATE_SHIFT) - 1;
#[derive(Debug, Default)]
pub struct RuntimeActivityGate {
    activity: std::sync::atomic::AtomicU64,
    drained: tokio::sync::Notify,
}
#[derive(Debug)]
pub(crate) struct RuntimeOperationLease {
    gate: Arc<RuntimeActivityGate>,
}
impl RuntimeOperationLease {
    pub(crate) fn require_gate(
        &self,
        gate: &Arc<RuntimeActivityGate>,
    ) -> crate::core::AgentResult<()> {
        if Arc::ptr_eq(&self.gate, gate) {
            Ok(())
        } else {
            Err(RuntimePublicOperationError::ForeignHandoff.into())
        }
    }
}
impl From<RuntimePublicOperationError> for crate::core::AgentError {
    fn from(source: RuntimePublicOperationError) -> Self {
        let message = source.to_string();
        let error = match source {
            source @ (RuntimePublicOperationError::NotAccepting { .. }
            | RuntimePublicOperationError::ForeignHandoff) => {
                aura_core::AuraError::PermissionDenied {
                    message,
                    source: Some(Arc::new(source)),
                }
            }
            source @ RuntimePublicOperationError::AdmissionCapacity => {
                aura_core::AuraError::Internal {
                    message,
                    source: Some(Arc::new(source)),
                }
            }
        };
        crate::core::AgentError::from(error)
    }
}
impl Drop for RuntimeOperationLease {
    fn drop(&mut self) {
        let previous = self.gate.activity.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous & ACTIVITY_COUNT_MASK != 0);
        self.gate.drained.notify_waiters();
    }
}
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct DrainedRuntimeOperationsCapability {
    handoff: RuntimeOperationLease,
}
#[cfg(test)]
impl DrainedRuntimeOperationsCapability {
    pub(crate) fn require_gate(
        &self,
        gate: &Arc<RuntimeActivityGate>,
    ) -> Result<(), RuntimePublicOperationError> {
        if Arc::ptr_eq(&self.handoff.gate, gate) && gate.state() == RuntimeActivityState::Stopping {
            Ok(())
        } else {
            Err(RuntimePublicOperationError::ForeignHandoff)
        }
    }
}
impl RuntimeActivityGate {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn state(&self) -> RuntimeActivityState {
        RuntimeActivityState::from_u8(
            (self.activity.load(Ordering::SeqCst) >> ACTIVITY_STATE_SHIFT) as u8,
        )
    }
    pub(crate) fn begin_shutdown(&self) -> RuntimeActivityState {
        let mut observed = self.activity.load(Ordering::SeqCst);
        loop {
            let state = RuntimeActivityState::from_u8((observed >> ACTIVITY_STATE_SHIFT) as u8);
            if state != RuntimeActivityState::Running {
                return state;
            }
            let closed = observed
                | (u64::from(RuntimeActivityState::Stopping.as_u8()) << ACTIVITY_STATE_SHIFT);
            match self.activity.compare_exchange(
                observed,
                closed,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return state,
                Err(actual) => observed = actual,
            }
        }
    }
    fn mark_stopped(&self) {
        let mut observed = self.activity.load(Ordering::SeqCst);
        loop {
            let stopped = (observed & ACTIVITY_COUNT_MASK)
                | (u64::from(RuntimeActivityState::Stopped.as_u8()) << ACTIVITY_STATE_SHIFT);
            match self.activity.compare_exchange(
                observed,
                stopped,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return,
                Err(actual) => observed = actual,
            }
        }
    }
    pub fn ensure_accepting_public_operations(&self) -> Result<(), RuntimePublicOperationError> {
        match self.state() {
            RuntimeActivityState::Running => Ok(()),
            state => Err(RuntimePublicOperationError::NotAccepting { state }),
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "runtime_operation_admission", capability_type = RuntimeOperationLease,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) fn admit(
        self: &Arc<Self>,
    ) -> Result<RuntimeOperationLease, RuntimePublicOperationError> {
        let mut observed = self.activity.load(Ordering::SeqCst);
        loop {
            let state = RuntimeActivityState::from_u8((observed >> ACTIVITY_STATE_SHIFT) as u8);
            if state != RuntimeActivityState::Running {
                return Err(RuntimePublicOperationError::NotAccepting { state });
            }
            if observed & ACTIVITY_COUNT_MASK >= MAX_ADMITTED_PUBLIC_OPERATIONS {
                return Err(RuntimePublicOperationError::AdmissionCapacity);
            }
            match self.activity.compare_exchange(
                observed,
                observed + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(RuntimeOperationLease { gate: self.clone() }),
                Err(actual) => observed = actual,
            }
        }
    }
    async fn wait_for_count(&self, retained: u64) {
        loop {
            let notification = self.drained.notified();
            tokio::pin!(notification);
            notification.as_mut().enable();
            if self.activity.load(Ordering::SeqCst) & ACTIVITY_COUNT_MASK == retained {
                return;
            }
            notification.await;
        }
    }
    pub(crate) async fn wait_for_operations(&self) {
        self.wait_for_count(0).await;
    }
    #[cfg(test)]
    pub(crate) async fn drain_for_handoff(
        self: &Arc<Self>,
        handoff: RuntimeOperationLease,
    ) -> Result<DrainedRuntimeOperationsCapability, RuntimePublicOperationError> {
        if !Arc::ptr_eq(self, &handoff.gate) {
            return Err(RuntimePublicOperationError::ForeignHandoff);
        }
        let previous = self.begin_shutdown();
        if previous != RuntimeActivityState::Running {
            return Err(RuntimePublicOperationError::NotAccepting { state: previous });
        }
        self.wait_for_count(1).await;
        Ok(DrainedRuntimeOperationsCapability { handoff })
    }
}

/// Retains one exact runtime admission through advanced effect-based execution.
/// This capability is move-owned and exposes no shared effect-system handle.
/// It is not a complete-runtime drain or profile reassembly capability.
pub struct AdmittedRuntimeEffectsCapability {
    effects: Arc<AuraEffectSystem>,
    operation: RuntimeOperationLease,
}
#[derive(Debug, thiserror::Error)]
#[error("runtime inbox rejected admitted delivery: {outcome:?}")]
struct RuntimeEnvelopeQueueError {
    outcome: crate::runtime::subsystems::transport::QueueEnvelopeOutcome,
}
impl AdmittedRuntimeEffectsCapability {
    pub fn effects(&self) -> &AuraEffectSystem {
        self.effects.as_ref()
    }
    pub fn requeue_envelope(
        &self,
        envelope: aura_core::effects::transport::TransportEnvelope,
    ) -> crate::core::AgentResult<()> {
        let outcome = self.effects.requeue_envelope(envelope);
        match outcome {
            crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued => Ok(()),
            rejected @ crate::runtime::subsystems::transport::QueueEnvelopeOutcome::DroppedOverflow => Err(aura_core::AuraError::Internal {
                message: "admitted runtime envelope could not enter its owned inbox".into(),
                source: Some(Arc::new(RuntimeEnvelopeQueueError { outcome: rejected })),
            }.into()),
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "runtime_effect_admission", capability_type = AdmittedRuntimeEffectsCapability, receiver_type = AdmittedRuntimeEffectsCapability, family = "runtime_helper")]
    pub(crate) fn require_runtime(&self, runtime: &RuntimeSystem) -> crate::core::AgentResult<()> {
        self.operation.require_gate(&runtime.activity_gate)?;
        if Arc::ptr_eq(&self.effects, &runtime.effect_system) {
            Ok(())
        } else {
            Err(RuntimePublicOperationError::ForeignHandoff.into())
        }
    }
}

/// Exact successful admission closure; only this actual gate can mint it.
struct ClosedRuntimeAdmissionCapability {
    gate: Arc<RuntimeActivityGate>,
}

/// A private original-shutdown identity, not a cleanup completion proof.
/// Only the actual retained window can mint it. It is never serialized.
pub(crate) struct RuntimeShutdownOwnerReference {
    identity: Arc<()>,
}

/// One shutdown resource origin retained through required disposal.
/// No Clone/Deserialize, raw timestamp, caller budget or public constructor.
pub(in crate::runtime) struct RuntimeShutdownWindowCapability {
    identity: Arc<()>,
    closed: ClosedRuntimeAdmissionCapability,
    effects: Arc<AuraEffectSystem>,
    budget: TimeoutBudget,
    task_root: Arc<TaskSupervisor>,
}
impl RuntimeShutdownWindowCapability {
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "RuntimeShutdownWindowCapability", capability_type = RuntimeShutdownWindowCapability,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    async fn from_closed_runtime(
        runtime: &RuntimeSystem,
        closed: ClosedRuntimeAdmissionCapability,
    ) -> Result<RuntimeShutdownWindowCapability, RuntimeShutdownError> {
        if !Arc::ptr_eq(&closed.gate, &runtime.activity_gate)
            || closed.gate.state() != RuntimeActivityState::Stopping
        {
            return Err(RuntimeShutdownError::ForeignWindow(
                RuntimePublicOperationError::ForeignHandoff,
            ));
        }
        let started = runtime
            .effect_system
            .physical_time()
            .await
            .map_err(|source| {
                RuntimeShutdownError::Budget(aura_core::TimeoutBudgetError::time_source_failure(
                    source,
                ))
            })?;
        let budget = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))
            .map_err(RuntimeShutdownError::Budget)?;
        let original = Self {
            identity: Arc::new(()),
            closed,
            effects: runtime.effect_system.clone(),
            task_root: runtime.runtime_tasks.clone(),
            budget,
        };
        original.require_runtime(runtime)?;
        Ok(original)
    }

    fn require_runtime(&self, runtime: &RuntimeSystem) -> Result<(), RuntimeShutdownError> {
        if Arc::ptr_eq(&self.closed.gate, &runtime.activity_gate)
            && Arc::ptr_eq(&self.effects, &runtime.effect_system)
            && Arc::ptr_eq(&self.task_root, &runtime.runtime_tasks)
            && self.closed.gate.state() == RuntimeActivityState::Stopping
        {
            Ok(())
        } else {
            Err(RuntimeShutdownError::ForeignWindow(
                RuntimePublicOperationError::ForeignHandoff,
            ))
        }
    }
    #[aura_macros::capability_boundary(category="capability_gated", capability="RuntimeShutdownWindowCapability", capability_type=RuntimeShutdownWindowCapability, receiver_type=RuntimeShutdownWindowCapability, family="runtime_helper")]
    pub(crate) fn require_service_owner(
        &self,
        effects: &AuraEffectSystem,
        root: &Arc<TaskSupervisor>,
    ) -> Result<(), RuntimeShutdownError> {
        if std::ptr::eq(self.effects.as_ref(), effects)
            && Arc::ptr_eq(&self.closed.gate, &effects.public_operation_activity())
            && Arc::ptr_eq(&self.task_root, root)
            && self.closed.gate.state() == RuntimeActivityState::Stopping
        {
            Ok(())
        } else {
            Err(RuntimeShutdownError::ForeignWindow(
                RuntimePublicOperationError::ForeignHandoff,
            ))
        }
    }

    #[aura_macros::capability_boundary(category="capability_gated", capability="RuntimeShutdownWindowCapability", capability_type=RuntimeShutdownWindowCapability, receiver_type=RuntimeShutdownWindowCapability, family="runtime_helper")]
    pub(crate) async fn execute_service<T, F, Fut>(
        &self,
        effects: &AuraEffectSystem,
        root: &Arc<TaskSupervisor>,
        service: &'static str,
        operation: F,
    ) -> Result<T, ServiceError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, ServiceError>>,
    {
        self.require_service_owner(effects, root)
            .map_err(|source| {
                ServiceError::shutdown_failed(service, "foreign original shutdown owner")
                    .with_cause(source)
            })?;
        execute_with_timeout_budget(effects, &self.budget, operation)
            .await
            .map_err(|source| match source {
                TimeoutRunError::Operation(source) => source,
                TimeoutRunError::Timeout(
                    source @ aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
                ) => ServiceError::new(
                    service,
                    ServiceErrorKind::Timeout,
                    "original shutdown resource deadline exceeded",
                )
                .with_cause(source),
                TimeoutRunError::Timeout(source) => ServiceError::new(
                    service,
                    ServiceErrorKind::Internal,
                    "required original shutdown observation failed",
                )
                .with_cause(source),
            })
    }
    pub(crate) fn owner_reference(&self) -> RuntimeShutdownOwnerReference {
        RuntimeShutdownOwnerReference {
            identity: self.identity.clone(),
        }
    }
    pub(crate) fn same_owner(&self, original: &RuntimeShutdownOwnerReference) -> bool {
        Arc::ptr_eq(&self.identity, &original.identity)
    }
    #[aura_macros::capability_boundary(category="capability_gated", capability="RuntimeShutdownWindowCapability", capability_type=RuntimeShutdownWindowCapability, receiver_type=RuntimeShutdownWindowCapability, family="runtime_helper")]
    pub(crate) async fn shutdown_service_tasks(
        &self,
        effects: &AuraEffectSystem,
        root: &Arc<TaskSupervisor>,
        service: &'static str,
        tasks: &crate::task_registry::TaskGroup,
    ) -> Result<(), ServiceError> {
        self.require_service_owner(effects, root)
            .map_err(|source| {
                ServiceError::shutdown_failed(service, "foreign original shutdown owner")
                    .with_cause(source)
            })?;
        if !root.owns_group(tasks) {
            return Err(
                ServiceError::shutdown_failed(service, "foreign service task root").with_cause(
                    RuntimeShutdownError::ForeignWindow(
                        RuntimePublicOperationError::ForeignHandoff,
                    ),
                ),
            );
        }
        tasks
            .shutdown_with_original_budget(effects, &self.budget)
            .await
            .map_err(|source| {
                ServiceError::shutdown_failed(
                    service,
                    "required owned service task disposal failed",
                )
                .with_cause(source)
            })
    }

    /// Acknowledge resource-bounded progress after required work has completed.
    /// This does not itself prove service cleanup; the private service owner
    /// supplies the synchronous publication after its actual stop/task ACKs.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RuntimeShutdownWindowCapability", receiver_type = RuntimeShutdownWindowCapability, family = "runtime_helper")]
    pub(in crate::runtime) async fn acknowledge_service_progress(
        &self,
        effects: &AuraEffectSystem,
        root: &Arc<TaskSupervisor>,
        service: &'static str,
        publish: impl FnOnce(),
    ) -> Result<(), ServiceError> {
        self.require_service_owner(effects, root)
            .map_err(|source| {
                ServiceError::shutdown_failed(
                    service,
                    "foreign original shutdown acknowledgment owner",
                )
                .with_cause(source)
            })?;
        aura_core::time::timeout::acknowledge_with_timeout_budget(
            effects,
            &self.budget,
            || async { Ok(()) },
            publish,
        )
        .await
        .map_err(|source| {
            crate::runtime::services::traits::service_window_failure(
                service,
                aura_core::time::timeout::TimeoutRunError::<ServiceError>::Timeout(source),
            )
        })
    }

    pub(super) fn budget(&self) -> &TimeoutBudget {
        &self.budget
    }
}
impl RuntimeActivityGate {
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "ClosedRuntimeAdmissionCapability", capability_type = ClosedRuntimeAdmissionCapability,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    fn close_owned(
        self: &Arc<Self>,
    ) -> Result<ClosedRuntimeAdmissionCapability, RuntimeShutdownError> {
        let state = self.begin_shutdown();
        if state != RuntimeActivityState::Running {
            tracing::info!(
                event = RuntimeShutdownEvent::AlreadyInProgress.as_event_name(),
                previous_state = ?state,
                "Runtime shutdown requested after admission had closed"
            );
            return Err(RuntimeShutdownError::AdmissionClosed { state });
        }
        Ok(ClosedRuntimeAdmissionCapability { gate: self.clone() })
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

    /// Runtime-held foreground sync commands, including cancelled clients.
    sync_command_registry: super::services::sync_command_registry::SyncCommandRegistryService,

    /// Sync service manager (optional, for background journal synchronization)
    sync_manager: Option<SyncServiceManager>,

    /// Rendezvous manager (optional, for peer discovery and channel establishment)
    rendezvous_manager: Option<RendezvousManager>,

    /// Move manager for bounded movement planning and delivery.
    move_manager: Option<MoveManager>,

    /// Local health observer for adaptive privacy policy.
    local_health_observer: Option<LocalHealthObserver>,

    /// Runtime-owned adaptive selection manager.
    selection_manager: Option<SelectionManager>,

    /// Anonymous path manager for reusable established anonymous paths.
    anonymous_path_manager: Option<AnonymousPathManager>,

    /// Hold manager for shared custody and selector-based retrieval.
    hold_manager: Option<HoldManager>,

    /// Cover traffic generator for shared move-substrate cover planning.
    cover_traffic_generator: Option<CoverTrafficGenerator>,

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
    /// Publish (or republish) this authority's LAN rendezvous descriptor.
    ///
    /// Called once first-run account bootstrap has created the identity key,
    /// so a new account is discoverable immediately instead of after the next
    /// periodic descriptor refresh.
    pub(crate) async fn publish_lan_descriptor(&self) -> Result<(), ServiceError> {
        self.maintenance_service
            .publish_initial_lan_descriptor()
            .await
    }

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
        let activity_gate = effect_system.public_operation_activity();
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new_with_storage(effect_system.clone());
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let reconfiguration_manager = ReconfigurationManager::new();
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            ceremony_tracker.clone(),
            ceremony_runner.clone(),
            threshold_signing.clone(),
            reconfiguration_manager.clone(),
            None,
            None,
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
        let sync_command_registry =
            super::services::sync_command_registry::SyncCommandRegistryService::new(
                effect_system.clone(),
                runtime_tasks.clone(),
            );
        Self {
            effect_executor,
            effect_system,
            sync_command_registry,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: None,
            move_manager: None,
            local_health_observer: None,
            selection_manager: None,
            anonymous_path_manager: None,
            hold_manager: None,
            cover_traffic_generator: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager,
            runtime_tasks,
            activity_gate,
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
        let activity_gate = effect_system.public_operation_activity();
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new_with_storage(effect_system.clone());
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let reconfiguration_manager = ReconfigurationManager::new();
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            ceremony_tracker.clone(),
            ceremony_runner.clone(),
            threshold_signing.clone(),
            reconfiguration_manager.clone(),
            Some(sync_manager.clone()),
            None,
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
        let sync_command_registry =
            super::services::sync_command_registry::SyncCommandRegistryService::new(
                effect_system.clone(),
                runtime_tasks.clone(),
            );
        Self {
            effect_executor,
            effect_system,
            sync_command_registry,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: Some(sync_manager),
            rendezvous_manager: None,
            move_manager: None,
            local_health_observer: None,
            selection_manager: None,
            anonymous_path_manager: None,
            hold_manager: None,
            cover_traffic_generator: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager,
            runtime_tasks,
            activity_gate,
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
        let activity_gate = effect_system.public_operation_activity();
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new_with_storage(effect_system.clone());
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let reconfiguration_manager = ReconfigurationManager::new();
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let reactive_pipeline_service =
            ReactivePipelineService::new(effect_system.clone(), authority_id, diagnostics.clone());
        let maintenance_service = RuntimeMaintenanceService::new(
            effect_system.clone(),
            authority_id,
            device_id,
            ceremony_tracker.clone(),
            ceremony_runner.clone(),
            threshold_signing.clone(),
            reconfiguration_manager.clone(),
            None,
            Some(rendezvous_manager.clone()),
            None,
            None,
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
        let sync_command_registry =
            super::services::sync_command_registry::SyncCommandRegistryService::new(
                effect_system.clone(),
                runtime_tasks.clone(),
            );
        Self {
            effect_executor,
            effect_system,
            sync_command_registry,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager: None,
            rendezvous_manager: Some(rendezvous_manager),
            move_manager: None,
            local_health_observer: None,
            selection_manager: None,
            anonymous_path_manager: None,
            hold_manager: None,
            cover_traffic_generator: None,
            social_manager: None,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service: None,
            maintenance_service,
            reconfiguration_manager,
            runtime_tasks,
            activity_gate,
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
        local_health_observer: Option<LocalHealthObserver>,
        selection_manager: Option<SelectionManager>,
        anonymous_path_manager: Option<AnonymousPathManager>,
        hold_manager: Option<HoldManager>,
        cover_traffic_generator: Option<CoverTrafficGenerator>,
        rendezvous_handler: Option<RendezvousHandler>,
        lan_transport: Option<Arc<LanTransportService>>,
        social_manager: Option<SocialManager>,
        config: AgentConfig,
        authority_id: AuthorityId,
    ) -> Self {
        let device_id = config.device_id;
        let activity_gate = effect_system.public_operation_activity();
        let threshold_signing = ThresholdSigningService::new(effect_system.clone());
        let ceremony_tracker = CeremonyTracker::new_with_storage(effect_system.clone());
        let ceremony_runner = CeremonyRunner::new(ceremony_tracker.clone());
        let reconfiguration_manager = ReconfigurationManager::new();
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
            ceremony_tracker.clone(),
            ceremony_runner.clone(),
            threshold_signing.clone(),
            reconfiguration_manager.clone(),
            sync_manager.clone(),
            rendezvous_manager.clone(),
            rendezvous_handler.clone(),
            lan_transport.clone(),
        );
        let runtime_tasks = Arc::new(TaskSupervisor::with_diagnostics(diagnostics.clone()));
        let sync_command_registry =
            super::services::sync_command_registry::SyncCommandRegistryService::new(
                effect_system.clone(),
                runtime_tasks.clone(),
            );
        Self {
            effect_executor,
            effect_system,
            sync_command_registry,
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            move_manager,
            local_health_observer,
            selection_manager,
            anonymous_path_manager,
            hold_manager,
            cover_traffic_generator,
            social_manager,
            ceremony_tracker,
            ceremony_runner,
            threshold_signing,
            reactive_pipeline_service,
            lan_listener_service,
            maintenance_service,
            reconfiguration_manager,
            runtime_tasks,
            activity_gate,
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

    /// Observe runtime admission without authority to publish its completion.
    ///
    /// ```
    /// fn observe(agent: &aura_agent::AuraAgent) {
    ///     let gate = agent.runtime().activity_gate();
    ///     let _ = gate.state();
    /// }
    /// ```
    ///
    /// ```compile_fail,E0624
    /// fn cannot_forge_shutdown(agent: &aura_agent::AuraAgent) {
    ///     let gate = agent.runtime().activity_gate();
    ///     gate.mark_stopped();
    /// }
    /// ```
    ///
    /// ```compile_fail,E0624
    /// fn cannot_close_admission(agent: &aura_agent::AuraAgent) {
    ///     agent.runtime().activity_gate().begin_shutdown();
    /// }
    /// ```
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
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "runtime_effects_operation", capability_type = AdmittedRuntimeEffectsCapability,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub fn admit_effects_operation(
        &self,
    ) -> crate::core::AgentResult<AdmittedRuntimeEffectsCapability> {
        let operation = self.effect_system.admit_public_operation()?;
        let capability = AdmittedRuntimeEffectsCapability {
            effects: self.effect_system.clone(),
            operation,
        };
        capability.require_runtime(self)?;
        Ok(capability)
    }

    pub fn effects(&self) -> Arc<AuraEffectSystem> {
        self.effect_system.clone()
    }

    /// Re-publish committed facts into the reactive views (see
    /// `ReactivePipelineService::replay_committed_facts`).
    pub async fn replay_committed_facts(&self) -> Result<(), ServiceError> {
        self.reactive_pipeline_service
            .replay_committed_facts()
            .await
    }

    /// Check whether the service-owned reactive pipeline is running.
    pub async fn reactive_pipeline_running(&self) -> bool {
        self.reactive_pipeline_service.is_running().await
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

    /// Register configured foreground sync on the actual runtime task/effect owner.
    /// Startup admission hands off before the returned long-lived command handle.
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "AdmittedSyncCommandCapability", capability_type = super::services::sync_command_registry::AdmittedSyncCommandCapability,
        family = "proof_issuer")]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub async fn admit_sync_command(
        &self,
        config: super::services::SyncManagerConfig,
    ) -> Result<super::services::sync_command_registry::AdmittedSyncCommandCapability, ServiceError>
    {
        let admission = self
            .effect_system
            .admit_public_operation()
            .map_err(|source| {
                ServiceError::unavailable(
                    "sync_command_registry",
                    "runtime command admission failed",
                )
                .with_cause(source)
            })?;
        self.sync_command_registry
            .start_registered(config, admission)
            .await
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

    #[cfg(test)]
    pub(in crate::runtime) async fn close_for_shutdown_test(
        &self,
    ) -> Result<RuntimeShutdownWindowCapability, RuntimeShutdownError> {
        RuntimeShutdownWindowCapability::from_closed_runtime(
            self,
            self.activity_gate.close_owned()?,
        )
        .await
    }

    pub async fn shutdown_typed(self, ctx: &EffectContext) -> Result<(), RuntimeShutdownError> {
        let closed = self.activity_gate.close_owned()?;

        let original = RuntimeShutdownWindowCapability::from_closed_runtime(&self, closed).await?;
        match execute_with_timeout_budget(
            self.effect_system.as_ref(),
            original.budget(),
            || async {
                self.activity_gate.wait_for_operations().await;
                Ok::<(), RuntimeShutdownError>(())
            },
        )
        .await
        {
            Ok(()) => {}
            Err(TimeoutRunError::Operation(source)) => return Err(source),
            Err(TimeoutRunError::Timeout(source)) => {
                return Err(RuntimeShutdownError::Budget(source))
            }
        }

        let runtime_tasks = self.runtime_tasks.clone();
        let mut shutdown_error: Option<RuntimeShutdownError> = None;

        // Registered command actors require their explicit cleanup ACK while
        // their callbacks and the reactive pipeline are still live. Closing
        // public admission has already prevented new commands; the original
        // shutdown owner bounds both manager cleanup and natural task completion.
        if let Err(source) = original
            .execute_service(
                self.effect_system.as_ref(),
                &self.runtime_tasks,
                "sync_command_registry",
                || async {
                    self.sync_command_registry
                        .stop_with_original_shutdown(&original)
                        .await?;
                    if !matches!(
                        self.sync_command_registry.health().await,
                        ServiceHealth::Stopped
                    ) {
                        return Err(ServiceError::shutdown_failed(
                            "sync_command_registry",
                            "required original registry stop ACK is absent",
                        ));
                    }
                    Ok(())
                },
            )
            .await
        {
            shutdown_error.get_or_insert(RuntimeShutdownError::Service(source));
        }

        // Drain the reactive scheduler before cancelling the broader runtime task tree.
        tracing::info!(
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "reactive_pipeline",
            "Starting runtime shutdown"
        );
        let pipeline_result =
            execute_with_timeout_budget(self.effect_system.as_ref(), original.budget(), || {
                self.reactive_pipeline_service
                    .stop_with_original_budget(original.budget())
            })
            .await;
        if let Err(error) = &pipeline_result {
            tracing::warn!(
                event = RuntimeShutdownEvent::ReactivePipelineSignalFailed.as_event_name(),
                error = %error,
                "Required reactive pipeline shutdown failed"
            );
        }
        match pipeline_result {
            Ok(()) => {}
            Err(TimeoutRunError::Operation(source)) => {
                shutdown_error.get_or_insert(RuntimeShutdownError::Service(source));
            }
            Err(TimeoutRunError::Timeout(source)) => {
                shutdown_error.get_or_insert(RuntimeShutdownError::Budget(source));
            }
        }

        tracing::info!(
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "task_tree",
            "Cancelling runtime task tree"
        );
        if let Err(error) = runtime_tasks
            .shutdown_with_original_budget(self.effect_system.as_ref(), original.budget())
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
        let stopped_services =
            execute_with_timeout_budget(self.effect_system.as_ref(), original.budget(), || {
                self.stop_services(&original)
            })
            .await;
        let service_failure = match stopped_services {
            Ok(()) => None,
            Err(TimeoutRunError::Operation(source)) => Some(RuntimeShutdownError::Service(source)),
            Err(TimeoutRunError::Timeout(source)) => Some(RuntimeShutdownError::Budget(source)),
        };
        if let Some(e) = service_failure {
            tracing::warn!(
                event = RuntimeShutdownEvent::ServicesFailed.as_event_name(),
                error = %e,
                "Failed to stop runtime services during shutdown"
            );
            shutdown_error.get_or_insert(e);
        }

        let shutdown_effects = self.effect_system.clone();
        let shutdown_activity = self.activity_gate.clone();
        let RuntimeSystem {
            lifecycle_manager,
            authority_manager,
            authority_id,
            sync_manager: _sync_manager,
            rendezvous_manager: _rendezvous_manager,
            ..
        } = self;

        tracing::info!(
            event = RuntimeShutdownEvent::Stage.as_event_name(),
            stage = "lifecycle_manager",
            "Shutting down lifecycle manager"
        );
        let lifecycle_result =
            execute_with_timeout_budget(shutdown_effects.as_ref(), original.budget(), || {
                lifecycle_manager.shutdown(ctx)
            })
            .await;
        let lifecycle_failure = match lifecycle_result {
            Ok(()) => None,
            Err(TimeoutRunError::Operation(source)) => {
                Some(RuntimeShutdownError::Lifecycle(source))
            }
            Err(TimeoutRunError::Timeout(source)) => Some(RuntimeShutdownError::Budget(source)),
        };
        if let Some(error) = lifecycle_failure {
            tracing::warn!(
                event = RuntimeShutdownEvent::LifecycleFailed.as_event_name(),
                error = %error,
                "Lifecycle manager shutdown failed"
            );
            shutdown_error.get_or_insert(error);
        }

        match shutdown_error {
            Some(error) => Err(error),
            None => {
                // All required stage and lifecycle ACKs precede authority termination.
                // Earlier failures still ran cleanup, but never enter this branch.
                execute_with_timeout_budget(
                    shutdown_effects.as_ref(),
                    original.budget(),
                    || async {
                        let now = shutdown_effects.physical_time().await.map_err(|source| {
                            RuntimeShutdownError::Budget(
                                aura_core::TimeoutBudgetError::time_source_failure(source),
                            )
                        })?;
                        authority_manager
                            .set_status(authority_id, AuthorityStatus::Terminated, now.ts_ms)
                            .await
                            .map_err(|source| {
                                RuntimeShutdownError::Service(
                                    ServiceError::shutdown_failed(
                                        "authority_manager",
                                        "required final authority termination",
                                    )
                                    .with_cause(source),
                                )
                            })?;
                        Ok::<(), RuntimeShutdownError>(())
                    },
                )
                .await
                .map_err(|source| match source {
                    TimeoutRunError::Operation(source) => source,
                    TimeoutRunError::Timeout(source) => RuntimeShutdownError::Budget(source),
                })?;
                shutdown_activity.mark_stopped();
                Ok(())
            }
        }
    }
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
    let ingress_controller = LanIngressController::new();
    let time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync> =
        Arc::new(effects.time_effects().clone());
    let websocket_effects = effects.clone();
    let tcp_accept_group = parent_tasks.clone();
    let tcp_connection_group = tcp_accept_group.clone();
    let tcp_ingress_controller = ingress_controller.clone();
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
                let Some(connection_permit) = tcp_ingress_controller.try_acquire_connection() else {
                    let now_ms = time_effects
                        .physical_time()
                        .await
                        .ok()
                        .map(|t| t.ts_ms)
                        .unwrap_or(0);
                    tracing::warn!(
                        addr = %addr,
                        limit = MAX_LAN_CONCURRENT_CONNECTIONS,
                        "LAN transport rejected connection because the connection budget is exhausted"
                    );
                    let mut metrics = metrics.write().await;
                    metrics.connections_rejected =
                        metrics.connections_rejected.saturating_add(1);
                    if now_ms > 0 {
                        metrics.last_error_ms = now_ms;
                    }
                    continue;
                };

                let effects = effects.clone();
                let metrics = metrics.clone();
                let time_effects = time_effects.clone();
                let ingress_controller = tcp_ingress_controller.clone();
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
                        let _connection_permit = connection_permit;
                        let mut len_buf = [0u8; 4];
                        if let Err(err) = stream.read_exact(&mut len_buf).await {
                            tracing::debug!(
                                error = %err,
                                addr = %addr,
                                "LAN transport read len failed"
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
                        let len = u32::from_be_bytes(len_buf) as usize;
                        if len == 0 || len > MAX_LAN_FRAME_BYTES {
                            tracing::debug!(
                                addr = %addr,
                                len = len,
                                max_len = MAX_LAN_FRAME_BYTES,
                                "LAN transport invalid frame size"
                            );
                            let now_ms = time_effects
                                .physical_time()
                                .await
                                .ok()
                                .map(|t| t.ts_ms)
                                .unwrap_or(0);
                            let mut metrics = metrics.write().await;
                            metrics.frames_rejected = metrics.frames_rejected.saturating_add(1);
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

                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .ok()
                            .map(|t| t.ts_ms)
                            .unwrap_or(0);
                        if !ingress_controller.allow_frame(addr.ip(), now_ms).await {
                            tracing::warn!(
                                addr = %addr,
                                per_peer_limit = MAX_LAN_FRAMES_PER_PEER_WINDOW,
                                window_ms = LAN_PEER_RATE_WINDOW_MS,
                                "LAN transport rate-limited inbound frame"
                            );
                            let mut metrics = metrics.write().await;
                            metrics.frames_rejected = metrics.frames_rejected.saturating_add(1);
                            if now_ms > 0 {
                                metrics.last_error_ms = now_ms;
                            }
                            return;
                        }

                        let envelope = match aura_core::util::serialization::from_slice(&payload) {
                            Ok(envelope) => envelope,
                            Err(err) => {
                                tracing::debug!(
                                    error = %err,
                                    addr = %addr,
                                    "LAN transport decode failed"
                                );
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
                        {
                            let mut metrics = metrics.write().await;
                            metrics.frames_received = metrics.frames_received.saturating_add(1);
                            metrics.bytes_received = metrics.bytes_received.saturating_add(len as u64);
                            if now_ms > 0 {
                                metrics.last_frame_ms = now_ms;
                            }
                        }

                        let _ = handle_inbound_transport_envelope(effects, metrics, envelope).await;
                    });
            }
        });

    let metrics = lan_transport.metrics_handle();
    let ingress_controller = ingress_controller.clone();
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
                let Some(connection_permit) = ingress_controller.try_acquire_connection() else {
                    let now_ms = time_effects
                        .physical_time()
                        .await
                        .ok()
                        .map(|t| t.ts_ms)
                        .unwrap_or(0);
                    tracing::warn!(
                        addr = %addr,
                        limit = MAX_LAN_CONCURRENT_CONNECTIONS,
                        "LAN websocket rejected connection because the connection budget is exhausted"
                    );
                    let mut metrics = metrics.write().await;
                    metrics.connections_rejected =
                        metrics.connections_rejected.saturating_add(1);
                    if now_ms > 0 {
                        metrics.last_error_ms = now_ms;
                    }
                    continue;
                };

                let effects = websocket_effects.clone();
                let metrics = metrics.clone();
                let time_effects = time_effects.clone();
                let ingress_controller = ingress_controller.clone();
                let connection_group = websocket_connection_group.clone();
                let _connection_task = connection_group.spawn_named(
                    format!("websocket_connection.{addr}"),
                    async move {
                        let _connection_permit = connection_permit;
                        let websocket = match accept_async_with_config(
                            stream,
                            Some(lan_websocket_config()),
                        )
                        .await
                        {
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

                            let payload = match decode_lan_websocket_message(message) {
                                Ok(payload) => payload,
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
                            if payload.len() > MAX_LAN_FRAME_BYTES {
                                let now_ms = time_effects
                                    .physical_time()
                                    .await
                                    .ok()
                                    .map(|t| t.ts_ms)
                                    .unwrap_or(0);
                                tracing::warn!(
                                    addr = %addr,
                                    len = payload.len(),
                                    max_len = MAX_LAN_FRAME_BYTES,
                                    "LAN websocket frame exceeded configured size budget"
                                );
                                let mut metrics = metrics.write().await;
                                metrics.frames_rejected = metrics.frames_rejected.saturating_add(1);
                                if now_ms > 0 {
                                    metrics.last_error_ms = now_ms;
                                }
                                return;
                            }
                            let now_ms = time_effects
                                .physical_time()
                                .await
                                .ok()
                                .map(|t| t.ts_ms)
                                .unwrap_or(0);
                            if !ingress_controller.allow_frame(addr.ip(), now_ms).await {
                                tracing::warn!(
                                    addr = %addr,
                                    per_peer_limit = MAX_LAN_FRAMES_PER_PEER_WINDOW,
                                    window_ms = LAN_PEER_RATE_WINDOW_MS,
                                    "LAN websocket rate-limited inbound frame"
                                );
                                let mut metrics = metrics.write().await;
                                metrics.frames_rejected = metrics.frames_rejected.saturating_add(1);
                                if now_ms > 0 {
                                    metrics.last_error_ms = now_ms;
                                }
                                return;
                            }
                            let envelope =
                                match aura_core::util::serialization::from_slice::<TransportEnvelope>(
                                    &payload,
                                ) {
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
                                handle_inbound_transport_envelope(
                                    effects.clone(),
                                    metrics.clone(),
                                    envelope,
                                )
                                .await
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
    metrics: Arc<tokio::sync::RwLock<LanTransportMetrics>>,
    envelope: TransportEnvelope,
) -> Option<TransportEnvelope> {
    let ingress = match check_lan_transport_integrity(envelope) {
        Ok(ingress) => ingress,
        Err(error) => {
            tracing::debug!(error = %error, "rejected LAN transport envelope before runtime handling");
            return None;
        }
    };
    // A receipt signed by its own embedded key proves frame integrity only.
    // The queued envelope retains untrusted source metadata; protocol owners
    // must resolve a trusted authority/device key before privileged mutation.
    let envelope = ingress.into_routable_envelope();
    if let Ok(now) = effects.time_effects().physical_time().await {
        effects.record_peer_reachable(envelope.source, now.ts_ms);
    }
    if matches!(
        // A fresh network envelope: it is admitted against its flow window when
        // taken, so it must not go through `requeue_envelope`'s readmit pass.
        effects.queue_runtime_envelope(envelope),
        crate::runtime::subsystems::transport::QueueEnvelopeOutcome::DroppedOverflow
    ) {
        tracing::warn!("dropping LAN envelope because the runtime inbox is at capacity");
        let mut metrics = metrics.write().await;
        metrics.queue_drops = metrics.queue_drops.saturating_add(1);
    }
    None
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, thiserror::Error)]
enum LanTransportIngressError {
    #[error("missing guard-chain receipt")]
    MissingReceipt,
    #[error("receipt does not match envelope routing metadata")]
    ReceiptRouteMismatch,
    #[error("receipt signature is empty")]
    EmptyReceiptSignature,
    #[error("receipt signature exceeds transport limit")]
    OversizedReceiptSignature,
    #[error("invalid receipt signature")]
    InvalidReceiptSignature,
    #[error("receipt nonce is zero")]
    EmptyReceiptNonce,
    #[error("missing content-type metadata")]
    MissingContentType,
    #[error("unsupported LAN envelope schema version")]
    UnsupportedSchemaVersion,
    #[error("invalid LAN envelope schema version")]
    InvalidSchemaVersion,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct IntegrityCheckedLanEnvelope(TransportEnvelope);

#[cfg(not(target_arch = "wasm32"))]
impl IntegrityCheckedLanEnvelope {
    /// Release the frame for protocol routing without asserting peer identity.
    fn into_routable_envelope(self) -> TransportEnvelope {
        self.0
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn check_lan_transport_integrity(
    envelope: TransportEnvelope,
) -> Result<IntegrityCheckedLanEnvelope, LanTransportIngressError> {
    let receipt = envelope
        .receipt
        .as_ref()
        .ok_or(LanTransportIngressError::MissingReceipt)?;

    if receipt.src != envelope.source
        || receipt.dst != envelope.destination
        || receipt.context != envelope.context
    {
        return Err(LanTransportIngressError::ReceiptRouteMismatch);
    }

    if receipt.sig.is_empty() {
        return Err(LanTransportIngressError::EmptyReceiptSignature);
    }

    if receipt.sig.len() > MAX_TRANSPORT_SIGNATURE_BYTES {
        return Err(LanTransportIngressError::OversizedReceiptSignature);
    }

    if receipt.nonce == 0 {
        return Err(LanTransportIngressError::EmptyReceiptNonce);
    }

    if !envelope.metadata.contains_key("content-type") {
        return Err(LanTransportIngressError::MissingContentType);
    }

    crate::runtime::receipt_model::verify_transport_receipt_for_envelope(receipt, &envelope)
        .map_err(|_| LanTransportIngressError::InvalidReceiptSignature)?;

    let schema_version = envelope
        .metadata
        .get("wire-format-version")
        .map(|version| {
            version
                .parse::<u16>()
                .map_err(|_| LanTransportIngressError::InvalidSchemaVersion)
        })
        .transpose()?
        .unwrap_or(aura_protocol::messages::WIRE_FORMAT_VERSION);
    if schema_version > aura_protocol::messages::WIRE_FORMAT_VERSION {
        return Err(LanTransportIngressError::UnsupportedSchemaVersion);
    }
    Ok(IntegrityCheckedLanEnvelope(envelope))
}

/// Best-effort read of the persisted account nickname for LAN announcements.
async fn load_account_nickname_suggestion(effects: &AuraEffectSystem) -> Option<String> {
    use aura_core::effects::StorageCoreEffects;
    let bytes = effects.retrieve("account.json").await.ok().flatten()?;
    serde_json::from_slice::<aura_app::views::account::AccountConfig>(&bytes)
        .ok()?
        .nickname_suggestion
        .filter(|name| !name.trim().is_empty())
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
        signing_key: [u8; 32],
    ) -> Result<(), ServiceError> {
        rendezvous_manager
            .cache_descriptor(descriptor.clone())
            .await
            .map_err(|error| ServiceError::startup_failed("rendezvous_cache", error.to_string()))?;
        rendezvous_manager
            .set_lan_descriptor(descriptor, signing_key)
            .await;
        Ok(())
    }

    async fn retrieve_lan_identity_signing_key(
        effects: &AuraEffectSystem,
        authority: AuthorityId,
    ) -> Result<[u8; 32], ServiceError> {
        effects
            .lan_discovery_signing_key(&authority)
            .await
            .map_err(|error| {
                ServiceError::startup_failed(
                    "lan_discovery_identity",
                    format!("invalid LAN discovery identity key: {error}"),
                )
            })
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
    let mut descriptor = require_published_lan_descriptor(result, device_id)?;
    // Label the announcement so peers can show a name for this candidate.
    descriptor.nickname_suggestion = load_account_nickname_suggestion(&effects).await;
    let signing_key = retrieve_lan_identity_signing_key(&effects, authority_id).await?;
    install_lan_descriptor(rendezvous_manager, descriptor, signing_key).await?;

    if let Err(error) = register_bootstrap_candidate_with(rendezvous_manager, lan_transport).await {
        tracing::debug!(
            error = %error,
            "Failed to register bootstrap candidate after publishing LAN descriptor"
        );
    }

    Ok(())
}

pub(crate) async fn register_bootstrap_candidate_with(
    rendezvous_manager: &RendezvousManager,
    lan_transport: &LanTransportService,
) -> Result<(), RendezvousManagerError> {
    let tcp_addrs = lan_transport.advertised_addrs();
    let websocket_addrs = lan_transport.websocket_addrs();
    let Some(address) = websocket_addrs
        .first()
        .cloned()
        .or_else(|| tcp_addrs.first().cloned())
    else {
        return Ok(());
    };

    rendezvous_manager
        .register_bootstrap_candidate(address, None)
        .await
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
    use std::collections::HashMap;

    fn test_envelope() -> TransportEnvelope {
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-test-envelope".to_string(),
        );
        let mut envelope = TransportEnvelope {
            destination: AuthorityId::new_from_entropy([1u8; 32]),
            source: AuthorityId::new_from_entropy([2u8; 32]),
            context: ContextId::new_from_entropy([3u8; 32]),
            payload: b"payload".to_vec(),
            metadata,
            receipt: None,
        };
        let mut receipt = aura_core::effects::transport::TransportReceipt {
            context: envelope.context,
            src: envelope.source,
            dst: envelope.destination,
            epoch: 1,
            cost: 1,
            nonce: 7,
            prev: [0u8; 32],
            sig: Vec::new(),
        };
        crate::runtime::receipt_model::sign_transport_receipt_for_envelope(
            &mut receipt,
            &envelope,
            &crate::runtime::receipt_model::test_receipt_signing_key(),
        )
        .expect("test receipt should sign");
        envelope.receipt = Some(receipt);
        envelope
    }

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

    /// Every receipt a sender stamps across a full flow window must pass the
    /// receiver's LAN checks: generations are monotone and never zero (a zero
    /// nonce is rejected as a replay).
    #[test]
    fn lan_ingress_accepts_every_receipt_in_a_flow_window() {
        use aura_core::effects::FlowBudgetEffects;

        let source = AuthorityId::new_from_entropy([12u8; 32]);
        let destination = AuthorityId::new_from_entropy([13u8; 32]);
        let context = ContextId::new_from_entropy([14u8; 32]);
        let runtime = EffectSystemBuilder::testing()
            .with_authority(source)
            .build_sync()
            .expect("build_sync should succeed in testing mode");
        let effects = runtime.effects();
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

        let window = aura_core::types::flow_window::DEFAULT_FLOW_WINDOW;
        for send in 1..=window {
            let receipt = rt
                .block_on(effects.charge_flow(&context, &destination, aura_core::FlowCost::new(1)))
                .unwrap_or_else(|error| panic!("send {send} was refused: {error}"));
            assert_eq!(receipt.nonce.value(), send, "monotone generation");

            let mut envelope = test_envelope();
            envelope.source = source;
            envelope.destination = destination;
            envelope.context = context;
            let mut transport_receipt = aura_core::effects::transport::TransportReceipt {
                context: receipt.ctx,
                src: receipt.src,
                dst: receipt.dst,
                epoch: receipt.epoch.value(),
                cost: receipt.cost.value(),
                nonce: receipt.nonce.value(),
                prev: receipt.prev.0,
                sig: Vec::new(),
            };
            crate::runtime::receipt_model::sign_transport_receipt_for_envelope(
                &mut transport_receipt,
                &envelope,
                &crate::runtime::receipt_model::test_receipt_signing_key(),
            )
            .expect("test receipt should sign");
            envelope.receipt = Some(transport_receipt);

            check_lan_transport_integrity(envelope)
                .unwrap_or_else(|error| panic!("send {send} rejected at ingress: {error}"));
        }
        assert!(
            rt.block_on(effects.charge_flow(&context, &destination, aura_core::FlowCost::new(1)))
                .is_err(),
            "the sender stops at its window without a checkpoint"
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
    fn lan_ingress_rejects_missing_receipt() {
        let mut envelope = test_envelope();
        envelope.receipt = None;

        let error = check_lan_transport_integrity(envelope)
            .expect_err("unsigned LAN envelope must be rejected");

        assert!(matches!(error, LanTransportIngressError::MissingReceipt));
    }

    #[test]
    fn lan_ingress_rejects_route_mismatch() {
        let mut envelope = test_envelope();
        envelope.receipt.as_mut().expect("receipt").dst = AuthorityId::new_from_entropy([4u8; 32]);

        let error = check_lan_transport_integrity(envelope)
            .expect_err("mismatched receipt route must be rejected");

        assert!(matches!(
            error,
            LanTransportIngressError::ReceiptRouteMismatch
        ));
    }

    #[test]
    fn lan_ingress_rejects_missing_content_type() {
        let mut envelope = test_envelope();
        envelope.metadata.clear();

        let error = check_lan_transport_integrity(envelope)
            .expect_err("content-type is required for LAN ingress");

        assert!(matches!(
            error,
            LanTransportIngressError::MissingContentType
        ));
    }

    #[test]
    fn lan_ingress_checks_integrity_without_claiming_peer_identity() {
        let envelope = test_envelope();

        let checked = check_lan_transport_integrity(envelope)
            .expect("well-formed self-certified LAN frame should pass integrity checks");
        let envelope = checked.into_routable_envelope();
        assert_eq!(
            envelope.metadata.get("content-type").map(String::as_str),
            Some("application/aura-test-envelope")
        );
    }

    #[test]
    fn lan_ingress_rejects_unsupported_schema() {
        let mut envelope = test_envelope();
        envelope.metadata.insert(
            "wire-format-version".to_string(),
            (aura_protocol::messages::WIRE_FORMAT_VERSION + 1).to_string(),
        );
        let mut receipt = envelope.receipt.take().expect("receipt");
        crate::runtime::receipt_model::sign_transport_receipt_for_envelope(
            &mut receipt,
            &envelope,
            &crate::runtime::receipt_model::test_receipt_signing_key(),
        )
        .expect("receipt should bind future schema frame");
        envelope.receipt = Some(receipt);
        let error =
            check_lan_transport_integrity(envelope).expect_err("future schema must be rejected");
        assert!(matches!(
            error,
            LanTransportIngressError::UnsupportedSchemaVersion
        ));
    }

    #[test]
    fn lan_websocket_config_bounds_message_and_frame_sizes() {
        let config = lan_websocket_config();

        assert_eq!(config.max_message_size, Some(MAX_LAN_FRAME_BYTES));
        assert_eq!(config.max_frame_size, Some(MAX_LAN_FRAME_BYTES));
    }

    #[test]
    fn lan_websocket_decoder_accepts_binary_transport_envelopes() {
        let envelope = test_envelope();
        let payload =
            aura_core::util::serialization::to_vec(&envelope).expect("serialize test envelope");

        let decoded = decode_lan_websocket_message(
            tokio_tungstenite::tungstenite::Message::Binary(payload.clone()),
        )
        .expect("binary websocket payload should decode");

        assert_eq!(decoded, payload);
    }

    #[test]
    fn lan_websocket_decoder_accepts_wrapped_harness_transport_envelopes() {
        let envelope = test_envelope();
        let payload =
            aura_core::util::serialization::to_vec(&envelope).expect("serialize test envelope");
        let wrapped = serde_json::json!({
            "kind": "transport_envelope",
            "destination": envelope.destination.to_string(),
            "envelope_b64": STANDARD.encode(payload.as_slice()),
        });

        let decoded = decode_lan_websocket_message(tokio_tungstenite::tungstenite::Message::Text(
            wrapped.to_string(),
        ))
        .expect("wrapped harness websocket payload should decode");

        assert_eq!(decoded, payload);
    }

    #[tokio::test]
    async fn lan_ingress_controller_rate_limits_per_peer() {
        let controller = LanIngressController::new();
        let peer = std::net::Ipv4Addr::LOCALHOST.into();

        for _ in 0..MAX_LAN_FRAMES_PER_PEER_WINDOW {
            assert!(controller.allow_frame(peer, 1).await);
        }

        assert!(!controller.allow_frame(peer, 1).await);
        assert!(
            controller
                .allow_frame(peer, LAN_PEER_RATE_WINDOW_MS + 1)
                .await
        );
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

#[cfg(test)]
mod operation_drain_tests {
    use super::*;
    #[tokio::test]
    async fn admitted_operation_blocks_handoff_until_its_actual_lease_drops(
    ) -> Result<(), RuntimePublicOperationError> {
        let gate = Arc::new(RuntimeActivityGate::new());
        let handoff = gate.admit()?;
        let active = gate.admit()?;
        let drain = gate.drain_for_handoff(handoff);
        tokio::pin!(drain);
        assert!(futures::poll!(drain.as_mut()).is_pending());
        assert!(matches!(
            gate.admit(),
            Err(RuntimePublicOperationError::NotAccepting {
                state: RuntimeActivityState::Stopping
            })
        ));
        drop(active);
        let proof = drain.await?;
        proof.require_gate(&gate)?;
        assert!(matches!(
            proof.require_gate(&Arc::new(RuntimeActivityGate::new())),
            Err(RuntimePublicOperationError::ForeignHandoff)
        ));
        Ok(())
    }
    #[tokio::test]
    async fn foreign_lease_cannot_close_or_drain_original_runtime(
    ) -> Result<(), RuntimePublicOperationError> {
        let gate = Arc::new(RuntimeActivityGate::new());
        let foreign = Arc::new(RuntimeActivityGate::new());
        assert!(matches!(
            gate.drain_for_handoff(foreign.admit()?).await,
            Err(RuntimePublicOperationError::ForeignHandoff)
        ));
        assert_eq!(gate.state(), RuntimeActivityState::Running);
        Ok(())
    }
    #[tokio::test]
    async fn cancellation_releases_operation_but_does_not_reopen_closed_admission(
    ) -> Result<(), RuntimePublicOperationError> {
        let gate = Arc::new(RuntimeActivityGate::new());
        let handoff = gate.admit()?;
        let active = gate.admit()?;
        {
            let drain = gate.drain_for_handoff(handoff);
            tokio::pin!(drain);
            assert!(futures::poll!(drain.as_mut()).is_pending());
        }
        drop(active);
        gate.wait_for_operations().await;
        assert_eq!(gate.state(), RuntimeActivityState::Stopping);
        assert!(gate.admit().is_err());
        Ok(())
    }
}
#[cfg(test)]
mod operation_capacity_tests {
    use super::*;

    #[test]
    fn admission_capacity_is_finite_and_released_only_by_actual_lease_drop(
    ) -> Result<(), RuntimePublicOperationError> {
        let gate = Arc::new(RuntimeActivityGate::new());
        let mut admitted = Vec::new();
        for _ in 0..MAX_ADMITTED_PUBLIC_OPERATIONS {
            admitted.push(gate.admit()?);
        }
        assert!(matches!(
            gate.admit(),
            Err(RuntimePublicOperationError::AdmissionCapacity)
        ));
        assert_eq!(gate.state(), RuntimeActivityState::Running);
        drop(admitted.pop());
        let replacement = gate.admit()?;
        assert!(matches!(
            gate.admit(),
            Err(RuntimePublicOperationError::AdmissionCapacity)
        ));
        drop(replacement);
        drop(admitted);
        assert_eq!(
            gate.activity.load(Ordering::SeqCst) & ACTIVITY_COUNT_MASK,
            0
        );
        Ok(())
    }

    #[test]
    fn closed_admission_has_precedence_over_resource_capacity(
    ) -> Result<(), RuntimePublicOperationError> {
        let gate = Arc::new(RuntimeActivityGate::new());
        let mut admitted = Vec::new();
        for _ in 0..MAX_ADMITTED_PUBLIC_OPERATIONS {
            admitted.push(gate.admit()?);
        }
        assert_eq!(gate.begin_shutdown(), RuntimeActivityState::Running);
        assert!(matches!(
            gate.admit(),
            Err(RuntimePublicOperationError::NotAccepting {
                state: RuntimeActivityState::Stopping
            })
        ));
        drop(admitted);
        assert!(matches!(
            gate.admit(),
            Err(RuntimePublicOperationError::NotAccepting { .. })
        ));
        Ok(())
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod actual_effects_admission_tests {
    use super::*;

    async fn agent_at(profile: &std::path::Path) -> crate::core::AgentResult<crate::AuraAgent> {
        let authority = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"work10-actual-effects-admission-runtime",
        ));
        let context = EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-actual-effects-admission-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let config = crate::core::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy(aura_core::hash::hash(
                b"work10-actual-effects-admission-physical-device",
            )),
            storage: crate::core::config::StorageConfig {
                base_path: profile.to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        crate::core::AgentBuilder::new()
            .with_authority(authority)
            .with_config(config)
            .build_testing_async(&context)
            .await
    }

    async fn stop_tasks(agent: &crate::AuraAgent) -> crate::core::AgentResult<()> {
        agent
            .runtime()
            .tasks()
            .shutdown_with_timeout(Duration::from_secs(5))
            .await
            .map_err(|source| {
                aura_core::AuraError::Internal {
                    message: "drain actual regression runtime tasks".into(),
                    source: Some(Arc::new(source)),
                }
                .into()
            })
    }

    #[tokio::test]
    async fn actual_runtime_effects_lease_blocks_operation_drain_and_closes_stale_admission(
    ) -> crate::core::AgentResult<()> {
        let profile = tempfile::tempdir().map_err(aura_core::AuraError::from)?;
        let agent = Box::pin(agent_at(profile.path())).await?;
        let gate = agent.runtime().activity_gate();
        let operation = agent.runtime().admit_effects_operation()?;
        assert!(Arc::ptr_eq(
            &gate,
            &agent.runtime().effects().public_operation_activity()
        ));
        assert_eq!(gate.begin_shutdown(), RuntimeActivityState::Running);
        operation.require_runtime(agent.runtime())?;
        let drain = gate.wait_for_operations();
        tokio::pin!(drain);
        assert!(futures::poll!(drain.as_mut()).is_pending());
        let denial = match agent.runtime().admit_effects_operation() {
            Err(source) => source,
            Ok(_) => panic!("closed actual runtime admitted a new effect operation"),
        };
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&denial);
        let mut found = false;
        while let Some(current) = cause {
            if let Some(RuntimePublicOperationError::NotAccepting { state }) =
                current.downcast_ref::<RuntimePublicOperationError>()
            {
                assert_eq!(*state, RuntimeActivityState::Stopping);
                found = true;
            }
            cause = current.source();
        }
        assert!(
            found,
            "actual native admission denial survives facade allocation"
        );
        drop(operation);
        drain.await;
        stop_tasks(&agent).await?;
        assert_eq!(
            gate.state(),
            RuntimeActivityState::Stopping,
            "operation drain does not publish full shutdown completion"
        );
        Ok(())
    }

    #[tokio::test]
    async fn equal_identity_distinct_profile_runtime_cannot_use_foreign_effects_lease(
    ) -> crate::core::AgentResult<()> {
        let first_profile = tempfile::tempdir().map_err(aura_core::AuraError::from)?;
        let other_profile = tempfile::tempdir().map_err(aura_core::AuraError::from)?;
        let first = Box::pin(agent_at(first_profile.path())).await?;
        let other = Box::pin(agent_at(other_profile.path())).await?;
        assert_eq!(first.authority_id(), other.authority_id());
        assert_eq!(first.runtime().device_id(), other.runtime().device_id());
        let operation = first.runtime().admit_effects_operation()?;
        let foreign = match operation.require_runtime(other.runtime()) {
            Err(source) => source,
            Ok(()) => panic!("equal identity values admitted a foreign physical runtime lease"),
        };
        let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&foreign);
        let mut found = false;
        while let Some(current) = cause {
            found |= matches!(
                current.downcast_ref::<RuntimePublicOperationError>(),
                Some(RuntimePublicOperationError::ForeignHandoff)
            );
            cause = current.source();
        }
        assert!(
            found,
            "actual foreign-runtime denial survives strongest lease validation"
        );

        assert_eq!(
            other.runtime().runtime_activity_state(),
            RuntimeActivityState::Running
        );
        drop(operation);
        stop_tasks(&first).await?;
        stop_tasks(&other).await?;
        Ok(())
    }
    #[tokio::test]
    async fn actual_shutdown_waits_for_original_effect_lease_before_scheduler_and_stopped_publication(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let profile = tempfile::tempdir()?;
        let agent = Box::pin(agent_at(profile.path())).await?;
        let effects = agent.runtime().effects();
        let gate = agent.runtime().activity_gate();
        let active = effects.admit_public_operation()?;
        let context = EffectContext::new(
            agent.authority_id(),
            aura_core::ContextId::new_from_entropy(aura_core::hash::hash(
                b"work10-original-actual-shutdown-context",
            )),
            aura_core::effects::ExecutionMode::Testing,
        );
        let shutdown = agent.shutdown(&context);
        tokio::pin!(shutdown);
        assert!(futures::poll!(shutdown.as_mut()).is_pending());
        assert_eq!(gate.state(), RuntimeActivityState::Stopping);
        assert!(matches!(
            effects.public_operation_activity().admit(),
            Err(RuntimePublicOperationError::NotAccepting {
                state: RuntimeActivityState::Stopping
            })
        ));
        active.require_gate(&gate)?;
        drop(active);
        shutdown.await?;
        assert_eq!(gate.state(), RuntimeActivityState::Stopped);
        assert_eq!(
            gate.activity.load(Ordering::SeqCst) & ACTIVITY_COUNT_MASK,
            0
        );
        Ok(())
    }
}

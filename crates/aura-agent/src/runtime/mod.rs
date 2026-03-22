//! Layer 6: Runtime Effect System Composition - Assembly & Lifecycle
//!
//! Effect system assembly and lifecycle management for authority-based runtime.
//! Coordinates effect registry, builder infrastructure, context management, and
//! handler composition for production/testing/simulation modes (per docs/103_effect_system.md).
//!
//! **Key Components**:
//! - **EffectSystemBuilder**: Compose handlers via builder pattern
//! - **EffectRegistry**: Map (EffectType, Operation) → Handler implementation
//! - **EffectContext**: Per-execution context (authority, epoch, budgets)
//! - **LifecycleManager**: Service startup/shutdown coordination
//!
//! **Execution Modes** (per docs/103_effect_system.md):
//! - **Production**: Real handlers (crypto, storage, network)
//! - **Testing**: Mock handlers with deterministic behavior
//! - **Simulation**: Deterministic handlers with scenario injection, time control

// Runtime builder and container
pub mod builder;

// Effect system registry
pub mod registry;

// Execution infrastructure
pub mod executor;
pub mod lifecycle;

// Subsystem extraction for AuraEffectSystem
pub mod subsystems;

// Context management
pub mod context;
pub mod contextual;

// Runtime services
pub mod consensus;
pub mod services;

// Effect system implementation
pub mod effects;

pub mod system;

// Shared in-memory transport wiring for simulations/demos
pub mod shared_transport;

// Simulation factory (feature-gated)
#[cfg(feature = "simulation")]
pub mod simulation_factory;

// Cross-cutting concerns
pub mod contracts;
pub mod diagnostics;
pub mod errors;
pub mod instrumentation;
pub mod reliability;
pub mod session_ingress;

// Choreography integration
pub mod choreography_adapter;
cfg_if::cfg_if! {
    if #[cfg(feature = "choreo-backend-telltale-vm")] {
        pub mod choreo_engine;
        pub mod effect_trace_capture;
        pub mod parity_policy;
        pub mod vm_effect_handler;
        pub mod vm_host_bridge;
        pub mod vm_hardening;
    }
}

// Runtime utilities
pub mod time_handler;

// Re-export main types for convenience
#[allow(unused_imports)] // Re-exported for public API
pub use crate::task_registry::{TaskGroup, TaskSupervisionError, TaskSupervisor};
pub use aura_core::OperationSessionId;
pub use builder::EffectSystemBuilder;
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use choreo_engine::{AuraChoreoEngine, AuraChoreoEngineError};
#[allow(unused_imports)] // Re-exported for public API
pub use choreography_adapter::{
    AuraHandlerAdapter, AuraProtocolAdapter, GuardConfig, MessageGuardRequirements,
};
pub use context::EffectContext;
pub use contracts::{AuraDelegationCoherence, AuraDelegationWitness, AuraLinkBoundary};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use contracts::{
    AuraRuntimeAdmissionEvidence, AuraRuntimeAdmissionEvidenceKind, AuraRuntimeEnvelopeAdmission,
};
pub use diagnostics::{
    RuntimeDiagnostic, RuntimeDiagnosticKind, RuntimeDiagnosticSeverity, RuntimeDiagnosticSink,
};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use effect_trace_capture::{
    AuraEffectTraceEncoding, AuraEffectTraceGranularity, EffectTraceBundle, EffectTraceCapture,
    EffectTraceCaptureError, EffectTraceCaptureOptions,
};
pub use effects::AuraEffectSystem;
pub use errors::RuntimeBoundaryError;
pub use instrumentation::{
    RuntimeReconfigurationEvent, RuntimeSessionEvent, RuntimeShutdownEvent, RuntimeVmEvent,
};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use parity_policy::{AuraEnvelopeParityError, AuraEnvelopeParityPolicy};
#[allow(unused_imports)] // Re-exported for public API
pub use session_ingress::{
    caller_session_owner_label, handle_owned_vm_round, open_owned_manifest_vm_session_admitted,
    OwnedVmSession, RuntimeSessionOwner, SessionIngressError,
};
pub use shared_transport::SharedTransport;
#[allow(unused_imports)] // Re-exported for public API
pub use system::{RuntimeActivityGate, RuntimeActivityState, RuntimePublicOperationError};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use vm_effect_handler::{AuraVmEffectEvent, AuraVmEffectHandler};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use vm_hardening::{
    apply_protocol_execution_policy, apply_scheduler_execution_policy, aura_flow_policy_predicate,
    aura_output_predicate_allow_list, build_envelope_diff_artifact_for_policy, build_vm_config,
    configured_guard_capacity, parse_communication_replay_mode, parse_determinism_mode,
    parse_effect_determinism_tier, policy_for_protocol, policy_for_ref,
    policy_requires_envelope_artifact, required_runtime_capabilities_for_policy,
    scheduler_control_input_for_image, scheduler_policy_for_input, scheduler_policy_ref,
    validate_determinism_profile, validate_envelope_artifact_for_policy,
    validate_protocol_execution_policy, validate_scheduler_execution_policy, vm_config_for_profile,
    AuraVmConcurrencyProfile, AuraVmDeterminismProfileError, AuraVmGuardLayer,
    AuraVmHardeningProfile, AuraVmParityProfile, AuraVmProtocolExecutionPolicy, AuraVmRuntimeMode,
    AuraVmRuntimeSelector, AuraVmSchedulerControlInput, AuraVmSchedulerEnvelopeClass,
    AuraVmSchedulerExecutionPolicy, AuraVmSchedulerSignals, AuraVmSchedulerSignalsProvider,
    AURA_OUTPUT_PREDICATE_CHOICE, AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE,
    AURA_OUTPUT_PREDICATE_GUARD_RELEASE, AURA_OUTPUT_PREDICATE_OBSERVABLE,
    AURA_OUTPUT_PREDICATE_STEP, AURA_OUTPUT_PREDICATE_TRANSPORT_RECV,
    AURA_OUTPUT_PREDICATE_TRANSPORT_SEND, AURA_VM_POLICY_CONSENSUS_FALLBACK,
    AURA_VM_POLICY_CONSENSUS_FAST_PATH, AURA_VM_POLICY_DKG_CEREMONY, AURA_VM_POLICY_PROD_DEFAULT,
    AURA_VM_POLICY_RECOVERY_GRANT, AURA_VM_POLICY_SYNC_ANTI_ENTROPY, AURA_VM_SCHED_PRIORITY_AGING,
    AURA_VM_SCHED_PROGRESS_AWARE, AURA_VM_SCHED_ROUND_ROBIN,
};
#[allow(unused_imports)] // Re-exported for public API
#[cfg(feature = "choreo-backend-telltale-vm")]
pub use vm_host_bridge::{AuraVmConcurrencyEnvelopeError, AuraVmSessionOpenError};

// Re-export JournalCoupler for choreography journal coupling
#[allow(unused_imports)] // Re-exported for public API
pub use aura_guards::guards::journal::{
    CouplingMetrics, JournalCoupler, JournalCouplerBuilder, JournalCouplingResult, JournalOperation,
};

// Subsystem re-exports (available for incremental adoption
pub use registry::{
    EffectOperation, EffectRegistry, EffectRegistryError, EffectRegistryExt, EffectType,
};
#[allow(unused_imports)]
pub use subsystems::choreography::{
    RuntimeChoreographySessionId, SessionOwnerCapability, SessionOwnerCapabilityScope,
};
#[allow(unused_imports)]
pub use subsystems::{ChoreographyState, CryptoSubsystem, JournalSubsystem, TransportSubsystem};

pub use executor::EffectExecutor;
pub use lifecycle::LifecycleManager;
#[allow(unused_imports)] // Re-exported for public API
pub use services::{
    AuthorityManager, AuthorityStatus, FlowBudgetManager, ReceiptManager, RuntimeService,
    RuntimeServiceContext, ServiceError, ServiceErrorKind, ServiceHealth, SyncManagerConfig,
    SyncManagerState, SyncServiceManager,
};
#[allow(unused_imports)]
pub use session_ingress::SessionStartFailureReason;

// Simulation factory re-export
#[cfg(feature = "simulation")]
pub use simulation_factory::EffectSystemFactory;

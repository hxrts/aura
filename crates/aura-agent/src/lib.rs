//! # Aura Agent - Layer 6: Runtime Composition
//!
//! This crate provides runtime composition and effect system assembly for authority-based
//! identity management in the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 6 runtime composition crate providing:
//! - Authority-first runtime assembly and lifecycle management
//! - Effect registry and builder infrastructure
//! - Context management for multi-threaded agent coordination
//! - Choreography adapter for protocol execution
//! - Production, testing, and simulation execution modes
//!
//! ## Architecture Constraints

#![allow(clippy::uninlined_format_args)] // Runtime code uses explicit format args for clarity
#![allow(clippy::redundant_clone)] // Some clones needed for async context propagation
#![allow(missing_docs)] // Runtime API documentation evolving with design
//!
//! This crate depends on:
//! - **Layer 1-5**: All lower layers (core, domain crates, effects, protocols, features)
//! - **MUST NOT**: Create new effect implementations (use aura-effects)
//! - **MUST NOT**: Implement multi-party coordination (use aura-protocol)
//! - **MUST NOT**: Be imported by Layer 1-5 crates (no circular dependencies)
//!
//! ## What Belongs Here
//!
//! - Effect registry and builder infrastructure
//! - Runtime system composition and lifecycle
//! - Authority context management and tracking
//! - Execution mode implementation (production, testing, simulation)
//! - Choreography protocol adapter for protocol execution
//! - Receipt and flow budget management
//! - Public API for agent creation and operation
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Effect composition rules (belong in aura-composition)
//! - Multi-party protocol logic (belong in aura-protocol)
//! - Feature protocol implementations (belong in Layer 5 crates)
//! - Testing harnesses and fixtures (belong in aura-testkit)
//!
#![warn(clippy::await_holding_lock)]
//! ## Design Principles
//!
//! - Authority-first design: all operations scoped to specific authorities
//! - Lazy composition: effects are assembled on-demand, not eagerly
//! - Stateless handlers: runtime delegates state to journals and contexts
//! - Mode-aware execution: production, testing, and simulation use same API
//! - Lifecycle management: resource cleanup and graceful shutdown
//! - Zero coupling to Layer 5: runtime is agnostic to specific protocols
//!
//! ## Key Components
//!
//! - **AgentBuilder**: Fluent API for composing agents with authority context
//! - **EffectRegistry**: Dynamic registry of available effect handlers
//! - **EffectSystemBuilder**: Assembly infrastructure for effect combinations
//! - **AuraAgent**: Public API for agent operations
//! - **RuntimeSystem**: Internal runtime coordination
#![allow(unexpected_cfgs)]
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_agent::{AgentBuilder, AuthorityId};
//!
//! // Production agent with authority-first design
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_production()
//!     .await?;
//!
//! // Testing agent
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_testing()?;
//! ```

#![allow(clippy::expect_used)]

#[cfg(not(feature = "choreo-backend-telltale-machine"))]
compile_error!(
    "Aura agent requires the Telltale choreography backend. \
     Enable feature `choreo-backend-telltale-machine`."
);

// Core modules (public API)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod core;

// Builder system for ergonomic runtime construction
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod builder;

// Runtime modules (internal)
#[cfg(feature = "choreo-backend-telltale-machine")]
mod runtime;
#[cfg(feature = "choreo-backend-telltale-machine")]
mod task_registry;
#[cfg(feature = "choreo-backend-telltale-machine")]
mod token_profiles;

// Handler modules (public for service access)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod handlers;
#[cfg(feature = "choreo-backend-telltale-machine")]
mod reconfiguration;

// Runtime-owned indexed journal utilities (stateful)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod database;

// Reactive programming infrastructure (public)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod reactive;

// RuntimeBridge implementation (for aura-app dependency inversion)
#[cfg(feature = "choreo-backend-telltale-machine")]
mod runtime_bridge;
#[cfg(test)]
mod testing;

// Journal fact registry helpers (public helper functions)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod fact_registry;
#[cfg(feature = "choreo-backend-telltale-machine")]
pub mod fact_types;

// Public API - authority-first design
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use core::{AgentBuilder, AgentConfig, AgentError, AgentResult, AuraAgent, AuthorityContext};

// Builder system exports
#[cfg(all(feature = "android", feature = "choreo-backend-telltale-machine"))]
pub use builder::AndroidPresetBuilder;
#[cfg(all(feature = "web", feature = "choreo-backend-telltale-machine"))]
pub use builder::WebPresetBuilder;
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use builder::{BuildError, CliPresetBuilder, CustomPresetBuilder};
#[cfg(all(feature = "ios", feature = "choreo-backend-telltale-machine"))]
pub use builder::{DataProtectionClass, IosPresetBuilder};

// Session management types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use handlers::{SessionHandle, SessionServiceApi, SessionStats};

// Authentication types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use handlers::{
    AuthChallenge, AuthMethod, AuthResponse, AuthResult, AuthServiceApi, AuthenticationStatus,
};

// Invitation types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use handlers::{
    Invitation, InvitationResult, InvitationServiceApi, InvitationStatus, InvitationType,
};

// Recovery types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use handlers::{
    GuardianApproval, RecoveryOperation, RecoveryRequest, RecoveryResult, RecoveryServiceApi,
    RecoveryState,
};

// Rendezvous types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use handlers::{ChannelResult, RendezvousHandler, RendezvousResult, RendezvousServiceApi};

// Runtime types for advanced usage
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::system::RuntimeShutdownError;
#[cfg(all(
    feature = "choreo-backend-telltale-machine",
    not(target_arch = "wasm32")
))]
pub use runtime::AuraHandlerAdapter;
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::{
    EffectContext, EffectExecutor, EffectOperation, EffectRegistry, EffectRegistryError,
    EffectRegistryExt, EffectSystemBuilder, EffectType, FlowBudgetManager, LifecycleManager,
    OperationSessionId, ReceiptManager, RuntimeChoreographySessionId, RuntimeService,
    RuntimeServiceContext, ServiceError, ServiceErrorKind, ServiceHealth, SharedTransport,
    TaskSupervisor,
};

// Protocol adapter for choreography execution (used by tests)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::choreo_engine::{AuraChoreoEngine, AuraChoreoEngineError};
#[cfg(all(
    feature = "choreo-backend-telltale-machine",
    not(target_arch = "wasm32")
))]
pub use runtime::choreography_adapter::{AuraProtocolAdapter, MessageRequest, ReceivedMessage};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::parity_policy::{AuraEnvelopeParityError, AuraEnvelopeParityPolicy};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::vm_effect_handler::{AuraVmEffectEvent, AuraVmEffectHandler};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::vm_hardening::{
    apply_protocol_execution_policy, apply_scheduler_execution_policy, aura_flow_policy_predicate,
    aura_output_predicate_allow_list, build_envelope_diff_artifact_for_policy, build_vm_config,
    configured_guard_capacity, parse_communication_replay_mode, parse_determinism_mode,
    parse_effect_determinism_tier, policy_for_protocol, policy_for_ref,
    policy_requires_envelope_artifact, required_runtime_capabilities_for_policy,
    scheduler_control_input_for_image, scheduler_control_input_for_protocol_machine_image,
    scheduler_policy_for_input, scheduler_policy_ref, validate_determinism_profile,
    validate_envelope_artifact_for_policy, validate_protocol_execution_policy,
    validate_scheduler_execution_policy, vm_config_for_profile, AuraVmDeterminismProfileError,
    AuraVmGuardLayer, AuraVmHardeningProfile, AuraVmParityProfile, AuraVmProtocolExecutionPolicy,
    AuraVmRuntimeMode, AuraVmRuntimeSelector, AuraVmSchedulerControlInput,
    AuraVmSchedulerEnvelopeClass, AuraVmSchedulerExecutionPolicy, AuraVmSchedulerSignals,
    AuraVmSchedulerSignalsProvider, AURA_OUTPUT_PREDICATE_CHOICE,
    AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE, AURA_OUTPUT_PREDICATE_GUARD_RELEASE,
    AURA_OUTPUT_PREDICATE_OBSERVABLE, AURA_OUTPUT_PREDICATE_STEP,
    AURA_OUTPUT_PREDICATE_TRANSPORT_RECV, AURA_OUTPUT_PREDICATE_TRANSPORT_SEND,
    AURA_VM_POLICY_CONSENSUS_FALLBACK, AURA_VM_POLICY_CONSENSUS_FAST_PATH,
    AURA_VM_POLICY_DKG_CEREMONY, AURA_VM_POLICY_PROD_DEFAULT, AURA_VM_POLICY_RECOVERY_GRANT,
    AURA_VM_POLICY_SYNC_ANTI_ENTROPY, AURA_VM_SCHED_PRIORITY_AGING, AURA_VM_SCHED_PROGRESS_AWARE,
    AURA_VM_SCHED_ROUND_ROBIN,
};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::{
    AuraEffectTraceEncoding, AuraEffectTraceGranularity, EffectTraceBundle, EffectTraceCapture,
    EffectTraceCaptureError, EffectTraceCaptureOptions,
};

// Sync service types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::{SyncManagerConfig, SyncManagerState, SyncServiceManager};

// Rendezvous service types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::{RendezvousManager, RendezvousManagerConfig};

// Social service types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::{
    AccountabilityWitness, AccountabilityWitnessKind, HoldBudgetSnapshot, HoldDepositOutcome,
    HoldGcOutcome, HoldLocalIndexEntry, HoldManager, HoldManagerConfig, HoldProjection,
    HoldRetrievalOutcome, HoldRetrievalStatus, HoldSelectionPlan, HoldSyncBatch,
    QueuedAccountabilityReply, QueuedSyncRetrieval, VerifiedServiceWitness, VerifierRole,
};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::{SocialManager, SocialManagerConfig, SocialManagerState};

// Threshold signing service types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use reconfiguration::{CoherenceStatus, ReconfigurationError, SessionFootprintClass};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::ThresholdSigningService;
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::services::{
    ReconfigurationManager, ReconfigurationManagerError, SessionDelegationOutcome,
    SessionDelegationTransfer,
};
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::SessionOwnerCapabilityScope;

// Re-export core types for convenience
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use aura_core::effects::ExecutionMode;

// Effect system types
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use runtime::AuraEffectSystem;

// Simulation factory (feature-gated)
#[cfg(all(feature = "simulation", feature = "choreo-backend-telltale-machine"))]
pub use runtime::EffectSystemFactory;

// Re-export core types for convenience (authority-first)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub use aura_core::types::identifiers::{AuthorityId, ContextId, SessionId};

#[cfg(feature = "choreo-backend-telltale-machine")]
pub use fact_registry::build_fact_registry;

/// Selected choreography backend label.
#[cfg(feature = "choreo-backend-telltale-machine")]
pub const CHOREO_BACKEND: &str = "telltale_machine";

/// Create a production agent (convenience function)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub async fn create_production_agent(
    ctx: &EffectContext,
    authority_id: AuthorityId,
) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_production(ctx)
        .await
}

/// Create a testing agent (convenience function)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub fn create_testing_agent(authority_id: AuthorityId) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing()
}

/// Create a simulation agent (convenience function)
#[cfg(feature = "choreo-backend-telltale-machine")]
pub fn create_simulation_agent(authority_id: AuthorityId, seed: u64) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_simulation(seed)
}

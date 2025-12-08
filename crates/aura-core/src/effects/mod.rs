//! Layer 1: Core Effect Trait Definitions
//!
//! Pure trait definitions for all side-effect operations in Aura.
//! This module defines **what** effects can be performed; handlers define **how**.
//! (per docs/106_effect_system_and_runtime.md)
//!
//! # Effect Classification
//!
//! Effects are organized into four categories based on where they should be implemented:
//!
//! ## Infrastructure Effects (Layer 3: `aura-effects`)
//! OS integration with no Aura-specific semantics:
//! - **Time**, **Crypto**, **Network**, **Storage**, **Random**, **Console**, **Transport**
//! - **Biometric**, **Bloom**, **Reliability**, **Secure**, **System**
//!
//! ## Application Effects (Layer 2-4: Domain crates)
//! Aura-specific business logic:
//! - **Journal** (`aura-journal`), **Authorization**, **Capability** (`aura-wot`)
//! - **Agent** (`aura-agent`), **Authority** (`aura-protocol`/`aura-relational`)
//! - **Guard** (`aura-protocol::guards`), **Ledger** (`aura-journal`/`aura-protocol`)
//!
//! ## Protocol Coordination Effects (Layer 4: `aura-protocol`)
//! Multi-party protocol coordination:
//! - **Choreographic** (`aura-protocol::choreography`)
//! - **Sync** (`aura-sync` or `aura-protocol`)
//!
//! ## Testing/Simulation Effects (Layer 8: Test infrastructure)
//! Testing and verification:
//! - **Chaos**, **Simulation**, **Testing** (`aura-simulator`, `aura-testkit`)
//! - **Quint** (`aura-quint`)
//!
//! ## Composite Effects (Layer 1: `aura-core`)
//! Convenience supertraits combining other effects (no handlers needed)
//!
//! **Execution Modes** (per ExecutionMode enum):
//! - **Testing**: Mock implementations, deterministic behavior
//! - **Production**: Real system operations
//! - **Simulation**: Deterministic with controllable effects (seed-driven)
//!
//! All effect-using code is parameterized by effect traits, enabling:
//! Deterministic testing, flexible handler composition, runtime mode switching (per docs/106_effect_system_and_runtime.md)

// Core effect trait definitions
pub mod agent;
pub mod amp;
pub mod authority;
pub mod authorization;
pub mod availability; // Data availability within replication units
pub mod biometric;
pub mod bloom;
pub mod capability;
pub mod choreographic; // Multi-party protocol coordination
pub mod console;
pub mod crypto;
pub mod flood; // Rendezvous flooding for discovery
pub mod flow; // Flow budget management
pub mod guard; // Pure guard evaluation with effect commands
pub mod guardian; // Guardian relational coordination
pub mod indexed; // Indexed journal lookups (B-tree, Bloom, Merkle)
pub mod intent; // Intent dispatch effects
pub mod journal;
pub mod leakage; // Privacy leakage tracking
pub mod ledger; // Event sourcing and audit trails
pub mod network;
pub mod query; // Datalog query effects (bridges Journal + Biscuit + Reactive)
pub mod random;
pub mod reactive; // FRP as algebraic effects
pub mod relay; // Relay selection for message forwarding
pub mod reliability;
pub mod secure;
pub mod storage;
pub mod supertraits;
pub mod sync; // Anti-entropy synchronization
pub mod system;
pub mod threshold; // Unified threshold signing
pub mod time;
pub mod transport;
pub mod tree; // Commitment tree operations

// Simulation/testing effect traits (feature-gated)
#[cfg(feature = "simulation")]
pub mod chaos;
#[cfg(feature = "simulation")]
pub mod quint;
#[cfg(feature = "simulation")]
pub mod simulation;
#[cfg(feature = "simulation")]
pub mod testing;

// Re-export core effect traits
pub use agent::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    ConfigError, ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig,
    DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
    SessionManagementEffects, SessionMessage, SessionRole, SessionStatus, SessionType,
};
pub use amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
pub use authority::{AuthorityEffects, AuthorityRelationalEffects, RelationalEffects};
pub use authorization::{
    AuthorizationDecision, AuthorizationEffects, AuthorizationError, BiscuitAuthorizationEffects,
};
pub use availability::{AvailabilityError, DataAvailability};
pub use biometric::{
    BiometricCapability, BiometricConfig, BiometricEffects, BiometricEnrollmentResult,
    BiometricError, BiometricSecurityLevel, BiometricStatistics, BiometricType,
    BiometricVerificationResult,
};
pub use bloom::{BloomConfig, BloomError, BloomFilter};
pub use capability::{
    CapabilityConfig, CapabilityEffects, CapabilityError, CapabilityStatistics,
    CapabilityTokenFormat, CapabilityTokenInfo, CapabilityTokenRequest,
    CapabilityVerificationResult, TokenStatus, VerificationLevel,
};
#[cfg(feature = "simulation")]
pub use chaos::{ByzantineType, ChaosEffects, ChaosError, CorruptionType, ResourceType};
pub use console::ConsoleEffects;
pub use crypto::{CryptoEffects, CryptoError};
pub use flood::{
    FloodAction, FloodBudget, FloodError, LayeredBudget, RendezvousFlooder, RendezvousPacket,
};
pub use flow::{FlowBudgetEffects, FlowHint};
pub use guardian::{GuardianAcceptInput, GuardianEffects, GuardianRequestInput};
pub use indexed::{FactId, IndexStats, IndexedFact, IndexedJournalEffects};
pub use intent::{
    AuthorizationLevel, IntentDispatchError, IntentEffects, IntentMetadata, SimpleIntentEffects,
};
pub use journal::JournalEffects;
pub use leakage::{
    LeakageBudget, LeakageChoreographyExt, LeakageEffects, LeakageEvent, ObserverClass,
};
#[allow(deprecated)]
// Migration utilities removed - middleware transition complete
pub use network::{NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
pub use query::{QueryEffects, QueryError, QuerySubscription};
#[cfg(feature = "simulation")]
pub use quint::{
    // Generative simulation types
    ActionDescriptor,
    ActionEffect,
    ActionResult,
    // Core verification types
    Counterexample,
    EvaluationResult,
    EvaluationStatistics,
    Property,
    PropertyEvaluator,
    PropertyId,
    PropertyKind,
    PropertySpec,
    QuintEvaluationEffects,
    QuintMappable,
    QuintSimulationEffects,
    QuintStateExtractable,
    QuintVerificationEffects,
    VerificationId,
    VerificationResult,
};
pub use random::RandomEffects;
pub use reactive::{
    ReactiveDeriveEffects, ReactiveEffects, ReactiveError, Signal, SignalId, SignalStream,
};
pub use relay::{RelayCandidate, RelayContext, RelayError, RelayRelationship, RelaySelector};
pub use reliability::{
    // Unified retry types
    BackoffStrategy,
    RateLimit,
    // Unified rate limiting types
    RateLimitConfig,
    RateLimitResult,
    RateLimiter,
    RateLimiterStatistics,
    ReliabilityEffects,
    ReliabilityError,
    RetryContext,
    RetryPolicy,
    RetryResult,
};
pub use secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageError, SecureStorageLocation,
};
#[cfg(feature = "simulation")]
pub use simulation::{
    ByzantineFault, CheckpointId, ComputationFault, ExportFormat, FaultInjectionConfig,
    FaultInjectionEffects, FaultType, NetworkFault, OperationStats, ScenarioId, ScenarioState,
    SimulationCheckpoint, SimulationControlEffects, SimulationEffects, SimulationMetrics,
    SimulationObservationEffects, SimulationScenario, SimulationTime, StorageFault, TimeFault,
};
pub use storage::{StorageEffects, StorageError, StorageLocation, StorageStats};
pub use supertraits::{
    AntiEntropyEffects, ChoreographyEffects, CrdtEffects, MinimalEffects, SigningEffects,
    SnapshotEffects, TreeEffects,
};
pub use system::{SystemEffects, SystemError};
#[cfg(feature = "simulation")]
pub use testing::{TestingEffects, TestingError};
pub use time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeComparison, TimeEffects,
    TimeError, TimeoutHandle, WakeCondition,
};
pub use transport::{
    TransportEffects, TransportEnvelope, TransportError, TransportReceipt, TransportStats,
};

// Re-export protocol coordination effect traits
pub use choreographic::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics,
};
pub use guard::{
    Decision, EffectCommand, EffectInterpreter, FlowBudgetView, GuardOutcome, GuardSnapshot,
    JournalEntry, MetadataView, SimulationEvent,
};
pub use ledger::{EffectApiEffects, EffectApiError, EffectApiEvent, EffectApiEventStream};
pub use sync::SyncMetrics;
pub use threshold::{PublicKeyPackage, ThresholdSigningEffects, ThresholdSigningError};
pub use tree::{Cut, Partial, ProposalId, Snapshot, TreeOperationEffects};

// Re-export unified error system
pub use crate::AuraError;

/// Execution mode controlling effect handler selection across all system layers
///
/// This enum controls which implementations of effect handlers are used throughout
/// the entire Aura system, from testing to production deployments.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum ExecutionMode {
    /// Testing mode: Mock implementations, deterministic behavior
    #[default]
    Testing,
    /// Production mode: Real implementations, actual system operations
    Production,
    /// Simulation mode: Deterministic implementations with controllable effects
    Simulation {
        /// Random seed for deterministic simulation
        seed: u64,
    },
}

impl ExecutionMode {
    /// Check if this mode uses deterministic effects
    pub fn is_deterministic(&self) -> bool {
        matches!(self, Self::Testing | Self::Simulation { .. })
    }

    /// Check if this mode uses real system operations
    pub fn is_production(&self) -> bool {
        matches!(self, Self::Production)
    }

    /// Get the seed for deterministic modes
    pub fn seed(&self) -> Option<u64> {
        match self {
            Self::Simulation { seed } => Some(*seed),
            _ => None,
        }
    }
}

/// Effect type enumeration for all effects in the Aura system
///
/// Categorizes all effects in the Aura system for efficient dispatch
/// and middleware composition.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum EffectType {
    /// Cryptographic operations (FROST, DKD, hashing, key derivation)
    Crypto,
    /// Network communication (send, receive, broadcast)
    Network,
    /// Persistent storage operations
    Storage,
    /// Time-related operations (current time, sleep)
    Time,
    /// Console and logging operations
    Console,
    /// Random number generation
    Random,
    /// Reactive state management (FRP signals)
    Reactive,
    /// Intent dispatch (user action processing)
    Intent,
    /// Effect API operations (transaction log, state)
    EffectApi,
    /// Journal operations (event log, snapshots)
    Journal,

    /// Tree operations (commitment tree, MLS)
    Tree,

    /// Choreographic protocol coordination
    Choreographic,

    /// System monitoring, logging, and configuration
    System,

    /// Device-local storage
    DeviceStorage,
    /// Device authentication and sessions
    Authentication,
    /// Configuration management
    Configuration,
    /// Session lifecycle management
    SessionManagement,

    /// Fault injection for testing
    FaultInjection,
    /// Time control for simulation
    TimeControl,
    /// State inspection for debugging
    StateInspection,
    /// Property checking for verification
    PropertyChecking,
    /// Chaos coordination for resilience testing
    ChaosCoordination,

    /// Datalog query execution and subscriptions
    Query,
}

impl std::fmt::Display for EffectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Crypto => write!(f, "crypto"),
            Self::Network => write!(f, "network"),
            Self::Storage => write!(f, "storage"),
            Self::Time => write!(f, "time"),
            Self::Console => write!(f, "console"),
            Self::Random => write!(f, "random"),
            Self::Reactive => write!(f, "reactive"),
            Self::Intent => write!(f, "intent"),
            Self::EffectApi => write!(f, "effect_api"),
            Self::Journal => write!(f, "journal"),
            Self::Tree => write!(f, "tree"),
            Self::Choreographic => write!(f, "choreographic"),
            Self::System => write!(f, "system"),
            Self::DeviceStorage => write!(f, "device_storage"),
            Self::Authentication => write!(f, "authentication"),
            Self::Configuration => write!(f, "configuration"),
            Self::SessionManagement => write!(f, "session_management"),
            Self::FaultInjection => write!(f, "fault_injection"),
            Self::TimeControl => write!(f, "time_control"),
            Self::StateInspection => write!(f, "state_inspection"),
            Self::PropertyChecking => write!(f, "property_checking"),
            Self::ChaosCoordination => write!(f, "chaos_coordination"),
            Self::Query => write!(f, "query"),
        }
    }
}

impl EffectType {
    /// Get all effect types
    pub fn all() -> Vec<Self> {
        vec![
            Self::Crypto,
            Self::Network,
            Self::Storage,
            Self::Time,
            Self::Console,
            Self::Random,
            Self::Reactive,
            Self::Intent,
            Self::EffectApi,
            Self::Journal,
            Self::Tree,
            Self::Choreographic,
            Self::System,
            Self::DeviceStorage,
            Self::Authentication,
            Self::Configuration,
            Self::SessionManagement,
            Self::FaultInjection,
            Self::TimeControl,
            Self::StateInspection,
            Self::PropertyChecking,
            Self::ChaosCoordination,
            Self::Query,
        ]
    }
}

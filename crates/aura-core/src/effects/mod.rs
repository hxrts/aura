//! Layer 1: Core Effect Trait Definitions
//!
//! Pure trait definitions for all side-effect operations in Aura.
//! This module defines **what** effects can be performed; handlers define **how**.
//! (per docs/106_effect_system_and_runtime.md)
//!
//! **Effect Classification**:
//! - **Infrastructure Effects**: Core runtime (Time, Crypto, Network, Storage, Random).
//!   Must implement in aura-effects (Layer 3); domain crates may not.
//! - **Application Effects**: Domain-specific (Journal, Authorization, Capability).
//!   Implemented in domain crates (aura-journal, aura-wot, aura-verify, etc.).
//! - **Composite Effects**: Convenience traits combining infrastructure/application effects.
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
pub mod authority;
pub mod authorization;
pub mod biometric;
pub mod bloom;
pub mod capability;
pub mod chaos;
pub mod choreographic; // Multi-party protocol coordination
pub mod console;
pub mod crypto;
pub mod event_sourcing; // Event sourcing and audit trails
pub mod flow; // Flow budget management
pub mod guard_effects; // Pure guard evaluation with effect commands
pub mod journal;
pub mod leakage; // Privacy leakage tracking
pub mod migration; // Empty module - migration complete
pub mod network;
pub mod quint;
pub mod random;
pub mod reliability;
pub mod secure;
pub mod simulation;
pub mod storage;
pub mod supertraits;
pub mod sync; // Anti-entropy synchronization
pub mod system;
pub mod testing;
pub mod time;
pub mod transport;
pub mod tree_operations; // Commitment tree operations

// Re-export core effect traits
pub use agent::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    ConfigError, ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig,
    DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
    SessionManagementEffects, SessionMessage, SessionRole, SessionStatus, SessionType,
};
pub use authority::{AuthorityEffects, AuthorityRelationalEffects, RelationalEffects};
pub use authorization::{
    AuthorizationDecision, AuthorizationEffects, AuthorizationError, BiscuitAuthorizationEffects,
};
pub use biometric::{
    BiometricCapability, BiometricConfig, BiometricEffects, BiometricEnrollmentResult,
    BiometricError, BiometricSecurityLevel, BiometricStatistics, BiometricType,
    BiometricVerificationResult,
};
pub use bloom::{BloomConfig, BloomEffects, BloomError, BloomFilter};
pub use capability::{
    CapabilityConfig, CapabilityEffects, CapabilityError, CapabilityStatistics,
    CapabilityTokenFormat, CapabilityTokenInfo, CapabilityTokenRequest,
    CapabilityVerificationResult, TokenStatus, VerificationLevel,
};
pub use chaos::{ByzantineType, ChaosEffects, ChaosError, CorruptionType, ResourceType};
pub use console::ConsoleEffects;
pub use crypto::{CryptoEffects, CryptoError};
pub use flow::{FlowBudgetEffects, FlowHint};
pub use journal::JournalEffects;
pub use leakage::{
    LeakageBudget, LeakageChoreographyExt, LeakageEffects, LeakageEvent, ObserverClass,
};
#[allow(deprecated)]
// Migration utilities removed - middleware transition complete
pub use network::{NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
pub use quint::{
    Counterexample, EvaluationResult, EvaluationStatistics, Property, PropertyEvaluator,
    PropertyId, PropertyKind, PropertySpec, QuintEvaluationEffects, QuintVerificationEffects,
    VerificationId, VerificationResult,
};
pub use random::RandomEffects;
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
pub use event_sourcing::{EffectApiEffects, EffectApiError, EffectApiEvent, EffectApiEventStream};
pub use guard_effects::{
    Decision, EffectCommand, EffectInterpreter, FlowBudgetView, GuardOutcome, GuardSnapshot,
    JournalEntry, MetadataView, SimulationEvent,
};
pub use sync::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
pub use tree_operations::{Cut, Partial, ProposalId, Snapshot, TreeOperationEffects};

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
        ]
    }
}

//! Unified Aura Handler Architecture
//!
//! This module defines the core traits and types for Aura's unified handler system.
//! All effect execution, session type interpretation, and middleware composition
//! flows through these interfaces.
//!
//! # Architecture Overview
//!
//! The unified handler architecture replaces the previous fragmented approach
//! with a single, elegant composition system:
//!
//! - **AuraHandler**: Core trait for all effect execution and session interpretation
//! - **AuraContext**: Unified context flowing through all operations
//! - **Effect**: Type-safe wrapper for all effect operations
//! - **EffectType**: Classification system for effect dispatch
//! - **ExecutionMode**: Environment control (Testing, Production, Simulation)
//!
//! # Design Principles
//!
//! - **Algebraic Composition**: Preserve free algebra properties
//! - **Middleware-First**: All handlers are middleware in a composable stack
//! - **Unified Context**: Single context object flows through all layers
//! - **Zero Legacy**: Clean replacement with no backwards compatibility
//!
//! # Naming Conventions
//!
//! This module follows algebraic effects theory naming conventions:
//!
//! ## Effects vs Handlers
//!
//! - **Effect**: A declaration of capabilities or operations that a computation may perform,
//!   without specifying how those operations are implemented. Effects are abstract interfaces
//!   that represent what can be done.
//!   - Examples: `CryptoEffects`, `NetworkEffects`, `StorageEffects`
//!   - Naming: `{Domain}Effects` trait
//!
//! - **Handler**: A concrete implementation that interprets effects by providing the actual
//!   behavior for each effect operation. Handlers define how effects are executed in a
//!   specific context.
//!   - Examples: `MockCryptoHandler`, `RealCryptoHandler`, `TcpNetworkHandler`
//!   - Naming: `{Adjective}{Domain}Handler` struct
//!
//! ## Coordination Patterns
//!
//! - **Coordinator**: Orchestrates multiple handlers or manages multi-party operations
//!   - Examples: `CrdtCoordinator`, `StorageCoordinator`
//!   - Naming: `{Domain}Coordinator`
//!
//! - **Adapter**: Bridges between different interfaces or protocols
//!   - Examples: `AuraHandlerAdapter`, `ChoreographicAdapter`
//!   - Naming: `{Source}{Target}Adapter`
//!
//! - **Bridge**: Connects different subsystems or layers
//!   - Examples: `UnifiedAuraHandlerBridge`, `TypedBridge`
//!   - Naming: `{System}Bridge` or `{Adjective}Bridge`
//!
//! This distinction ensures:
//! 1. Effects are purely declarative - they specify the interface without implementation
//! 2. Handlers are interpretive - they provide the concrete semantics
//! 3. The same effect can have multiple handlers (mock vs real, different backends)
//! 4. Handlers can be composed, chained, or swapped without changing effect declarations

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

// Re-export ExecutionMode from aura_core to avoid duplication
pub use aura_core::effects::ExecutionMode;

// Legacy module declarations removed - now organized under new structure


/// Effect type classification for dispatch and middleware routing
///
/// Categorizes all effects in the Aura system for efficient dispatch
/// and middleware composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Ledger operations (transaction log, state)
    Ledger,
    /// Journal operations (event log, snapshots)
    Journal,

    /// Tree operations (ratchet tree, MLS)
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

impl fmt::Display for EffectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crypto => write!(f, "crypto"),
            Self::Network => write!(f, "network"),
            Self::Storage => write!(f, "storage"),
            Self::Time => write!(f, "time"),
            Self::Console => write!(f, "console"),
            Self::Random => write!(f, "random"),
            Self::Ledger => write!(f, "ledger"),
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

/// Error type for Aura handler operations
#[derive(Debug, Error)]
pub enum AuraHandlerError {
    /// Effect not supported by this handler
    #[error("Effect {effect_type:?} not supported")]
    UnsupportedEffect {
        /// Type of effect that is not supported
        effect_type: EffectType,
    },

    /// Operation not found within effect type
    #[error("Operation '{operation}' not found in effect {effect_type:?}")]
    UnknownOperation {
        /// Type of effect being queried
        effect_type: EffectType,
        /// Name of the operation requested
        operation: String,
    },

    /// Effect parameter serialization failed
    #[error("Failed to serialize parameters for {effect_type:?}.{operation}")]
    EffectSerialization {
        /// Type of effect
        effect_type: EffectType,
        /// Name of the operation
        operation: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Effect parameter deserialization failed
    #[error("Failed to deserialize parameters for {effect_type:?}.{operation}")]
    EffectDeserialization {
        /// Type of effect
        effect_type: EffectType,
        /// Name of the operation
        operation: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Session type execution failed
    #[error("Session type execution failed")]
    SessionExecution {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Context operation failed
    #[error("Context operation failed: {message}")]
    ContextError {
        /// Error message
        message: String,
    },

    /// Middleware operation failed
    #[error("Middleware operation failed")]
    MiddlewareError {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Registry operation failed
    #[error("Registry operation failed")]
    RegistryError {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Operation not supported by effect type
    #[error("Operation '{operation}' not supported by effect {effect_type:?}")]
    UnsupportedOperation {
        /// Type of effect
        effect_type: EffectType,
        /// Name of the operation
        operation: String,
    },

    /// Parameter deserialization failed
    #[error("Failed to deserialize parameters")]
    ParameterDeserializationFailed {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Effect execution failed
    #[error("Effect execution failed")]
    ExecutionFailed {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Authorization failed
    #[error("Authorization failed")]
    AuthorizationFailed {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Handler creation failed
    #[error("Handler creation failed")]
    HandlerCreationFailed {
        /// Underlying error from handler creation
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl AuraHandlerError {
    /// Create a context error
    pub fn context_error(message: impl Into<String>) -> Self {
        Self::ContextError {
            message: message.into(),
        }
    }

    /// Wrap another error as a middleware error
    pub fn middleware_error(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::MiddlewareError {
            source: Box::new(source),
        }
    }

    /// Wrap another error as a registry error
    pub fn registry_error(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::RegistryError {
            source: Box::new(source),
        }
    }

    /// Wrap another error as a session execution error
    pub fn session_error(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::SessionExecution {
            source: Box::new(source),
        }
    }
}

// The primary AuraHandler trait is now defined in the erased module
// to avoid duplication and ensure all handlers use the unified interface.

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
            Self::Ledger,
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

    /// Check if this is a core protocol effect
    pub fn is_protocol_effect(&self) -> bool {
        matches!(
            self,
            Self::Crypto
                | Self::Network
                | Self::Storage
                | Self::Time
                | Self::Console
                | Self::Random
                | Self::Ledger
                | Self::Journal
                | Self::Choreographic
                | Self::System
        )
    }

    /// Check if this is an agent effect
    pub fn is_agent_effect(&self) -> bool {
        matches!(
            self,
            Self::DeviceStorage
                | Self::Authentication
                | Self::Configuration
                | Self::SessionManagement
        )
    }

    /// Check if this is a simulation effect
    pub fn is_simulation_effect(&self) -> bool {
        matches!(
            self,
            Self::FaultInjection
                | Self::TimeControl
                | Self::StateInspection
                | Self::PropertyChecking
                | Self::ChaosCoordination
        )
    }
}

// Re-export types from submodules (selective to avoid ambiguous re-exports)

// Core handler infrastructure
pub mod core;
pub use core::{
    CompositeHandler, AuraHandler, BoxedHandler, HandlerUtils,
    AuraHandlerBuilder, AuraHandlerConfig, AuraHandlerFactory, FactoryError,
    EffectRegistry, RegistrableHandler, RegistryError,
};

// Context management
pub mod context;
pub use context::{
    AgentContext, AuraContext, ChoreographicContext, PlatformInfo,
    SimulationContext, TracingContext, MetricsContext,
};

// Bridge adapters  
pub mod bridges;
pub use bridges::{TypedHandlerBridge, UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};

// Memory-based handlers
pub mod memory;
pub use memory::{
    MemoryChoreographicHandler, GuardianAuthorizationHandler, MemoryLedgerHandler,
};

// Convert AuraHandlerError to AuraError for ? operator
impl From<AuraHandlerError> for aura_core::AuraError {
    fn from(err: AuraHandlerError) -> Self {
        aura_core::AuraError::internal(format!("Handler error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_mode_properties() {
        let testing = ExecutionMode::Testing;
        assert!(testing.is_deterministic());
        assert!(!testing.is_production());
        assert_eq!(testing.seed(), None);

        let production = ExecutionMode::Production;
        assert!(!production.is_deterministic());
        assert!(production.is_production());
        assert_eq!(production.seed(), None);

        let simulation = ExecutionMode::Simulation { seed: 42 };
        assert!(simulation.is_deterministic());
        assert!(!simulation.is_production());
        assert_eq!(simulation.seed(), Some(42));
    }

    #[test]
    fn test_effect_type_classification() {
        assert!(EffectType::Crypto.is_protocol_effect());
        assert!(!EffectType::Crypto.is_agent_effect());
        assert!(!EffectType::Crypto.is_simulation_effect());

        assert!(!EffectType::Authentication.is_protocol_effect());
        assert!(EffectType::Authentication.is_agent_effect());
        assert!(!EffectType::Authentication.is_simulation_effect());

        assert!(!EffectType::FaultInjection.is_protocol_effect());
        assert!(!EffectType::FaultInjection.is_agent_effect());
        assert!(EffectType::FaultInjection.is_simulation_effect());
    }

    #[test]
    fn test_effect_serialization() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct TestParams {
            value: u32,
        }

        let params = TestParams { value: 42 };
        let serialized = bincode::serialize(&params).unwrap();
        let deserialized: TestParams = bincode::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.value, 42);
    }

    #[test]
    fn test_effect_type_all() {
        let all_effects = EffectType::all();
        assert!(all_effects.len() >= 18); // We have at least 18 effect types
        assert!(all_effects.contains(&EffectType::Crypto));
        assert!(all_effects.contains(&EffectType::Choreographic));
        assert!(all_effects.contains(&EffectType::FaultInjection));
    }
}

// Remaining handler modules  
pub mod agent;
pub mod storage;
pub mod system;
pub mod mock;

// Flattened handlers (previously in subdirectories)
pub mod sync_anti_entropy;
pub use sync_anti_entropy::AntiEntropyHandler;
pub mod sync_broadcaster;
pub use sync_broadcaster::{BroadcastConfig, BroadcasterHandler};

pub mod time_enhanced;
pub use time_enhanced::EnhancedTimeHandler;

pub mod timeout_coordinator;
pub use timeout_coordinator::TimeoutCoordinator;

pub mod transport_coordinator;

pub use transport_coordinator::{
    CoordinationResult, RetryingTransportManager, TransportCoordinator,
    TransportCoordinationConfig, TransportCoordinationError,
};

// External re-exports
// REMOVED: Users should import MockJournalHandler directly from aura-effects
// pub use aura_effects::journal::MockJournalHandler;
pub use mock::MockHandler;

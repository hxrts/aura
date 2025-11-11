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

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

pub mod composite;
pub mod context;
pub mod erased;
pub mod factory;
pub mod registry;
pub mod typed_bridge;
pub mod unified_bridge;

/// Execution mode for Aura handlers
///
/// Controls the environment and implementation strategy for effect execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutionMode {
    /// Testing mode: Mock implementations, deterministic behavior
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

pub use composite::CompositeHandler;
pub use context::{
    AgentContext, AuraContext, ChoreographicContext, MiddlewareContext, PlatformInfo,
    SimulationContext,
};
pub use erased::{AuraHandler, BoxedHandler, HandlerUtils};
pub use factory::{AuraHandlerBuilder, AuraHandlerConfig, AuraHandlerFactory, FactoryError};
// Unified CompositeHandler replaces old MiddlewareStack
pub use registry::{EffectRegistry, RegistrableHandler, RegistryError};
pub use unified_bridge::{UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};

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

// Additional handler modules (others already declared above)
pub mod agent;
pub mod choreographic;
pub mod journal;
pub mod ledger;
pub mod sync;
pub mod system;
pub mod time;
pub mod tree;

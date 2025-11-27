//! Layer 4: Unified Handler Architecture - Effects-Handler Bridge
//!
//! Bridges effect trait definitions (Layer 1 abstract interfaces) with concrete handler
//! implementations (Layer 3/4 semantics). Handlers implement effect traits and compose
//! via guard chains to enforce authorization, privacy, and budget invariants.
//!
//! **Core Design Pattern** (per docs/001_system_architecture.md, docs/106_effect_system_and_runtime.md):
//! - **Effects** (Layer 1): Abstract trait interfaces; specify **what** operations are available
//! - **Handlers** (Layer 3/4): Concrete implementations; define **how** operations execute
//! - **Composition** (Layer 4): Guards and middleware wrap handlers for enforcement
//! - **Execution**: Effect-using code parameterized by effect traits; handlers injected at runtime
//!
//! **Benefits**:
//! - Multiple implementations (production, mock, simulation) without changing effect declarations
//! - Handler composition and middleware stacking (guard chain: CapGuard → FlowGuard → JournalCoupler)
//! - Testing with deterministic implementations (aura-simulator)
//! - Runtime mode switching (ExecutionMode::Testing/Production/Simulation)
//!
//! **Handler Categories** (organized by concern):
//! - **core**: Base effect handler traits, registry infrastructure, error types
//! - **tree**: Commitment tree reduction and application handlers
//! - **memory**: In-memory handlers for testing and simulation
//! - **bridges**: Integration adapters connecting effect systems
//! - **storage**: Storage coordination handlers
//! - **context**: Context/lifecycle management for handler operations
//!
//! **Handler Categories**:
//! - **core**: Base effect handler traits and registry
//! - **tree**: Commitment tree operations
//! - **memory**: In-memory implementations for testing
//! - **agent**: Device auth, session management
//! - **bridges**: Adapters for integration
//! - **storage**: Storage coordination
//! - **context**: Context management for handler operations
//!
//! **Guard Chain Integration** (docs/003_information_flow_contract.md):
//! Every message flows: CapGuard → FlowGuard → JournalCoupler → LeakageTracker → Transport
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

use thiserror::Error;

// Re-export types from aura_core to avoid duplication
pub use aura_core::effects::{EffectType, ExecutionMode};

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

// EffectType impl block is defined in aura-core to avoid orphan rule violations

// Re-export types from submodules (selective to avoid ambiguous re-exports)

// Core handler infrastructure (type erasure only - composition moved to aura-composition)
pub mod core;
pub use core::{AuraHandler, BoxedHandler, HandlerUtils};

// Re-export composition infrastructure from aura-composition
pub use aura_composition::{CompositeHandler, EffectRegistry, RegistrableHandler};

// Context management
pub mod context;
pub mod context_immutable;
pub use context::{
    AgentContext, AuraContext, ChoreographicContext, MetricsContext, PlatformInfo,
    SimulationContext, TracingContext,
};

// Bridge adapters
pub mod bridges;
pub use bridges::{TypedHandlerBridge, UnifiedAuraHandlerBridge, UnifiedHandlerBridgeFactory};

// Memory-based handlers
pub mod memory;
pub use memory::{
    /* GuardianAuthorizationHandler, */ MemoryChoreographicHandler, MemoryLedgerHandler,
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
// NOTE: agent module temporarily disabled - uses AuraEffectSystem from aura-agent (Layer 6)
// aura-protocol (Layer 4) should not depend on aura-agent types
// TODO: Refactor agent handlers to use only aura-core effect traits
// pub mod agent;
pub mod storage;
// REMOVED: pub mod system; // Moved to aura-effects (Layer 3) - basic handlers
pub mod mock;
pub mod tree;

// Flattened handlers (previously in subdirectories)
pub mod sync_anti_entropy;
pub use sync_anti_entropy::AntiEntropyHandler;
pub mod sync_broadcaster;
pub use sync_broadcaster::{BroadcastConfig, BroadcasterHandler};

pub use crate::coordinators::time_enhanced::EnhancedTimeHandler;

pub mod timeout_coordinator;
pub use timeout_coordinator::TimeoutCoordinator;

pub mod transport_coordinator;

pub use transport_coordinator::{
    CoordinationResult, RetryingTransportManager, TransportCoordinationConfig,
    TransportCoordinationError, TransportCoordinator,
};

// External re-exports
// REMOVED: Users should import MockJournalHandler directly from aura-effects
// pub use aura_effects::journal::MockJournalHandler;
pub use mock::MockHandler;

//! Aura Simulator Library
//!
//! This crate implements simulation functionality through composable middleware layers that handle:
//! - Scenario injection and dynamic test modifications
//! - Fault simulation and error injection
//! - Time control and temporal operations
//! - State inspection and monitoring
//! - Property checking and validation
//! - Chaos coordination and complex scenarios
//!
//! All simulation functionality is implemented as middleware components
//! following Aura's foundation pattern for algebraic effect composition.
//!
//! # Middleware Architecture
//!
//! The simulator uses a composable middleware stack where each layer adds specific
//! simulation capabilities:
//!
//! ```rust,ignore
//! use aura_simulator::{
//!     SimulatorStackBuilder,
//!     ScenarioInjectionMiddleware,
//!     FaultSimulationMiddleware,
//!     TimeControlMiddleware,
//!     StateInspectionMiddleware,
//!     PropertyCheckingMiddleware,
//!     ChaosCoordinationMiddleware,
//!     CoreSimulatorHandler,
//! };
//!
//! let stack = SimulatorStackBuilder::new()
//!     .with_middleware(std::sync::Arc::new(ScenarioInjectionMiddleware::new()))
//!     .with_middleware(std::sync::Arc::new(FaultSimulationMiddleware::new()))
//!     .with_middleware(std::sync::Arc::new(TimeControlMiddleware::new()))
//!     .with_middleware(std::sync::Arc::new(StateInspectionMiddleware::new()))
//!     .with_middleware(std::sync::Arc::new(PropertyCheckingMiddleware::new()))
//!     .with_middleware(std::sync::Arc::new(ChaosCoordinationMiddleware::new()))
//!     .with_handler(std::sync::Arc::new(CoreSimulatorHandler::new()))
//!     .build()?;
//! ```
//!
//! # Example Usage
//!
//! ```rust,ignore
//! use aura_simulator::*;
//! use std::time::Duration;
//!
//! // Create simulation context
//! let context = SimulatorContext::new("test_scenario".to_string(), "run_1".to_string())
//!     .with_participants(5, 3)
//!     .with_seed(42);
//!
//! // Execute simulation operations
//! let result = stack.process(
//!     SimulatorOperation::ExecuteTick {
//!         tick_number: 1,
//!         delta_time: Duration::from_millis(100),
//!     },
//!     &context,
//! )?;
//! ```

#![allow(missing_docs)]

// Core middleware system
pub mod middleware;

// Quint integration for formal verification
pub mod quint;

// Utility functions
pub mod utils;

// Scenario definitions
pub mod scenario;

// Re-export core middleware types for external usage
pub use middleware::{
    ByzantineStrategy, ChaosCoordinationMiddleware, ChaosStrategy, FaultSimulationMiddleware,
    FaultType, LogLevel, NetworkConfig, PropertyCheckingMiddleware, PropertyViolationType, Result,
    ScenarioInjectionMiddleware, SimulationOutcome, SimulatorConfig, SimulatorContext,
    SimulatorError, SimulatorHandler, SimulatorMiddleware, SimulatorMiddlewareStack,
    SimulatorOperation, SimulatorStackBuilder, StateInspectionMiddleware, StateQuery, TimeConfig,
    TimeControlAction, TimeControlMiddleware,
};

// Re-export handler implementations
pub use middleware::handler::{CoreSimulatorHandler, NoOpSimulatorHandler};

// Re-export specific middleware types for convenience
pub use middleware::scenario_injection::{InjectionAction, ScenarioDefinition, TriggerCondition};

pub use middleware::fault_simulation::{FaultCondition, FaultInjectionRule, FaultRecoverySettings};

pub use middleware::time_control::RealtimeSync;

pub use middleware::state_inspection::{
    StateTrigger, StateWatcher, TriggerAction, WatcherCondition,
};

pub use middleware::property_checking::{
    PropertyCheckResult, PropertyChecker, PropertyType, PropertyViolation,
};

pub use middleware::chaos_coordination::{
    ChaosAction, ChaosRecoverySettings, ChaosRule, ChaosRuleAction, ChaosRuleCondition,
    ChaosRuleOperator, ChaosStrategyTemplate,
};

// Re-export Duration for convenience
pub use std::time::Duration;

#![allow(missing_docs)]

//! Aura Simulator Library
//!
//! This crate implements simulation functionality through effect system composition:
//! - Scenario injection and dynamic test modifications
//! - Fault simulation and error injection  
//! - Time control and temporal operations
//! - Deterministic simulation environments
//!
//! All simulation functionality is implemented as effect handlers
//! following Aura's unified effect system architecture.
//!
//! # Effect System Architecture  
//!
//! The simulator uses composable effect handlers where each handler provides specific
//! simulation capabilities through the unified effect system:
//!
//! ```rust,ignore
//! use aura_simulator::handlers::SimulationTimeHandler;
//! use aura_core::effects::TimeEffects;
//!
//! // Create simulation-specific effect handlers
//! let time_handler = SimulationTimeHandler::new();
//! 
//! // Use handlers through effect system instead of middleware
//! let timestamp = time_handler.current_timestamp().await?;
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
//! // Execute simulation through effect composition
//! let environment = SimulationEffectComposer::for_testing(device_id)?;
//! let timestamp = environment.current_timestamp().await?;
//! ```
#![allow(clippy::disallowed_methods)]

// Core middleware system
pub mod middleware;

// Simulation effect system
pub mod effects;

// Simulation handlers
pub mod handlers;

// Privacy analysis and observer models
pub mod privacy;

// Quint integration for formal verification
pub mod quint;

// Utility functions
pub mod utils;

// Simulation context
pub mod context;

// Scenario definitions
pub mod scenario;

// Testkit integration bridge
pub mod testkit_bridge;

// Re-export core types for external usage
pub use middleware::{
    ByzantineStrategy, FaultType, LogLevel, NetworkConfig, PerformanceMetrics,
    PropertyViolationType, Result, SimulationOutcome, SimulatorConfig,
    SimulatorContext, SimulatorError, SimulatorHandler, SimulatorOperation,
    StateQuery, StatelessEffectsMiddleware, TimeConfig, TimeControlAction,
};

// Re-export testkit bridge
pub use testkit_bridge::{MiddlewareConfig, TestkitSimulatorBridge};

// Re-export handler implementations
pub use middleware::handler::CoreSimulatorHandler;
pub use handlers::{
    SimulationEffectComposer, ComposedSimulationEnvironment,
    SimulationTimeHandler, SimulationFaultHandler, SimulationScenarioHandler
};

// Re-export scenario types for convenience
pub use handlers::{ScenarioDefinition, InjectionAction, TriggerCondition};

// Re-export Duration for convenience
pub use std::time::Duration;

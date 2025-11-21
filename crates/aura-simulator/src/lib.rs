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
//! use aura_simulator::handlers::SimulationEffectComposer;
//! use aura_protocol::standard_patterns::EffectRegistry;
//! use std::time::Duration;
//!
//! // NEW: Create simulation environment using EffectRegistry + composer pattern
//! let environment = SimulationEffectComposer::for_testing(device_id)?;
//!
//! // Or customize simulation effects
//! let effects = EffectRegistry::simulation(42)
//!     .with_device_id(device_id)
//!     .with_logging()
//!     .build()?;
//!
//! // Execute simulation through effect composition
//! let timestamp = environment.time_handler.as_ref().unwrap().current_timestamp().await?;
//! ```
#![allow(clippy::disallowed_methods)]

// Core simulator types following algebraic effects architecture
pub mod types;

// Simulation effect system
pub mod effects;

// Simulation handlers
pub mod handlers;

// Compatibility module for legacy handlers
pub mod compat;

// AMP scenario helpers
pub mod amp;

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
pub use types::{
    ByzantineStrategy, ChaosStrategy, FaultType, LogLevel, NetworkConfig,
    PropertyViolationType, Result, SimulationOutcome, SimulatorConfig, SimulatorContext,
    SimulatorError, SimulatorOperation, StateQuery, TimeConfig, TimeControlAction,
};

// Re-export effect handlers (pure algebraic effects)
pub use handlers::{
    ComposedSimulationEnvironment, SimulationEffectComposer, SimulationFaultHandler,
    SimulationScenarioHandler, SimulationTimeHandler, StatelessSimulatorHandler,
    CoreSimulatorHandler,
};

// Re-export testkit bridge 
pub use testkit_bridge::{MiddlewareConfig as HandlerConfig, TestkitSimulatorBridge};

// Legacy compatibility re-exports (deprecated - use pure effect handlers instead)
pub use compat::{PerformanceMetrics, SimulatorHandler};

// Re-export scenario types for convenience
pub use handlers::{InjectionAction, ScenarioDefinition, TriggerCondition};

// Re-export Duration for convenience
pub use std::time::Duration;

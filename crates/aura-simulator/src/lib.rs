#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)] // Simulator uses expect() for internal invariants
#![allow(clippy::unwrap_used)] // Simulator uses unwrap() for test code
#![allow(deprecated)] // Simulator intentionally uses deprecated time functions for testing purposes

//! # Aura Simulator - Layer 6: Runtime Composition
//!
//! This crate implements deterministic simulation runtime composition through effect system
//! architecture for testing and protocol verification in the Aura platform.
//!
//! ## Purpose
//!
//! Layer 6 runtime composition crate providing:
//! - Deterministic simulation environment for protocol testing
//! - Effect handlers for simulation-specific capabilities (time, faults, scenarios)
//! - Time control and temporal operations without real delays
//! - Fault injection and Byzantine strategy simulation
//! - Scenario composition and dynamic test modifications
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1-5**: All lower layers (core, domain crates, effects, protocols, features)
//! - **MUST NOT**: Create new persistent effect handlers (use aura-effects)
//! - **MUST NOT**: Implement multi-party coordination (use aura-protocol)
//! - **MUST NOT**: Be imported by Layer 1-5 crates (no circular dependencies)
//!
//! ## What Belongs Here
//!
//! - Simulation effect handlers (time, fault injection, scenario injection)
//! - Deterministic execution environment composition
//! - Effect composer for combining simulation capabilities
//! - Time control and temporal simulation utilities
//! - Fault injection strategies (Byzantine, chaos, network faults)
//! - Scenario definition and injection infrastructure
//! - Privacy analysis and observer model implementations
//! - Testkit integration bridge
//!
//! ## What Does NOT Belong Here
//!
//! - Production effect implementations (belong in aura-effects)
//! - Effect composition infrastructure (belong in aura-composition)
//! - Multi-party protocol logic (belong in aura-protocol)
//! - Feature protocol implementations (belong in Layer 5 crates)
//! - Agent runtime composition (belong in aura-agent)
//! - Concrete test cases and fixtures (belong in aura-testkit)
//!
//! ## Design Principles
//!
//! - Simulation is deterministic: same seed produces identical execution
//! - Time is controlled: simulated time advances without real delays
//! - Faults are injected: Byzantine, chaos, and network strategies composable
//! - Effect-based: all simulation capabilities via effect system, no globals
//! - Scenario injection: tests can modify behavior without code changes
//! - Privacy analysis: formal verification of information flow properties
//! - Composable: simulation handlers combine like production effects
//!
//! ## Key Components
//!
//! - **SimulationEffectComposer**: Assembly of simulation effect environment
//! - **SimulationTimeHandler**: Time control without real delays
//! - **SimulationFaultHandler**: Fault injection and Byzantine strategies
//! - **SimulationScenarioHandler**: Dynamic scenario injection
//! - **SimulatorConfig**: Configuration for simulation parameters
//!
//! ## Effect System Architecture
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
//! ## Example Usage
//!
//! ```rust,ignore
//! use aura_simulator::handlers::SimulationEffectComposer;
//! use aura_agent::EffectRegistry;
//! use std::time::Duration;
//!
//! // Create simulation environment using EffectRegistry + composer pattern
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
/// Simulation scenarios for testing consensus and protocols
pub mod scenarios;

// Testkit integration bridge
pub mod testkit_bridge;

// Re-export core types for external usage
pub use types::{
    ByzantineStrategy, ChaosStrategy, FaultType, LogLevel, NetworkConfig, PropertyViolationType,
    Result, SimulationOutcome, SimulatorConfig, SimulatorContext, SimulatorError,
    SimulatorOperation, StateQuery, TimeConfig, TimeControlAction,
};

// Re-export effect handlers (pure algebraic effects)
pub use handlers::{
    ComposedSimulationEnvironment, CoreSimulatorHandler, SimulationEffectComposer,
    SimulationFaultHandler, SimulationScenarioHandler, SimulationTimeHandler,
    StatelessSimulatorHandler,
};

// Re-export testkit bridge
pub use testkit_bridge::{MiddlewareConfig as HandlerConfig, TestkitSimulatorBridge};

// Legacy compatibility re-exports (deprecated - use pure effect handlers instead)
pub use compat::{PerformanceMetrics, SimulatorHandler};

// Re-export scenario types for convenience
pub use handlers::{InjectionAction, ScenarioDefinition, TriggerCondition};

// Re-export Duration for convenience
pub use std::time::Duration;

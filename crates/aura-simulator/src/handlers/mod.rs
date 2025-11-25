//! Layer 6: Simulation Handler Implementations - Deterministic Testing
//!
//! Effect handlers enabling deterministic simulation and property testing
//! (per docs/106_effect_system_and_runtime.md, ExecutionMode::Simulation).
//!
//! **Handler Types** (per docs/106_effect_system_and_runtime.md):
//! - **TimeControl**: Deterministic time stepping (no real delays)
//! - **FaultInjection**: Byzantine faults, chaos injection, corruption
//! - **ScenarioInjection**: Dynamic scenario modification and state setup
//! - **EffectComposition**: Combine multiple simulation effects
//!
//! **Testing Capabilities**:
//! - Seed-driven reproducible behavior (ExecutionMode::Simulation { seed })
//! - Scenario replay and modification for property testing
//! - Fault injection for resilience testing (Byzantine, network failures)
//! - Time acceleration (no wall-clock waits)

pub mod core;
pub mod effect_composer;
pub mod fault_simulation;
pub mod retry;
pub mod scenario;
pub mod stateless_simulator;
pub mod time_control;

// Re-export all handler types
pub use core::CoreSimulatorHandler;
pub use effect_composer::{ComposedSimulationEnvironment, SimulationEffectComposer};
pub use fault_simulation::SimulationFaultHandler;
pub use retry::{simulated_exponential_backoff, simulated_fixed_retry};
pub use scenario::{
    InjectionAction, ScenarioDefinition, SimulationScenarioHandler, TriggerCondition,
};
pub use stateless_simulator::StatelessSimulatorHandler;
pub use time_control::SimulationTimeHandler;

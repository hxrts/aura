//! Simulator handler implementations

pub mod core;
pub mod effect_composer;
pub mod fault_simulation;
pub mod scenario;
pub mod stateless_simulator;
pub mod time_control;

pub use core::CoreSimulatorHandler;
pub use effect_composer::{ComposedSimulationEnvironment, SimulationEffectComposer};
pub use fault_simulation::SimulationFaultHandler;
pub use scenario::{
    InjectionAction, ScenarioDefinition, SimulationScenarioHandler, TriggerCondition,
};
pub use stateless_simulator::{
    StatelessSimulatorHandler, StatelessSimulatorConfig, SimulationTickResult,
    ScenarioSummary, LegacyFaultType,
};
pub use time_control::SimulationTimeHandler;

// All standard handlers moved to middleware::handler module

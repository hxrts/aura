//! Simulation Effect System
//!
//! This module provides the simulation-specific effect system that extends the core
//! Aura effect system with simulation capabilities including fault injection,
//! time control, state inspection, property checking, and chaos coordination.

pub mod middleware;
pub mod system;

// Re-export core components
pub use middleware::{
    ChaosCoordinationMiddleware, FaultInjectionMiddleware, PropertyCheckingMiddleware,
    StateInspectionMiddleware, TimeControlMiddleware,
};
pub use system::{
    SimulationEffectSystem, SimulationEffectSystemFactory, SimulationEffectSystemStats,
};

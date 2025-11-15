//! Simulation Effect System
//!
//! This module provides the simulation-specific effect system that extends the core
//! Aura effect system with simulation capabilities. Middleware patterns have been
//! removed in favor of direct effect handler composition.

pub mod system;

// Re-export core components
// middleware exports removed - migrated to effect system
pub use system::{
    SimulationEffectSystem, SimulationEffectSystemFactory, SimulationEffectSystemStats,
};

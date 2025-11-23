//! Layer 6: Simulation Effect System
//!
//! Simulation-specific effect system extending core Aura effects with
//! deterministic time, fault injection, and scenario capabilities.

pub mod system;

// Re-export core components
// middleware exports removed - migrated to effect system
pub use system::{
    SimulationEffectSystem, SimulationEffectSystemFactory, SimulationEffectSystemStats,
};

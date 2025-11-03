//! Time effect handlers
//!
//! Provides different implementations of TimeEffects for various execution contexts.

pub mod real;
pub mod simulated;

pub use real::RealTimeHandler;
pub use simulated::SimulatedTimeHandler;
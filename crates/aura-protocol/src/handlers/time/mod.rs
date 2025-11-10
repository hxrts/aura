//! Time effect handlers
//!
//! Provides different implementations of TimeEffects for various execution contexts.

pub mod enhanced;
pub mod real;
pub mod simulated;

pub use enhanced::EnhancedTimeHandler;
pub use real::RealTimeHandler;
pub use simulated::SimulatedTimeHandler;

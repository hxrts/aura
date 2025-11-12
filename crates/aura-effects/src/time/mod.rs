//! Time effect handlers
//!
//! This module provides standard implementations of the `TimeEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

pub mod real;
pub mod simulated;

pub use real::RealTimeHandler;
pub use simulated::SimulatedTimeHandler;

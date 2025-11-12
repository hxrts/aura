//! Random effect handlers
//!
//! This module provides standard implementations of the `RandomEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

pub mod mock;
pub mod real;

pub use mock::MockRandomHandler;
pub use real::RealRandomHandler;

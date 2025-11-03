//! Network effect handlers
//!
//! Provides different implementations of NetworkEffects for various execution contexts.

pub mod memory;
pub mod real;
pub mod simulated;

pub use memory::MemoryNetworkHandler;
pub use real::RealNetworkHandler;
pub use simulated::SimulatedNetworkHandler;
//! Transport Middleware System (TODO fix - Simplified)
//!
//! **CLEANUP**: Removed over-engineered middleware layers as part of Week 11 cleanup.
//! This crate now provides essential transport abstractions with privacy-preserving
//! leakage budget tracking for the Aura threshold identity platform.
//!
//! **REMOVED**: Over-engineered middleware (-3,197 lines) that duplicated functionality
//! available in effect system, journal middleware, and aura-crypto.

pub mod memory;
// pub mod middleware; // Removed over-engineered middleware per Week 11 cleanup
pub mod hole_punch;
pub mod network;
pub mod peers;
pub mod privacy;
pub mod reconnect;
pub mod secure_channel;
pub mod stun;
pub mod websocket;

// Integration tests for SecureChannel system
#[cfg(test)]
mod secure_channel_integration_tests;

// Re-export essential components
pub use memory::*;
// pub use middleware::*; // Removed over-engineered middleware per Week 11 cleanup
pub use hole_punch::*;
pub use network::*;
pub use peers::*;
pub use privacy::*;
pub use reconnect::*;
pub use secure_channel::*;
pub use stun::*;
pub use websocket::*;

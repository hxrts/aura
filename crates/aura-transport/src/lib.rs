#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Transport Middleware System (TODO fix - Simplified)
//!
//! This crate provides essential transport abstractions with privacy-preserving
//! leakage budget tracking for the Aura threshold identity platform.

pub mod hole_punch;
pub mod memory;
pub mod network;
pub mod peers;
pub mod privacy;
pub mod reconnect;
pub mod secure_channel;
pub mod stun;
pub mod websocket;

// Re-export essential components
pub use hole_punch::*;
pub use memory::*;
pub use network::*;
pub use peers::*;
pub use privacy::*;
pub use reconnect::*;
pub use secure_channel::*;
pub use stun::*;
pub use websocket::*;

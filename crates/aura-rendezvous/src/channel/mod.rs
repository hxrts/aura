//! Secure Channel Establishment Protocol
//!
//! Complete secure channel protocol implementation using choreography.

mod handshake;
mod lifecycle;
mod rotation;
pub mod secure;

pub use handshake::{HandshakeComplete, HandshakeInit, HandshakeResponse, HandshakeResult};
pub use lifecycle::{ChannelLifecycleState, ChannelState};
pub use rotation::KeyRotationRequest;
pub use secure::{ChannelConfig, SecureChannelCoordinator};

//! Layer 2: Transport Protocol Message Types
//!
//! Protocol message types for network coordination: STUN (NAT traversal),
//! hole punching (peer-to-peer connection), WebSocket framing.
//!
//! **Design** (per docs/108_transport_and_information_flow.md):
//! - Protocol-agnostic message definitions for choreography composition
//! - Enables future choreographic implementations with multiple transport backends
//! - Messages flow through guard chain (aura-protocol/guards) for authorization/flow control

pub mod hole_punch;
pub(crate) mod rendezvous_constants;
pub mod stun;
pub mod websocket;

// Internal API only - protocols are implementation details
// Note: These are available for future choreographic implementations
// pub(crate) use stun::{StunMessage, StunMethod, StunClass, StunAttribute};
// pub(crate) use hole_punch::{HolePunchMessage};
// pub(crate) use websocket::{WebSocketMessage};

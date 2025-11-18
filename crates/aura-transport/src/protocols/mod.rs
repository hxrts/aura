//! Protocol Message Definitions
//!
//! This module provides essential protocol message types for transport coordination.
//! These types are designed for compatibility with choreographic protocols and
//! mature networking libraries.

pub mod stun;
pub mod hole_punch;
pub mod websocket;

// Internal API only - protocols are implementation details
// Note: These are available for future choreographic implementations
// pub(crate) use stun::{StunMessage, StunMethod, StunClass, StunAttribute};
// pub(crate) use hole_punch::{HolePunchMessage};
// pub(crate) use websocket::{WebSocketMessage};
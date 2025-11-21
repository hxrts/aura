//! SBB Message Integration with Transport Layer
//!
//! This module integrates SBB flooding with the existing transport layer
//! for actual message delivery across peer connections.

mod bridge;
mod transport;
mod types;

pub use transport::{
    MockTransportSender, NetworkConfig, NetworkTransport, NetworkTransportSender,
    SbbTransportBridge, TransportSender,
};
pub use types::{SbbMessageType, TransportMethod, TransportOfferPayload};

// Re-export ContextTransportBridge from context module
pub use crate::context::ContextTransportBridge;

//! Pluggable transport layer with presence ticket enforcement
//!
//! Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design
//!
//! This crate provides the Transport trait that all transport implementations must satisfy,
//! along with presence ticket structures for authenticated peer connections.

#![allow(warnings, clippy::all)] // TODO: Add comprehensive documentation in future work

pub mod envelope;
pub mod factory;
pub mod https_relay;
pub mod peer_discovery;
pub mod presence;
pub mod sbb_gossip;
pub mod sbb_publisher;
pub mod sbb_recognizer;
pub mod stub;
pub mod transport; // Unified transport with session types and capabilities
pub mod types;
pub mod unified_transport;

pub use envelope::*;
pub use factory::*;
pub use https_relay::*;
pub use peer_discovery::*;
pub use presence::*;
pub use sbb_gossip::*;
pub use sbb_publisher::*;
pub use sbb_recognizer::*;
pub use stub::*;
pub use transport::*; // This now includes session types and capability transport
pub use types::*;
pub use unified_transport::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Invalid presence ticket")]
    InvalidPresenceTicket,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Peer not found: {0}")]
    PeerNotFound(String),

    #[error("Timeout")]
    Timeout,

    #[error("Invalid transport configuration: {0}")]
    InvalidConfig(String),

    #[error("Transport type not implemented: {0}")]
    NotImplemented(String),

    #[error("Transport not authorized: {0}")]
    NotAuthorized(String),

    #[error("Invalid transport state: {0}")]
    InvalidState(String),

    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Insufficient capability: {0}")]
    InsufficientCapability(String),
}

pub type Result<T> = std::result::Result<T, TransportError>;

//! Pluggable transport layer with presence ticket enforcement
//!
//! Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design
//!
//! This crate provides the Transport trait that all transport implementations must satisfy,
//! along with presence ticket structures for authenticated peer connections.

#![allow(missing_docs)] // TODO: Add comprehensive documentation in future work

pub mod factory;
pub mod presence;
pub mod stub;
pub mod transport;  // Unified transport with session types and capabilities
pub mod types;

pub use factory::*;
pub use presence::*;
pub use stub::*;
pub use transport::*;  // This now includes session types and capability transport
pub use types::*;

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

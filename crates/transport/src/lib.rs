//! Pluggable transport layer with presence ticket enforcement
//!
//! Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design
//!
//! This crate provides the Transport trait that all transport implementations must satisfy,
//! along with presence ticket structures for authenticated peer connections.

#![allow(missing_docs)] // TODO: Add comprehensive documentation in future work

pub mod capability_transport;
pub mod presence;
pub mod stub;
pub mod transport;
pub mod types;

pub use capability_transport::*;
pub use presence::*;
pub use stub::*;
pub use transport::*;
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
}

pub type Result<T> = std::result::Result<T, TransportError>;

//! Centralized wire format types for Aura protocol messages
//!
//! This crate consolidates all over-the-wire message types that were previously
//! scattered across coordination, transport, and journal crates. It provides:
//!
//! - Protocol messages for DKD, FROST, resharing, and recovery
//! - Transport messages for presence and capability verification
//! - Version negotiation and protocol evolution support
//! - Consistent serialization across all message types
//!
//! # Architecture
//!
//! All message types include version fields for forward compatibility and
//! implement consistent serialization traits for reliable wire format.

#![allow(missing_docs)] // TODO: Add comprehensive documentation

pub mod protocol;
pub mod serialization;
pub mod transport;
pub mod versioning;

// Re-export main message types
pub use protocol::*;
pub use transport::*;
pub use versioning::*;

/// Current wire format version
pub const WIRE_FORMAT_VERSION: u16 = 1;

/// Error types for message handling
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Unsupported wire format version: {found}, max supported: {max_supported}")]
    UnsupportedVersion { found: u16, max_supported: u16 },

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Invalid message format: {0}")]
    InvalidFormat(String),
}

/// Result type for message operations
pub type MessageResult<T> = Result<T, MessageError>;

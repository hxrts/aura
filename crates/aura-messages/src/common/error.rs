//! Message-specific error types

use serde::{Deserialize, Serialize};

/// Error types for message handling
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum MessageError {
    #[error("Unsupported wire format version: {found}, max supported: {max_supported}")]
    UnsupportedVersion { found: u16, max_supported: u16 },

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Invalid message format: {0}")]
    InvalidFormat(String),

    #[error("Invalid envelope size: expected {expected}, got {actual}")]
    InvalidEnvelopeSize { expected: usize, actual: usize },

    #[error("CID mismatch: expected {expected}, computed {computed}")]
    CidMismatch { expected: String, computed: String },

    #[error("Message too large: {size} bytes exceeds limit of {limit} bytes")]
    MessageTooLarge { size: usize, limit: usize },
}

/// Result type for message operations
pub type MessageResult<T> = Result<T, MessageError>;

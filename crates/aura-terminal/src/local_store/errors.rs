//! Local store error types

use thiserror::Error;

/// Errors that can occur in local storage operations
#[derive(Debug, Error)]
pub enum LocalStoreError {
    /// Storage effects operation failed
    #[error("storage operation failed: {0}")]
    StorageError(String),

    /// Serialization failed
    #[error("serialization failed: {0}")]
    SerializationError(String),

    /// Deserialization failed
    #[error("deserialization failed: {0}")]
    DeserializationError(String),
}

//! Local store error types

use thiserror::Error;

/// Errors that can occur in local storage operations
#[derive(Debug, Error)]
pub enum LocalStoreError {
    /// Failed to read from store file
    #[error("failed to read store: {0}")]
    ReadError(String),

    /// Failed to write to store file
    #[error("failed to write store: {0}")]
    WriteError(String),

    /// Storage effects operation failed
    #[error("storage operation failed: {0}")]
    StorageError(String),

    /// Encryption failed
    #[error("encryption failed: {0}")]
    EncryptionError(String),

    /// Decryption failed
    #[error("decryption failed: {0}")]
    DecryptionError(String),

    /// Key derivation failed
    #[error("key derivation failed: {0}")]
    KeyDerivationError(String),

    /// Serialization failed
    #[error("serialization failed: {0}")]
    SerializationError(String),

    /// Deserialization failed
    #[error("deserialization failed: {0}")]
    DeserializationError(String),

    /// Invalid store format
    #[error("invalid store format: {0}")]
    InvalidFormat(String),
}

impl From<std::io::Error> for LocalStoreError {
    fn from(err: std::io::Error) -> Self {
        LocalStoreError::ReadError(err.to_string())
    }
}

impl From<serde_json::Error> for LocalStoreError {
    fn from(err: serde_json::Error) -> Self {
        LocalStoreError::SerializationError(err.to_string())
    }
}

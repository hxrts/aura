//! Unified storage error types
//!
//! This module defines comprehensive error types for all storage operations,
//! consolidating errors from the old aura-store and aura-storage crates.

use thiserror::Error;
use serde::{Deserialize, Serialize};
use aura_core::{ChunkId, ContentId};

/// Comprehensive storage error type
#[derive(Debug, Clone, Error, Serialize, Deserialize, PartialEq)]
pub enum StorageError {
    // Content and chunk errors
    /// Chunk not found
    #[error("Chunk not found: {0}")]
    ChunkNotFound(ChunkId),

    /// Content not found
    #[error("Content not found: {0}")]
    ContentNotFound(ContentId),

    /// Invalid chunk ID format
    #[error("Invalid chunk ID: {0}")]
    InvalidChunkId(String),

    /// Invalid content format
    #[error("Invalid content: {0}")]
    InvalidContent(String),

    /// Invalid chunk layout
    #[error("Invalid chunk layout: {0}")]
    InvalidChunkLayout(String),

    // Access control errors
    /// Permission denied for resource access
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Insufficient capabilities for operation
    #[error("Insufficient capabilities: {0}")]
    InsufficientCapabilities(String),

    /// Invalid capability token
    #[error("Invalid capability token: {0}")]
    InvalidCapability(String),

    // Storage backend errors
    /// I/O error from storage backend
    #[error("I/O error: {0}")]
    IoError(String),

    /// Storage backend unavailable
    #[error("Storage backend unavailable: {0}")]
    BackendUnavailable(String),

    /// Storage quota exceeded
    #[error("Storage quota exceeded")]
    QuotaExceeded,

    /// Storage corruption detected
    #[error("Storage corruption detected: {0}")]
    CorruptionDetected(String),

    // Serialization and encoding errors
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),

    /// Encoding error (base64, hex, etc.)
    #[error("Encoding error: {0}")]
    EncodingError(String),

    // Search errors
    /// Search query parse error
    #[error("Search query parse error: {0}")]
    SearchQueryError(String),

    /// Search index error
    #[error("Search index error: {0}")]
    SearchIndexError(String),

    /// Search timeout
    #[error("Search operation timed out")]
    SearchTimeout,

    // Network and coordination errors
    /// Network error during distributed operation
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Synchronization error
    #[error("Synchronization error: {0}")]
    SynchronizationError(String),

    /// Consensus error in distributed operation
    #[error("Consensus error: {0}")]
    ConsensusError(String),

    /// Operation timeout
    #[error("Operation timed out")]
    Timeout,

    // CRDT and consistency errors
    /// CRDT merge conflict
    #[error("CRDT merge conflict: {0}")]
    MergeConflict(String),

    /// Causal ordering violation
    #[error("Causal ordering violation: {0}")]
    CausalViolation(String),

    /// Version conflict
    #[error("Version conflict: expected {expected}, got {actual}")]
    VersionConflict { 
        /// Expected version
        expected: u64, 
        /// Actual version found
        actual: u64 
    },

    // Configuration and setup errors
    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Storage not initialized
    #[error("Storage not initialized")]
    NotInitialized,

    /// Storage already initialized
    #[error("Storage already initialized")]
    AlreadyInitialized,

    // Cryptographic errors
    /// Encryption error
    #[error("Encryption error: {0}")]
    EncryptionError(String),

    /// Decryption error
    #[error("Decryption error: {0}")]
    DecryptionError(String),

    /// Key derivation error
    #[error("Key derivation error: {0}")]
    KeyDerivationError(String),

    /// Invalid cryptographic parameters
    #[error("Invalid cryptographic parameters: {0}")]
    InvalidCryptoParams(String),

    // Generic errors
    /// Internal error (should not happen in normal operation)
    #[error("Internal error: {0}")]
    InternalError(String),

    /// Operation not supported
    #[error("Operation not supported: {0}")]
    NotSupported(String),

    /// Resource temporarily unavailable
    #[error("Resource temporarily unavailable: {0}")]
    TemporarilyUnavailable(String),
}

impl StorageError {
    /// Create a new permission denied error
    pub fn permission_denied(msg: &str) -> Self {
        Self::PermissionDenied(msg.to_string())
    }

    /// Create a new insufficient capabilities error
    pub fn insufficient_capabilities(msg: &str) -> Self {
        Self::InsufficientCapabilities(msg.to_string())
    }

    /// Create a new I/O error
    pub fn io_error(msg: &str) -> Self {
        Self::IoError(msg.to_string())
    }

    /// Create a new serialization error
    pub fn serialization_error(msg: &str) -> Self {
        Self::SerializationError(msg.to_string())
    }

    /// Create a new network error
    pub fn network_error(msg: &str) -> Self {
        Self::NetworkError(msg.to_string())
    }

    /// Create a new internal error
    pub fn internal_error(msg: &str) -> Self {
        Self::InternalError(msg.to_string())
    }

    /// Check if this is a transient error that might succeed on retry
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            StorageError::NetworkError(_)
                | StorageError::BackendUnavailable(_)
                | StorageError::Timeout
                | StorageError::SearchTimeout
                | StorageError::TemporarilyUnavailable(_)
        )
    }

    /// Check if this is a permissions-related error
    pub fn is_permission_error(&self) -> bool {
        matches!(
            self,
            StorageError::PermissionDenied(_)
                | StorageError::InsufficientCapabilities(_)
                | StorageError::InvalidCapability(_)
        )
    }

    /// Check if this is a content-related error
    pub fn is_content_error(&self) -> bool {
        matches!(
            self,
            StorageError::ChunkNotFound(_)
                | StorageError::ContentNotFound(_)
                | StorageError::InvalidChunkId(_)
                | StorageError::InvalidContent(_)
                | StorageError::InvalidChunkLayout(_)
        )
    }

    /// Check if this is a storage backend error
    pub fn is_backend_error(&self) -> bool {
        matches!(
            self,
            StorageError::IoError(_)
                | StorageError::BackendUnavailable(_)
                | StorageError::QuotaExceeded
                | StorageError::CorruptionDetected(_)
        )
    }

    /// Get error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            StorageError::ChunkNotFound(_)
            | StorageError::ContentNotFound(_)
            | StorageError::InvalidChunkId(_)
            | StorageError::InvalidContent(_)
            | StorageError::InvalidChunkLayout(_) => "content",

            StorageError::PermissionDenied(_)
            | StorageError::InsufficientCapabilities(_)
            | StorageError::InvalidCapability(_) => "permission",

            StorageError::IoError(_)
            | StorageError::BackendUnavailable(_)
            | StorageError::QuotaExceeded
            | StorageError::CorruptionDetected(_) => "backend",

            StorageError::SerializationError(_)
            | StorageError::DeserializationError(_)
            | StorageError::EncodingError(_) => "serialization",

            StorageError::SearchQueryError(_)
            | StorageError::SearchIndexError(_)
            | StorageError::SearchTimeout => "search",

            StorageError::NetworkError(_)
            | StorageError::SynchronizationError(_)
            | StorageError::ConsensusError(_)
            | StorageError::Timeout => "network",

            StorageError::MergeConflict(_)
            | StorageError::CausalViolation(_)
            | StorageError::VersionConflict { .. } => "consistency",

            StorageError::InvalidConfiguration(_)
            | StorageError::NotInitialized
            | StorageError::AlreadyInitialized => "configuration",

            StorageError::EncryptionError(_)
            | StorageError::DecryptionError(_)
            | StorageError::KeyDerivationError(_)
            | StorageError::InvalidCryptoParams(_) => "crypto",

            StorageError::InternalError(_)
            | StorageError::NotSupported(_)
            | StorageError::TemporarilyUnavailable(_) => "internal",
        }
    }

    /// Get user-friendly error message
    pub fn user_message(&self) -> &str {
        match self {
            StorageError::ChunkNotFound(_) | StorageError::ContentNotFound(_) => {
                "The requested content could not be found"
            }
            StorageError::PermissionDenied(_) | StorageError::InsufficientCapabilities(_) => {
                "You don't have permission to access this resource"
            }
            StorageError::QuotaExceeded => "Storage quota has been exceeded",
            StorageError::BackendUnavailable(_) => "Storage service is temporarily unavailable",
            StorageError::NetworkError(_) | StorageError::Timeout => {
                "Network error occurred, please try again"
            }
            StorageError::SearchTimeout => "Search operation took too long, please try again",
            _ => "An error occurred while accessing storage",
        }
    }
}

/// Convert from standard I/O errors
impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        StorageError::IoError(error.to_string())
    }
}

/// Convert from serialization errors
impl From<serde_json::Error> for StorageError {
    fn from(error: serde_json::Error) -> Self {
        StorageError::SerializationError(error.to_string())
    }
}

/// Convert from hex decode errors
impl From<hex::FromHexError> for StorageError {
    fn from(error: hex::FromHexError) -> Self {
        StorageError::EncodingError(format!("Hex decode error: {}", error))
    }
}

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_categories() {
        let chunk_error = StorageError::ChunkNotFound(ChunkId::from_bytes(b"test"));
        assert_eq!(chunk_error.category(), "content");
        assert!(chunk_error.is_content_error());

        let perm_error = StorageError::permission_denied("test");
        assert_eq!(perm_error.category(), "permission");
        assert!(perm_error.is_permission_error());

        let io_error = StorageError::io_error("test");
        assert_eq!(io_error.category(), "backend");
        assert!(io_error.is_backend_error());

        let net_error = StorageError::network_error("test");
        assert_eq!(net_error.category(), "network");
        assert!(net_error.is_transient());
    }

    #[test]
    fn test_user_messages() {
        let chunk_error = StorageError::ChunkNotFound(ChunkId::from_bytes(b"test"));
        assert!(!chunk_error.user_message().is_empty());

        let perm_error = StorageError::permission_denied("test");
        assert!(perm_error.user_message().contains("permission"));

        let quota_error = StorageError::QuotaExceeded;
        assert!(quota_error.user_message().contains("quota"));
    }

    #[test]
    fn test_error_conversions() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let storage_err: StorageError = io_err.into();
        assert!(matches!(storage_err, StorageError::IoError(_)));

        let json_err = serde_json::from_str::<()>("invalid json").unwrap_err();
        let storage_err: StorageError = json_err.into();
        assert!(matches!(storage_err, StorageError::SerializationError(_)));
    }

    #[test]
    fn test_version_conflict() {
        let version_err = StorageError::VersionConflict {
            expected: 5,
            actual: 3,
        };
        assert!(version_err.to_string().contains("expected 5"));
        assert!(version_err.to_string().contains("got 3"));
    }
}
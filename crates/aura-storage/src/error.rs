//! Storage-specific error types
//!
//! This module defines error types specific to the storage layer,
//! though most errors are handled through aura_core::AuraError.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Storage-specific error types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageError {
    /// Content not found
    ContentNotFound { content_id: String },
    /// Chunk not found
    ChunkNotFound { chunk_id: String },
    /// Storage quota exceeded
    QuotaExceeded { requested: u64, available: u64 },
    /// Content corruption detected
    ContentCorruption {
        expected_hash: String,
        actual_hash: String,
    },
    /// Invalid content type
    InvalidContentType {
        content_type: String,
        allowed_types: Vec<String>,
    },
    /// Capability verification failed
    CapabilityVerificationFailed {
        required: String,
        available: Vec<String>,
    },
    /// Encryption/decryption failed
    CryptographicFailure { operation: String, reason: String },
    /// Compression/decompression failed
    CompressionFailure { algorithm: String, reason: String },
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::ContentNotFound { content_id } => {
                write!(f, "Content not found: {}", content_id)
            }
            StorageError::ChunkNotFound { chunk_id } => {
                write!(f, "Chunk not found: {}", chunk_id)
            }
            StorageError::QuotaExceeded {
                requested,
                available,
            } => {
                write!(
                    f,
                    "Storage quota exceeded: requested {} bytes, available {} bytes",
                    requested, available
                )
            }
            StorageError::ContentCorruption {
                expected_hash,
                actual_hash,
            } => {
                write!(
                    f,
                    "Content corruption detected: expected hash {}, got {}",
                    expected_hash, actual_hash
                )
            }
            StorageError::InvalidContentType {
                content_type,
                allowed_types,
            } => {
                write!(
                    f,
                    "Invalid content type '{}', allowed types: {:?}",
                    content_type, allowed_types
                )
            }
            StorageError::CapabilityVerificationFailed {
                required,
                available,
            } => {
                write!(
                    f,
                    "Capability verification failed: required '{}', available {:?}",
                    required, available
                )
            }
            StorageError::CryptographicFailure { operation, reason } => {
                write!(
                    f,
                    "Cryptographic operation '{}' failed: {}",
                    operation, reason
                )
            }
            StorageError::CompressionFailure { algorithm, reason } => {
                write!(f, "Compression with '{}' failed: {}", algorithm, reason)
            }
        }
    }
}

impl std::error::Error for StorageError {}

/// Convert storage error to AuraError
impl From<StorageError> for aura_core::AuraError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::ContentNotFound { .. } | StorageError::ChunkNotFound { .. } => {
                aura_core::AuraError::not_found(err.to_string())
            }
            StorageError::QuotaExceeded { .. } => aura_core::AuraError::storage(err.to_string()),
            StorageError::ContentCorruption { .. } => {
                aura_core::AuraError::storage(err.to_string())
            }
            StorageError::InvalidContentType { .. } => {
                aura_core::AuraError::invalid(err.to_string())
            }
            StorageError::CapabilityVerificationFailed { .. } => {
                aura_core::AuraError::permission_denied(err.to_string())
            }
            StorageError::CryptographicFailure { .. } => {
                aura_core::AuraError::crypto(err.to_string())
            }
            StorageError::CompressionFailure { .. } => {
                aura_core::AuraError::storage(err.to_string())
            }
        }
    }
}

/// Storage operation result type
pub type StorageResult<T> = Result<T, StorageError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_error_display() {
        let error = StorageError::ContentNotFound {
            content_id: "test-content-123".into(),
        };

        assert_eq!(error.to_string(), "Content not found: test-content-123");
    }

    #[test]
    fn test_storage_error_to_aura_error() {
        let storage_err = StorageError::QuotaExceeded {
            requested: 1000,
            available: 500,
        };

        let aura_err: aura_core::AuraError = storage_err.into();
        assert!(aura_err.to_string().contains("quota exceeded"));
    }

    #[test]
    fn test_capability_verification_error() {
        let error = StorageError::CapabilityVerificationFailed {
            required: "storage.read".into(),
            available: vec!["storage.write".into()],
        };

        let aura_err: aura_core::AuraError = error.into();
        // Should be a permission denied error
        assert!(aura_err.to_string().to_lowercase().contains("permission"));
    }
}

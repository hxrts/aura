//! Unified error system for Aura core
//!
//! This module provides a single, simple error type to replace the over-engineered
//! error hierarchy. Following the whole system model principle of simplicity.

use serde::{Deserialize, Serialize};

/// Unified error type for all Aura operations
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum AuraError {
    /// Invalid input or configuration
    #[error("Invalid: {message}")]
    Invalid {
        /// Error message describing the invalid input
        message: String,
    },

    /// Resource not found
    #[error("Not found: {message}")]
    NotFound {
        /// Error message describing what was not found
        message: String,
    },

    /// Permission denied
    #[error("Permission denied: {message}")]
    PermissionDenied {
        /// Error message describing the permission issue
        message: String,
    },

    /// Cryptographic operation failed
    #[error("Crypto error: {message}")]
    Crypto {
        /// Error message describing the cryptographic failure
        message: String,
    },

    /// Network or transport error
    #[error("Network error: {message}")]
    Network {
        /// Error message describing the network issue
        message: String,
    },

    /// Serialization/deserialization error
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message describing the serialization failure
        message: String,
    },

    /// Storage operation failed
    #[error("Storage error: {message}")]
    Storage {
        /// Error message describing the storage failure
        message: String,
    },

    /// Internal system error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message describing the internal error
        message: String,
    },
}

impl AuraError {
    /// Create an invalid input error
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
        }
    }

    /// Create a permission denied error
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied {
            message: message.into(),
        }
    }

    /// Create a crypto error
    pub fn crypto(message: impl Into<String>) -> Self {
        Self::Crypto {
            message: message.into(),
        }
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
        }
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Create a storage error
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    /// Create a coordination failed error
    pub fn coordination_failed(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Coordination failed: {}", message.into()),
        }
    }
}

/// Standard Result type for Aura operations
pub type Result<T> = std::result::Result<T, AuraError>;

// Conversion traits for common error types
#[cfg(feature = "json-debug")]
impl From<serde_json::Error> for AuraError {
    fn from(err: serde_json::Error) -> Self {
        Self::serialization(err.to_string())
    }
}

impl From<std::io::Error> for AuraError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::not_found(err.to_string()),
            std::io::ErrorKind::PermissionDenied => Self::permission_denied(err.to_string()),
            _ => Self::internal(err.to_string()),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for AuraError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = AuraError::invalid("test message");
        assert!(matches!(err, AuraError::Invalid { .. }));
        assert_eq!(err.to_string(), "Invalid: test message");
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let aura_err = AuraError::from(io_err);
        assert!(matches!(aura_err, AuraError::NotFound { .. }));
    }

    #[test]
    fn test_result_type() {
        fn test_function() -> Result<i32> {
            Ok(42)
        }

        let result = test_function();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}

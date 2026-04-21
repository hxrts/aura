//! Unified error system for Aura core
//!
//! This module provides a single, simple error type to replace the over-engineered
//! error hierarchy. Following the whole system model principle of simplicity.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

type AuraErrorSource = Arc<dyn std::error::Error + Send + Sync>;

/// Unified error type for all Aura operations
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum AuraError {
    /// Invalid input or configuration
    #[error("Invalid: {message}")]
    Invalid {
        /// Error message describing the invalid input
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Resource not found
    #[error("Not found: {message}")]
    NotFound {
        /// Error message describing what was not found
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Permission denied
    #[error("Permission denied: {message}")]
    PermissionDenied {
        /// Error message describing the permission issue
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Cryptographic operation failed
    #[error("Crypto error: {message}")]
    Crypto {
        /// Error message describing the cryptographic failure
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Network or transport error
    #[error("Network error: {message}")]
    Network {
        /// Error message describing the network issue
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Serialization/deserialization error
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message describing the serialization failure
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Storage operation failed
    #[error("Storage error: {message}")]
    Storage {
        /// Error message describing the storage failure
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Internal system error
    #[error("Internal error: {message}")]
    Internal {
        /// Error message describing the internal error
        message: String,
        /// Underlying source error when available.
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraErrorSource>,
    },

    /// Terminal operation error
    #[error("Terminal error: {0}")]
    Terminal(String),
}

/// Shared error code mapping for protocol-level errors.
pub trait ProtocolErrorCode {
    fn code(&self) -> &'static str;
}

impl ProtocolErrorCode for AuraError {
    fn code(&self) -> &'static str {
        match self {
            AuraError::Invalid { .. } => "invalid",
            AuraError::NotFound { .. } => "not_found",
            AuraError::PermissionDenied { .. } => "permission_denied",
            AuraError::Crypto { .. } => "crypto",
            AuraError::Network { .. } => "network",
            AuraError::Serialization { .. } => "serialization",
            AuraError::Storage { .. } => "storage",
            AuraError::Internal { .. } => "internal",
            AuraError::Terminal(_) => "terminal",
        }
    }
}

impl AuraError {
    fn invalid_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::Invalid {
            message: message.into(),
            source: Some(source),
        }
    }

    fn not_found_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::NotFound {
            message: message.into(),
            source: Some(source),
        }
    }

    fn permission_denied_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::PermissionDenied {
            message: message.into(),
            source: Some(source),
        }
    }

    fn network_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::Network {
            message: message.into(),
            source: Some(source),
        }
    }

    fn serialization_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::Serialization {
            message: message.into(),
            source: Some(source),
        }
    }

    fn storage_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::Storage {
            message: message.into(),
            source: Some(source),
        }
    }

    fn internal_with_source(message: impl Into<String>, source: AuraErrorSource) -> Self {
        Self::Internal {
            message: message.into(),
            source: Some(source),
        }
    }

    /// Create an invalid input error
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
            source: None,
        }
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound {
            message: message.into(),
            source: None,
        }
    }

    /// Create a permission denied error
    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::PermissionDenied {
            message: message.into(),
            source: None,
        }
    }

    /// Create a crypto error
    pub fn crypto(message: impl Into<String>) -> Self {
        Self::Crypto {
            message: message.into(),
            source: None,
        }
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::Network {
            message: message.into(),
            source: None,
        }
    }

    /// Create a serialization error
    pub fn serialization(message: impl Into<String>) -> Self {
        Self::Serialization {
            message: message.into(),
            source: None,
        }
    }

    /// Create a storage error
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
            source: None,
        }
    }

    /// Create an agent error
    pub fn agent(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            source: None,
        }
    }

    /// Create a chat error
    pub fn chat(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Chat error: {}", message.into()),
            source: None,
        }
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
            source: None,
        }
    }

    /// Create a terminal error
    pub fn terminal(message: impl Into<String>) -> Self {
        Self::Terminal(message.into())
    }

    /// Create a coordination failed error
    pub fn coordination_failed(message: impl Into<String>) -> Self {
        Self::Internal {
            message: format!("Coordination failed: {}", message.into()),
            source: None,
        }
    }

    /// Create a budget exceeded error
    pub fn budget_exceeded(message: impl Into<String>) -> Self {
        Self::PermissionDenied {
            message: format!("Budget exceeded: {}", message.into()),
            source: None,
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            // Network errors are typically retryable
            Self::Network { .. } => true,
            // Storage errors might be retryable
            Self::Storage { .. } => true,
            // Other errors are not retryable
            _ => false,
        }
    }

    /// Get the error category as a string
    pub fn category(&self) -> &'static str {
        match self {
            Self::Invalid { .. } => "invalid",
            Self::NotFound { .. } => "not_found",
            Self::PermissionDenied { .. } => "permission_denied",
            Self::Crypto { .. } => "crypto",
            Self::Network { .. } => "network",
            Self::Serialization { .. } => "serialization",
            Self::Storage { .. } => "storage",
            Self::Internal { .. } => "internal",
            Self::Terminal(_) => "terminal",
        }
    }
}

/// Standard Result type for Aura operations
pub type Result<T> = std::result::Result<T, AuraError>;

// Conversion traits for common error types
impl From<serde_json::Error> for AuraError {
    fn from(err: serde_json::Error) -> Self {
        Self::serialization_with_source(err.to_string(), Arc::new(err))
    }
}

impl From<toml::de::Error> for AuraError {
    fn from(err: toml::de::Error) -> Self {
        Self::serialization_with_source(err.to_string(), Arc::new(err))
    }
}

impl From<std::io::Error> for AuraError {
    fn from(err: std::io::Error) -> Self {
        let message = err.to_string();
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::not_found_with_source(message, Arc::new(err)),
            std::io::ErrorKind::PermissionDenied => {
                Self::permission_denied_with_source(message, Arc::new(err))
            }
            _ => Self::internal_with_source(message, Arc::new(err)),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for AuraError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        let message = err.to_string();
        Self::internal_with_source(message, Arc::from(err))
    }
}

impl From<biscuit_auth::error::Token> for AuraError {
    fn from(err: biscuit_auth::error::Token) -> Self {
        Self::permission_denied_with_source(format!("Biscuit token error: {err}"), Arc::new(err))
    }
}

impl From<uuid::Error> for AuraError {
    fn from(err: uuid::Error) -> Self {
        Self::invalid_with_source(format!("UUID error: {err}"), Arc::new(err))
    }
}

impl From<hex::FromHexError> for AuraError {
    fn from(err: hex::FromHexError) -> Self {
        Self::serialization_with_source(format!("Hex decoding error: {err}"), Arc::new(err))
    }
}

impl From<base64::DecodeError> for AuraError {
    fn from(err: base64::DecodeError) -> Self {
        Self::serialization_with_source(format!("Base64 decoding error: {err}"), Arc::new(err))
    }
}

impl From<crate::effects::StorageError> for AuraError {
    fn from(err: crate::effects::StorageError) -> Self {
        Self::storage_with_source(format!("Storage error: {err}"), Arc::new(err))
    }
}

impl From<crate::effects::TimeError> for AuraError {
    fn from(err: crate::effects::TimeError) -> Self {
        Self::internal_with_source(format!("Time error: {err}"), Arc::new(err))
    }
}

impl From<crate::effects::NetworkError> for AuraError {
    fn from(err: crate::effects::NetworkError) -> Self {
        Self::network_with_source(err.to_string(), Arc::new(err))
    }
}

impl From<crate::effects::ChoreographyError> for AuraError {
    fn from(err: crate::effects::ChoreographyError) -> Self {
        Self::internal_with_source(format!("Choreography error: {err}"), Arc::new(err))
    }
}

impl From<crate::effects::AuthorizationError> for AuraError {
    fn from(err: crate::effects::AuthorizationError) -> Self {
        Self::permission_denied_with_source(err.to_string(), Arc::new(err))
    }
}

impl From<crate::effects::QueryError> for AuraError {
    fn from(err: crate::effects::QueryError) -> Self {
        Self::invalid_with_source(format!("Query error: {err}"), Arc::new(err))
    }
}

impl From<crate::effects::FactError> for AuraError {
    fn from(err: crate::effects::FactError) -> Self {
        Self::invalid_with_source(format!("Fact error: {err}"), Arc::new(err))
    }
}

impl From<crate::util::serialization::SerializationError> for AuraError {
    fn from(err: crate::util::serialization::SerializationError) -> Self {
        Self::serialization_with_source(err.to_string(), Arc::new(err))
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
        assert!(std::error::Error::source(&aura_err).is_some());
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

    #[test]
    fn test_serialization_error_preserves_source_chain() {
        let invalid_json = match serde_json::from_str::<serde_json::Value>("{not json") {
            Ok(value) => panic!("invalid json unexpectedly parsed as {value}"),
            Err(error) => error,
        };
        let err = AuraError::from(invalid_json);

        assert!(matches!(err, AuraError::Serialization { .. }));
        let source =
            std::error::Error::source(&err).unwrap_or_else(|| panic!("source should be preserved"));
        assert!(!source.to_string().is_empty());
    }
}

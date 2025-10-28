//! Unified serialization error types

use thiserror::Error;

/// Serialization operation result type
pub type Result<T> = std::result::Result<T, SerializationError>;

/// Unified error type for all serialization operations
#[derive(Error, Debug, Clone)]
pub enum SerializationError {
    /// JSON serialization/deserialization error
    #[error("JSON serialization error: {0}")]
    JsonError(String),

    /// CBOR serialization/deserialization error
    #[error("CBOR serialization error: {0}")]
    CborError(String),

    /// Bincode serialization/deserialization error
    #[error("Bincode serialization error: {0}")]
    BincodeError(String),

    /// TOML serialization/deserialization error
    #[error("TOML serialization error: {0}")]
    TomlError(String),

    /// Postcard serialization/deserialization error
    #[error("Postcard serialization error: {0}")]
    PostcardError(String),

    /// UTF-8 encoding error
    #[error("UTF-8 encoding error: {0}")]
    Utf8Error(String),

    /// I/O error during serialization
    #[error("I/O error during serialization: {0}")]
    IoError(String),

    /// Type mismatch during deserialization
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// Invalid data format
    #[error("Invalid data format: {0}")]
    InvalidFormat(String),

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
}

impl SerializationError {
    /// Create a JSON error
    pub fn json<S: Into<String>>(msg: S) -> Self {
        Self::JsonError(msg.into())
    }

    /// Create a CBOR error
    pub fn cbor<S: Into<String>>(msg: S) -> Self {
        Self::CborError(msg.into())
    }

    /// Create a Bincode error
    pub fn bincode<S: Into<String>>(msg: S) -> Self {
        Self::BincodeError(msg.into())
    }

    /// Create a TOML error
    pub fn toml<S: Into<String>>(msg: S) -> Self {
        Self::TomlError(msg.into())
    }

    /// Create a Postcard error
    pub fn postcard<S: Into<String>>(msg: S) -> Self {
        Self::PostcardError(msg.into())
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::IoError(_))
    }

    /// Check if error is due to invalid input
    pub fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidFormat(_) | Self::TypeMismatch { .. } | Self::Utf8Error(_)
        )
    }
}

// Conversion from serde_json::Error
impl From<serde_json::error::Error> for SerializationError {
    fn from(err: serde_json::error::Error) -> Self {
        Self::json(err.to_string())
    }
}

// Conversion from serde_cbor::Error
impl From<serde_cbor::error::Error> for SerializationError {
    fn from(err: serde_cbor::error::Error) -> Self {
        Self::cbor(err.to_string())
    }
}

// Bincode errors are internal, so we don't implement From directly
// Instead, callers use .map_err() with our bincode() constructor

// Conversion from toml::de::Error
impl From<toml::de::Error> for SerializationError {
    fn from(err: toml::de::Error) -> Self {
        Self::toml(err.to_string())
    }
}

// Conversion from toml::ser::Error
impl From<toml::ser::Error> for SerializationError {
    fn from(err: toml::ser::Error) -> Self {
        Self::toml(err.to_string())
    }
}

// Conversion from std::string::FromUtf8Error
impl From<std::string::FromUtf8Error> for SerializationError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        Self::Utf8Error(err.to_string())
    }
}

// Conversion from std::io::Error
impl From<std::io::Error> for SerializationError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = SerializationError::json("test error");
        assert!(matches!(err, SerializationError::JsonError(_)));
    }

    #[test]
    fn test_is_retryable() {
        let retryable = SerializationError::IoError("connection timeout".to_string());
        assert!(retryable.is_retryable());

        let non_retryable = SerializationError::InvalidFormat("bad format".to_string());
        assert!(!non_retryable.is_retryable());
    }

    #[test]
    fn test_is_invalid_input() {
        let invalid = SerializationError::InvalidFormat("bad format".to_string());
        assert!(invalid.is_invalid_input());

        let valid = SerializationError::IoError("io error".to_string());
        assert!(!valid.is_invalid_input());
    }
}

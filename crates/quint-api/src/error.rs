//! Error types for native Quint API

use thiserror::Error;

/// Result type for Quint API operations
pub type QuintResult<T> = Result<T, QuintError>;

/// Errors that can occur when interacting with Quint
#[derive(Error, Debug)]
pub enum QuintError {
    /// Quint parser not available or parsing failed
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Quint evaluation error
    #[error("Evaluation error: {0}")]
    EvaluationError(String),

    /// Property specification error
    #[error("Property specification error: {0}")]
    PropertySpecError(String),

    /// Property parsing error
    #[error("Property parsing failed: {message} at {location}")]
    PropertyParseError { message: String, location: String },

    /// Verification error
    #[error("Verification failed: {0}")]
    VerificationError(String),

    /// Verification timeout
    #[error("Verification timed out after {timeout_ms}ms")]
    VerificationTimeout { timeout_ms: u64 },

    /// Process execution error
    #[error("Process execution failed: {command} - {message}")]
    ProcessExecutionError { command: String, message: String },

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    JsonError(String),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl QuintError {
    /// Create a property specification error
    pub fn property_spec_error(message: impl Into<String>) -> Self {
        Self::PropertySpecError(message.into())
    }

    /// Create a property parsing error
    pub fn property_parse_error(message: impl Into<String>, location: impl Into<String>) -> Self {
        Self::PropertyParseError {
            message: message.into(),
            location: location.into(),
        }
    }

    /// Create a verification error
    pub fn verification_error(message: impl Into<String>) -> Self {
        Self::VerificationError(message.into())
    }

    /// Create a process execution error
    pub fn process_execution_error(command: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ProcessExecutionError {
            command: command.into(),
            message: message.into(),
        }
    }

    /// Create an internal error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::InternalError(message.into())
    }
}

impl From<serde_json::Error> for QuintError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

impl From<std::io::Error> for QuintError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

impl From<tokio::task::JoinError> for QuintError {
    fn from(err: tokio::task::JoinError) -> Self {
        Self::InternalError(format!("Task join error: {}", err))
    }
}
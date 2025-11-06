//! Domain error types for the Aura agent
//!
//! This module defines error types specific to the agent domain layer.
//! These errors wrap underlying effect system errors and provide
//! agent-specific context.

use aura_protocol::effects::{
    AuthError, ConfigError, DeviceStorageError, JournalError, SessionError,
};
use aura_types::{AuraError, ErrorCode, ErrorSeverity};
use thiserror::Error;

/// Result type for agent operations
pub type Result<T> = std::result::Result<T, AgentError>;

/// Errors that can occur during agent operations
#[derive(Debug, Error)]
pub enum AgentError {
    /// Effect system initialization failed
    #[error("Effect system error: {0}")]
    EffectSystemError(String),

    /// Device storage operation failed
    #[error("Storage operation failed: {0}")]
    StorageError(#[from] DeviceStorageError),

    /// Configuration operation failed
    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),

    /// Authentication operation failed
    #[error("Authentication error: {0}")]
    AuthError(#[from] AuthError),

    /// Session management operation failed
    #[error("Session error: {0}")]
    SessionError(#[from] SessionError),

    /// Journal operation failed
    #[error("Journal error: {0}")]
    JournalError(#[from] JournalError),

    /// Agent initialization failed
    #[error("Initialization failed: {0}")]
    InitializationFailed(String),

    /// Agent not initialized
    #[error("Agent not initialized")]
    NotInitialized,

    /// Invalid device ID
    #[error("Invalid device ID: {0}")]
    InvalidDeviceId(String),

    /// Invalid account ID
    #[error("Invalid account ID: {0}")]
    InvalidAccountId(String),

    /// Operation timed out
    #[error("Operation timed out after {0}s")]
    OperationTimeout(u64),

    /// Permission denied for operation
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Invalid operation state
    #[error("Invalid state for operation: {0}")]
    InvalidState(String),

    /// Validation failed
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// Internal agent error
    #[error("Internal agent error: {0}")]
    InternalError(String),
}

impl AgentError {
    /// Get error severity for this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            AgentError::EffectSystemError(_) => ErrorSeverity::Critical,
            AgentError::StorageError(DeviceStorageError::PermissionDenied) => ErrorSeverity::High,
            AgentError::AuthError(AuthError::AuthenticationFailed) => ErrorSeverity::High,
            AgentError::InitializationFailed(_) => ErrorSeverity::High,
            AgentError::NotInitialized => ErrorSeverity::Medium,
            AgentError::InvalidDeviceId(_) => ErrorSeverity::Medium,
            AgentError::InvalidAccountId(_) => ErrorSeverity::Medium,
            AgentError::OperationTimeout(_) => ErrorSeverity::Medium,
            AgentError::PermissionDenied(_) => ErrorSeverity::Medium,
            AgentError::NotFound(_) => ErrorSeverity::Low,
            AgentError::ValidationFailed(_) => ErrorSeverity::Low,
            _ => ErrorSeverity::Medium,
        }
    }

    /// Get error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            AgentError::EffectSystemError(_) => ErrorCode::SystemError,
            AgentError::StorageError(_) => ErrorCode::StorageError,
            AgentError::ConfigError(_) => ErrorCode::ConfigurationError,
            AgentError::AuthError(_) => ErrorCode::AuthenticationError,
            AgentError::SessionError(_) => ErrorCode::SessionError,
            AgentError::JournalError(_) => ErrorCode::ProtocolError,
            AgentError::InitializationFailed(_) => ErrorCode::InitializationError,
            AgentError::NotInitialized => ErrorCode::StateError,
            AgentError::InvalidDeviceId(_) => ErrorCode::ValidationError,
            AgentError::InvalidAccountId(_) => ErrorCode::ValidationError,
            AgentError::OperationTimeout(_) => ErrorCode::TimeoutError,
            AgentError::PermissionDenied(_) => ErrorCode::PermissionError,
            AgentError::NotFound(_) => ErrorCode::NotFoundError,
            AgentError::InvalidState(_) => ErrorCode::StateError,
            AgentError::ValidationFailed(_) => ErrorCode::ValidationError,
            AgentError::InternalError(_) => ErrorCode::InternalError,
        }
    }

    /// Create a permission denied error
    pub fn permission_denied(operation: &str) -> Self {
        AgentError::PermissionDenied(format!("Operation not permitted: {}", operation))
    }

    /// Create a not found error
    pub fn not_found(resource: &str) -> Self {
        AgentError::NotFound(resource.to_string())
    }

    /// Create a validation error
    pub fn validation_error(message: &str) -> Self {
        AgentError::ValidationFailed(message.to_string())
    }

    /// Create an invalid state error
    pub fn invalid_state(message: &str) -> Self {
        AgentError::InvalidState(message.to_string())
    }

    /// Create an internal error
    pub fn internal_error(message: &str) -> Self {
        AgentError::InternalError(message.to_string())
    }
}

impl From<AgentError> for AuraError {
    fn from(err: AgentError) -> Self {
        match err {
            AgentError::EffectSystemError(msg) => AuraError::system_error(msg),
            AgentError::StorageError(e) => AuraError::system_error(e.to_string()),
            AgentError::ConfigError(e) => AuraError::configuration_error(e.to_string()),
            AgentError::AuthError(e) => AuraError::permission_denied(e.to_string()),
            AgentError::SessionError(e) => AuraError::session_error(e.to_string()),
            AgentError::JournalError(e) => AuraError::coordination_failed(e.to_string()),
            AgentError::InitializationFailed(msg) => AuraError::initialization_failed(msg),
            AgentError::NotInitialized => AuraError::invalid_state("Agent not initialized"),
            AgentError::InvalidDeviceId(msg) => AuraError::validation_failed(msg),
            AgentError::InvalidAccountId(msg) => AuraError::validation_failed(msg),
            AgentError::OperationTimeout(secs) => {
                AuraError::timeout(format!("Operation timed out after {}s", secs))
            }
            AgentError::PermissionDenied(msg) => AuraError::permission_denied(msg),
            AgentError::NotFound(resource) => AuraError::not_found(resource),
            AgentError::InvalidState(msg) => AuraError::invalid_state(msg),
            AgentError::ValidationFailed(msg) => AuraError::validation_failed(msg),
            AgentError::InternalError(msg) => AuraError::internal_error(msg),
        }
    }
}

/// Extension trait for converting common errors to AgentError
pub trait IntoAgentError<T> {
    /// Convert error to AgentError with context
    fn agent_error(self, context: &str) -> Result<T>;
}

impl<T, E> IntoAgentError<T> for std::result::Result<T, E>
where
    E: std::fmt::Display,
{
    fn agent_error(self, context: &str) -> Result<T> {
        self.map_err(|e| AgentError::InternalError(format!("{}: {}", context, e)))
    }
}
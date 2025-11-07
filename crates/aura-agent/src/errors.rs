//! Domain error types for the Aura agent
//!
//! This module defines error types specific to the agent domain layer.
//! These errors wrap underlying effect system errors and provide
//! agent-specific context.

use aura_authentication::AuthenticationError;
use aura_protocol::effects::{CryptoError, LedgerError, NetworkError, StorageError, TimeError};
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
    StorageError(#[from] StorageError),

    /// Network operation failed
    #[error("Network error: {0}")]
    NetworkError(#[from] NetworkError),

    /// Cryptographic operation failed
    #[error("Crypto error: {0}")]
    CryptoError(#[from] CryptoError),

    /// Time operation failed
    #[error("Time error: {0}")]
    TimeError(#[from] TimeError),

    /// Ledger operation failed
    #[error("Ledger error: {0}")]
    LedgerError(#[from] LedgerError),

    /// Authentication operation failed
    #[error("Authentication error: {0}")]
    AuthError(#[from] AuthenticationError),

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
            AgentError::StorageError(StorageError::PermissionDenied(_)) => ErrorSeverity::High,
            AgentError::AuthError(_) => ErrorSeverity::High,
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
            AgentError::EffectSystemError(_) => ErrorCode::SystemConfigurationError,
            AgentError::StorageError(_) => ErrorCode::InfraStorageReadFailed,
            AgentError::NetworkError(_) => ErrorCode::InfraTransportConnectionFailed,
            AgentError::CryptoError(_) => ErrorCode::CryptoKeyDerivationFailed,
            AgentError::TimeError(_) => ErrorCode::SystemTimeError,
            AgentError::LedgerError(_) => ErrorCode::ProtocolExecutionFailed,
            AgentError::AuthError(_) => ErrorCode::AgentInsufficientPermissions,
            AgentError::InitializationFailed(_) => ErrorCode::AgentBootstrapRequired,
            AgentError::NotInitialized => ErrorCode::AgentInvalidState,
            AgentError::InvalidDeviceId(_) => ErrorCode::AgentDeviceNotFound,
            AgentError::InvalidAccountId(_) => ErrorCode::AgentAccountNotFound,
            AgentError::OperationTimeout(_) => ErrorCode::ProtocolSessionTimeout,
            AgentError::PermissionDenied(_) => ErrorCode::SystemPermissionDenied,
            AgentError::NotFound(_) => ErrorCode::AgentDeviceNotFound,
            AgentError::InvalidState(_) => ErrorCode::AgentInvalidState,
            AgentError::ValidationFailed(_) => ErrorCode::DataInvalidContext,
            AgentError::InternalError(_) => ErrorCode::GenericUnknown,
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
            AgentError::EffectSystemError(msg) => AuraError::internal_error(msg),
            AgentError::StorageError(e) => AuraError::internal_error(e.to_string()),
            AgentError::NetworkError(e) => AuraError::connection_error(e.to_string()),
            AgentError::CryptoError(e) => AuraError::crypto_operation_failed(e.to_string()),
            AgentError::TimeError(e) => AuraError::system_time_error(e.to_string()),
            AgentError::LedgerError(e) => AuraError::coordination_failed(e.to_string()),
            AgentError::AuthError(e) => AuraError::capability_authorization_error(e.to_string()),
            AgentError::InitializationFailed(msg) => AuraError::configuration_error(msg),
            AgentError::NotInitialized => AuraError::configuration_error("Agent not initialized"),
            AgentError::InvalidDeviceId(msg) => AuraError::serialization_error(msg),
            AgentError::InvalidAccountId(msg) => AuraError::serialization_error(msg),
            AgentError::OperationTimeout(secs) => {
                AuraError::timeout_error(format!("Operation timed out after {}s", secs))
            }
            AgentError::PermissionDenied(msg) => AuraError::capability_authorization_error(msg),
            AgentError::NotFound(resource) => {
                AuraError::internal_error(format!("Not found: {}", resource))
            }
            AgentError::InvalidState(msg) => AuraError::configuration_error(msg),
            AgentError::ValidationFailed(msg) => AuraError::serialization_error(msg),
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

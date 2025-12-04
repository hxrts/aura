//! # Error Types for Intent Dispatch

use thiserror::Error;

/// Errors that can occur when dispatching an intent
#[derive(Debug, Error, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Error))]
pub enum IntentError {
    /// The intent was not authorized by Biscuit policy
    #[error("Unauthorized: {reason}")]
    Unauthorized {
        /// Reason for authorization failure
        reason: String,
    },

    /// The intent failed validation
    #[error("Validation failed: {reason}")]
    ValidationFailed {
        /// Reason for validation failure
        reason: String,
    },

    /// The journal rejected the fact
    #[error("Journal error: {reason}")]
    JournalError {
        /// Reason for journal error
        reason: String,
    },

    /// Internal error during dispatch
    #[error("Internal error: {reason}")]
    InternalError {
        /// Reason for internal error
        reason: String,
    },

    /// The context was not found
    #[error("Context not found: {context_id}")]
    ContextNotFound {
        /// The missing context ID
        context_id: String,
    },

    /// Network error during sync
    #[error("Network error: {reason}")]
    NetworkError {
        /// Reason for network error
        reason: String,
    },

    /// Storage error during persistence
    #[error("Storage error: {reason}")]
    StorageError {
        /// Reason for storage error
        reason: String,
    },
}

impl IntentError {
    /// Create an unauthorized error
    pub fn unauthorized(reason: impl Into<String>) -> Self {
        Self::Unauthorized {
            reason: reason.into(),
        }
    }

    /// Create a validation error
    pub fn validation_failed(reason: impl Into<String>) -> Self {
        Self::ValidationFailed {
            reason: reason.into(),
        }
    }

    /// Create a journal error
    pub fn journal_error(reason: impl Into<String>) -> Self {
        Self::JournalError {
            reason: reason.into(),
        }
    }

    /// Create an internal error
    pub fn internal_error(reason: impl Into<String>) -> Self {
        Self::InternalError {
            reason: reason.into(),
        }
    }

    /// Create a storage error
    pub fn storage_error(reason: impl Into<String>) -> Self {
        Self::StorageError {
            reason: reason.into(),
        }
    }
}

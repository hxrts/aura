use aura_core::AuraError;

/// Structured error type for Layer 3 handlers.
#[derive(Debug, thiserror::Error)]
pub enum Layer3Error {
    /// Unsupported operation invoked through a handler.
    #[error("Unsupported operation: {operation}")]
    UnsupportedOperation {
        /// Name of the unsupported operation.
        operation: &'static str,
    },

    /// Handler failed to execute an operation.
    #[error("Handler failure: {message}")]
    HandlerFailure {
        /// Failure detail for diagnostics.
        message: String,
    },

    /// Invalid input for a handler operation.
    #[error("Invalid input: {message}")]
    InvalidInput {
        /// Validation failure detail.
        message: String,
    },
}

impl Layer3Error {
    /// Create an unsupported-operation error.
    pub fn unsupported(operation: &'static str) -> Self {
        Self::UnsupportedOperation { operation }
    }

    /// Create a handler failure error.
    pub fn handler_failure(message: impl Into<String>) -> Self {
        Self::HandlerFailure {
            message: message.into(),
        }
    }

    /// Create an invalid-input error.
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }
}

impl From<Layer3Error> for AuraError {
    fn from(error: Layer3Error) -> Self {
        match error {
            Layer3Error::UnsupportedOperation { operation } => {
                AuraError::invalid(format!("Unsupported operation: {}", operation))
            }
            Layer3Error::HandlerFailure { message } => AuraError::internal(message),
            Layer3Error::InvalidInput { message } => AuraError::invalid(message),
        }
    }
}

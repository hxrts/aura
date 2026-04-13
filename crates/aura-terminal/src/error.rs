//! Shared error type for Aura Terminal (CLI + TUI).
//!
//! This consolidates disparate ad-hoc errors (anyhow, OpError)
//! into a single user-facing taxonomy so we can format/log/emit through the
//! reactive signal pipeline uniformly.
//!
//! Error categories are defined in `aura_app::ui::types::ErrorCategory` for
//! frontend portability. This module provides the terminal-specific wrapper.

use aura_app::ui::types::ErrorCategory;
use thiserror::Error;

/// Unified result type for terminal-facing code.
pub type TerminalResult<T> = Result<T, TerminalError>;

/// Canonical terminal error taxonomy.
///
/// Each variant maps to an `ErrorCategory` for consistent behavior across frontends.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TerminalError {
    #[error("Invalid input: {0}")]
    Input(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Capability required: {0}")]
    Capability(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Feature unavailable: {0}")]
    NotImplemented(String),
    #[error("Operation failed: {0}")]
    Operation(String),
    #[error("Operation failed [{code}]: {message}")]
    StructuredOperation { code: &'static str, message: String },
}

impl TerminalError {
    #[must_use]
    pub fn structured_operation(code: &'static str, message: impl Into<String>) -> Self {
        Self::StructuredOperation {
            code,
            message: message.into(),
        }
    }

    /// Stable error code for UI and telemetry.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Input(_) => "TERM_INPUT",
            Self::Config(_) => "TERM_CONFIG",
            Self::Capability(_) => "TERM_CAPABILITY",
            Self::NotFound(_) => "TERM_NOT_FOUND",
            Self::Network(_) => "TERM_NETWORK",
            Self::NotImplemented(_) => "TERM_NOT_IMPLEMENTED",
            Self::Operation(_) => "TERM_OPERATION",
            Self::StructuredOperation { code, .. } => code,
        }
    }

    /// Underlying message without the display prefix.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Input(message)
            | Self::Config(message)
            | Self::Capability(message)
            | Self::NotFound(message)
            | Self::Network(message)
            | Self::NotImplemented(message)
            | Self::Operation(message)
            | Self::StructuredOperation { message, .. } => message,
        }
    }

    /// Get the error category for this error.
    ///
    /// Uses the portable `ErrorCategory` for consistent behavior.
    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        match self {
            Self::Input(_) => ErrorCategory::Input,
            Self::Config(_) => ErrorCategory::Config,
            Self::Capability(_) => ErrorCategory::Capability,
            Self::NotFound(_) => ErrorCategory::NotFound,
            Self::Network(_) => ErrorCategory::Network,
            Self::NotImplemented(_) => ErrorCategory::NotImplemented,
            Self::Operation(_) | Self::StructuredOperation { .. } => ErrorCategory::Operation,
        }
    }

    /// Check if this error is user-correctable.
    #[must_use]
    pub fn is_user_correctable(&self) -> bool {
        self.category().is_user_correctable()
    }

    /// Get a resolution hint for this error.
    #[must_use]
    pub fn resolution_hint(&self) -> &'static str {
        self.category().resolution_hint()
    }
}

impl From<aura_core::AuraError> for TerminalError {
    fn from(err: aura_core::AuraError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "terminal")]
impl From<aura_agent::AgentError> for TerminalError {
    fn from(err: aura_agent::AgentError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

impl From<aura_app::ui::types::IntentError> for TerminalError {
    fn from(err: aura_app::ui::types::IntentError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "development")]
impl From<aura_testkit::TestError> for TerminalError {
    fn from(err: aura_testkit::TestError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "development")]
impl From<aura_core::effects::TestingError> for TerminalError {
    fn from(err: aura_core::effects::TestingError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "development")]
impl From<aura_simulator::handlers::effect_composer::SimulationComposerError> for TerminalError {
    fn from(err: aura_simulator::handlers::effect_composer::SimulationComposerError) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "terminal")]
impl From<crate::tui::effects::OpError> for TerminalError {
    fn from(err: crate::tui::effects::OpError) -> Self {
        match err {
            crate::tui::effects::OpError::NotImplemented(s) => TerminalError::NotImplemented(s),
            crate::tui::effects::OpError::InvalidArgument(s) => TerminalError::Input(s),
            crate::tui::effects::OpError::TypedFailure(failure) => {
                TerminalError::StructuredOperation {
                    code: failure.code().as_str(),
                    message: failure.message().to_string(),
                }
            }
            crate::tui::effects::OpError::Failed(s) => {
                if let Some(message) = s.strip_prefix("Permission denied: ") {
                    TerminalError::Capability(message.to_string())
                } else if let Some(message) = s.strip_prefix("Not found: ") {
                    TerminalError::NotFound(message.to_string())
                } else if let Some(message) = s.strip_prefix("Invalid: ") {
                    TerminalError::Input(message.to_string())
                } else if let Some(message) = s.strip_prefix("Network error: ") {
                    TerminalError::Network(message.to_string())
                } else {
                    TerminalError::Operation(s)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::effects::{OpError, OpFailureCode};

    #[test]
    fn typed_operational_error_maps_to_structured_terminal_error() {
        let error = TerminalError::from(OpError::typed(
            OpFailureCode::SendMessage,
            "Failed to send message: missing channel membership",
        ));

        assert_eq!(error.code(), "TUI_SEND_MESSAGE");
        assert_eq!(
            error.message(),
            "Failed to send message: missing channel membership"
        );
        assert_eq!(error.category(), ErrorCategory::Operation);
    }
}

//! Shared error type for Aura Terminal (CLI + TUI).
//!
//! This consolidates disparate ad-hoc errors (anyhow, DispatchError, OpError)
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
#[derive(Debug, Error, Clone)]
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
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Operation failed: {0}")]
    Operation(String),
}

impl TerminalError {
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
            Self::Operation(_) => ErrorCategory::Operation,
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
impl From<crate::tui::effects::DispatchError> for TerminalError {
    fn from(err: crate::tui::effects::DispatchError) -> Self {
        match err {
            crate::tui::effects::DispatchError::PermissionDenied { required } => {
                TerminalError::Capability(format!("requires {}", required.as_biscuit_capability()))
            }
            crate::tui::effects::DispatchError::NotFound { resource } => {
                TerminalError::NotFound(resource)
            }
            crate::tui::effects::DispatchError::InvalidParameter { param, reason } => {
                TerminalError::Input(format!("{param}: {reason}"))
            }
            crate::tui::effects::DispatchError::NotImplemented { command } => {
                TerminalError::NotImplemented(command)
            }
        }
    }
}

#[cfg(feature = "terminal")]
impl From<crate::tui::effects::OpError> for TerminalError {
    fn from(err: crate::tui::effects::OpError) -> Self {
        match err {
            crate::tui::effects::OpError::NotImplemented(s) => TerminalError::NotImplemented(s),
            crate::tui::effects::OpError::InvalidArgument(s) => TerminalError::Input(s),
            crate::tui::effects::OpError::Failed(s) => TerminalError::Operation(s),
        }
    }
}

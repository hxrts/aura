//! Shared error type for Aura Terminal (CLI + TUI).
//!
//! This consolidates disparate ad-hoc errors (anyhow, DispatchError, OpError)
//! into a single user-facing taxonomy so we can format/log/emit through the
//! reactive signal pipeline uniformly.

use thiserror::Error;

/// Unified result type for terminal-facing code.
pub type TerminalResult<T> = Result<T, TerminalError>;

/// Canonical terminal error taxonomy.
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

impl From<anyhow::Error> for TerminalError {
    fn from(err: anyhow::Error) -> Self {
        TerminalError::Operation(err.to_string())
    }
}

#[cfg(feature = "terminal")]
impl From<crate::tui::effects::dispatcher::DispatchError> for TerminalError {
    fn from(err: crate::tui::effects::dispatcher::DispatchError) -> Self {
        match err {
            crate::tui::effects::dispatcher::DispatchError::PermissionDenied { required } => {
                TerminalError::Capability(format!("requires {}", required.as_str()))
            }
            crate::tui::effects::dispatcher::DispatchError::NotFound { resource } => {
                TerminalError::NotFound(resource)
            }
            crate::tui::effects::dispatcher::DispatchError::InvalidParameter { param, reason } => {
                TerminalError::Input(format!("{}: {}", param, reason))
            }
            crate::tui::effects::dispatcher::DispatchError::NotImplemented { command } => {
                TerminalError::NotImplemented(command)
            }
        }
    }
}

#[cfg(feature = "terminal")]
impl From<crate::tui::effects::operational::OpError> for TerminalError {
    fn from(err: crate::tui::effects::operational::OpError) -> Self {
        match err {
            crate::tui::effects::operational::OpError::NotImplemented(s) => {
                TerminalError::NotImplemented(s)
            }
            crate::tui::effects::operational::OpError::InvalidArgument(s) => {
                TerminalError::Input(s)
            }
            crate::tui::effects::operational::OpError::Failed(s) => {
                TerminalError::Operation(s)
            }
        }
    }
}

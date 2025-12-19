//! Build error types for the runtime builder system.

use std::fmt;

/// Error type for runtime builder operations
#[derive(Debug)]
pub enum BuildError {
    /// A required configuration value is missing
    MissingRequired(&'static str),

    /// Invalid configuration value
    InvalidConfig {
        field: &'static str,
        message: String,
    },

    /// Effect initialization failed
    EffectInit {
        effect: &'static str,
        message: String,
    },

    /// Runtime construction failed
    RuntimeConstruction(String),

    /// Authority context error
    AuthorityError(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequired(field) => {
                write!(f, "missing required configuration: {}", field)
            }
            Self::InvalidConfig { field, message } => {
                write!(f, "invalid configuration for '{}': {}", field, message)
            }
            Self::EffectInit { effect, message } => {
                write!(f, "failed to initialize {} effect: {}", effect, message)
            }
            Self::RuntimeConstruction(msg) => {
                write!(f, "runtime construction failed: {}", msg)
            }
            Self::AuthorityError(msg) => {
                write!(f, "authority error: {}", msg)
            }
        }
    }
}

impl std::error::Error for BuildError {}

impl From<BuildError> for crate::AgentError {
    fn from(e: BuildError) -> Self {
        crate::AgentError::config(e.to_string())
    }
}

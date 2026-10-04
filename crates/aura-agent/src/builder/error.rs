//! Build error types for the runtime builder system.

use std::fmt;

/// Error type for runtime builder operations
#[derive(Debug)]
pub enum BuildError {
    /// Runtime construction requires explicit bootstrap identity first
    BootstrapRequired {
        /// Preset/runtime surface requiring bootstrap
        preset: &'static str,
        /// Missing identity field
        identity: &'static str,
    },

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
    /// Required effect initialization retains its native failure.
    EffectInitSource {
        /// Effect whose initialization failed.
        effect: &'static str,
        /// Original process-local error.
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Runtime construction failed
    RuntimeConstruction(String),
    /// Native construction source remains process-local and typed.
    RuntimeConstructionSource(Box<dyn std::error::Error + Send + Sync>),

    /// Authority context error
    AuthorityError(String),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BootstrapRequired { preset, identity } => {
                write!(
                    f,
                    "{preset} bootstrap required: missing explicit {identity}; create or load an account identity first"
                )
            }
            Self::MissingRequired(field) => {
                write!(f, "missing required configuration: {}", field)
            }
            Self::InvalidConfig { field, message } => {
                write!(f, "invalid configuration for '{}': {}", field, message)
            }
            Self::EffectInit { effect, message } => {
                write!(f, "failed to initialize {} effect: {}", effect, message)
            }
            Self::EffectInitSource { effect, source } => {
                write!(f, "failed to initialize {effect} effect: {source}")
            }
            Self::RuntimeConstructionSource(source) => {
                write!(f, "runtime construction failed: {source}")
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

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RuntimeConstructionSource(source) => Some(source.as_ref()),
            Self::EffectInitSource { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<BuildError> for crate::AgentError {
    fn from(e: BuildError) -> Self {
        if matches!(
            &e,
            BuildError::RuntimeConstructionSource(_) | BuildError::EffectInitSource { .. }
        ) {
            crate::AgentError::from(aura_core::AuraError::Internal {
                message: "runtime construction failed".into(),
                source: Some(std::sync::Arc::new(e)),
            })
        } else {
            crate::AgentError::config(e.to_string())
        }
    }
}

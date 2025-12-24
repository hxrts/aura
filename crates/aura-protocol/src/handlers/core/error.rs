//! Factory error types
//!
//! Error handling for factory operations including configuration validation,
//! handler creation, middleware creation, and platform detection.

use thiserror::Error;

use crate::handlers::{EffectType, ExecutionMode};

/// Error type for factory operations
#[derive(Debug, Error)]
pub enum FactoryError {
    /// Configuration validation failed
    #[error("Configuration validation failed: {message}")]
    ConfigurationError {
        /// Description of the configuration error
        message: String,
    },

    /// Handler creation failed
    #[error("Failed to create handler for {effect_type:?}")]
    HandlerCreationFailed {
        /// The effect type that failed to create
        effect_type: EffectType,
        /// Underlying error from handler creation
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Middleware creation failed
    #[error("Failed to create middleware '{middleware_name}'")]
    MiddlewareCreationFailed {
        /// Name of the middleware that failed
        middleware_name: String,
        /// Underlying error from middleware creation
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Platform detection failed
    #[error("Failed to detect platform capabilities")]
    PlatformDetectionFailed {
        /// Underlying error from platform detection
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Required effect type not available
    #[error("Required effect type {effect_type:?} not available")]
    RequiredEffectUnavailable {
        /// The effect type that is required but unavailable
        effect_type: EffectType,
    },

    /// Invalid execution mode for platform
    #[error("Execution mode {mode:?} not supported on this platform")]
    UnsupportedExecutionMode {
        /// The execution mode that is not supported
        mode: ExecutionMode,
    },
}

impl FactoryError {
    /// Create a configuration error
    pub fn config_error(message: impl Into<String>) -> Self {
        Self::ConfigurationError {
            message: message.into(),
        }
    }

    /// Create a handler creation error
    pub fn handler_creation_failed(
        effect_type: EffectType,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerCreationFailed {
            effect_type,
            source: Box::new(source),
        }
    }

    /// Create a middleware creation error
    pub fn middleware_creation_failed(
        middleware_name: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::MiddlewareCreationFailed {
            middleware_name: middleware_name.into(),
            source: Box::new(source),
        }
    }
}

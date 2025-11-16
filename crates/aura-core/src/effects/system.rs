//! System Effects Trait and Error Types
//!
//! This module contains the SystemEffects trait definition and SystemError type
//! for system-level operations including logging, monitoring, and configuration.
//! This trait is part of the core effect system and provides foundational
//! system operations used across all layers.

use async_trait::async_trait;
use std::collections::HashMap;

/// System effect operations error
#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum SystemError {
    /// Service is unavailable
    #[error("System service unavailable")]
    ServiceUnavailable,

    /// Invalid configuration parameter
    #[error("Invalid configuration: {key}={value}")]
    InvalidConfiguration { key: String, value: String },

    /// Operation failed
    #[error("System operation failed: {message}")]
    OperationFailed { message: String },

    /// Permission denied
    #[error("Permission denied for operation: {operation}")]
    PermissionDenied { operation: String },

    /// Resource not found
    #[error("Resource not found: {resource}")]
    ResourceNotFound { resource: String },

    /// Resource exhausted
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },
}

impl From<crate::AuraError> for SystemError {
    fn from(e: crate::AuraError) -> Self {
        SystemError::OperationFailed {
            message: e.to_string(),
        }
    }
}

/// System effects interface for logging, monitoring, and configuration
///
/// This trait provides system-level operations for:
/// - Logging and audit trails
/// - System monitoring and health checks
/// - Configuration management
/// - System metrics and statistics
/// - Component lifecycle management
#[async_trait]
pub trait SystemEffects: Send + Sync {
    /// Log a message at the specified level
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError>;

    /// Log a message with additional context
    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError>;

    /// Get system information and status
    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError>;

    /// Set a configuration value
    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError>;

    /// Get a configuration value
    async fn get_config(&self, key: &str) -> Result<String, SystemError>;

    /// Perform a health check
    async fn health_check(&self) -> Result<bool, SystemError>;

    /// Get system metrics
    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError>;

    /// Restart a system component
    async fn restart_component(&self, component: &str) -> Result<(), SystemError>;

    /// Shutdown the system gracefully
    async fn shutdown(&self) -> Result<(), SystemError>;
}
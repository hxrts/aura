//! System-level Effect Traits
//!
//! Traits for system-level operations like logging, metrics, and platform integration.

use aura_core::{AuraError, AuraResult};
use async_trait::async_trait;

/// System error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum SystemError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("System error: {0}")]
    Other(String),
}

impl From<SystemError> for AuraError {
    fn from(err: SystemError) -> Self {
        AuraError::internal(err.to_string())
    }
}

/// System effects for platform integration
#[async_trait]
pub trait SystemEffects: Send + Sync {
    /// Log a message
    async fn log(&self, level: LogLevel, message: &str) -> AuraResult<()>;

    /// Get system info
    async fn get_system_info(&self) -> AuraResult<SystemInfo>;
}

/// Log level
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// System information
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub os: String,
    pub version: String,
    pub architecture: String,
}

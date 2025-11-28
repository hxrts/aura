//! Console effect interface for logging and debug output
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing logging and terminal output
//!
//! This is an infrastructure effect that must be implemented in `aura-effects`
//! with stateless handlers. Domain crates should not implement this trait directly.

use crate::AuraError;
use async_trait::async_trait;

/// Pure trait for console/logging operations
#[async_trait]
pub trait ConsoleEffects: Send + Sync {
    /// Log an info message
    async fn log_info(&self, message: &str) -> Result<(), AuraError>;

    /// Log a warning message
    async fn log_warn(&self, message: &str) -> Result<(), AuraError>;

    /// Log an error message
    async fn log_error(&self, message: &str) -> Result<(), AuraError>;

    /// Log a debug message
    async fn log_debug(&self, message: &str) -> Result<(), AuraError>;
}

/// Blanket implementation for Arc<T> where T: ConsoleEffects
#[async_trait]
impl<T: ConsoleEffects + ?Sized> ConsoleEffects for std::sync::Arc<T> {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        (**self).log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        (**self).log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        (**self).log_error(message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        (**self).log_debug(message).await
    }
}

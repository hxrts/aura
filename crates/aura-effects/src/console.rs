//! Console effect handlers
//!
//! This module provides standard implementations of the `ConsoleEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use async_trait::async_trait;
use aura_core::{effects::ConsoleEffects, AuraError};

/// Real console handler using actual tracing
#[derive(Debug, Clone)]
pub struct RealConsoleHandler;

impl Default for RealConsoleHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RealConsoleHandler {
    /// Create a new real console handler
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ConsoleEffects for RealConsoleHandler {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        tracing::info!("{}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        tracing::warn!("{}", message);
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        tracing::error!("{}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        tracing::debug!("{}", message);
        Ok(())
    }
}

// MockConsoleHandler moved to aura-testkit

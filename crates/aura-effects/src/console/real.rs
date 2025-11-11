//! Real console effect handler for production use

use aura_core::effects::ConsoleEffects;
use async_trait::async_trait;

/// Real console handler for production use
#[derive(Debug, Clone, Default)]
pub struct RealConsoleHandler;

impl RealConsoleHandler {
    /// Create a new real console handler
    pub fn new() -> Self {
        Self
    }

}

#[async_trait]
impl ConsoleEffects for RealConsoleHandler {
    async fn log_error(&self, message: &str) -> Result<(), aura_core::AuraError> {
        tracing::error!("{}", message);
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), aura_core::AuraError> {
        tracing::warn!("{}", message);
        Ok(())
    }

    async fn log_info(&self, message: &str) -> Result<(), aura_core::AuraError> {
        tracing::info!("{}", message);
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), aura_core::AuraError> {
        tracing::debug!("{}", message);
        Ok(())
    }

}
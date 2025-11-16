//! Output Effect Handler Implementation

use super::OutputEffects;
use aura_core::AuraResult;
use async_trait::async_trait;
use serde_json::Value;

/// Output effect handler
pub struct OutputEffectHandler<E> {
    /// The wrapped effect implementation
    inner: E,
}

impl<E> OutputEffectHandler<E> {
    /// Create a new output effect handler
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<E> OutputEffects for OutputEffectHandler<E>
where
    E: crate::ConsoleEffects + Send + Sync,
{
    async fn display(&self, content: &str) {
        let _ = self.inner.log_info(content).await;
    }

    async fn display_error(&self, error: &str) {
        let _ = self.inner.log_error(&format!("ERROR: {}", error)).await;
    }

    async fn display_success(&self, message: &str) {
        let _ = self.inner.log_info(&format!("SUCCESS: {}", message)).await;
    }

    async fn display_progress(&self, message: &str, progress: f64) {
        let percentage = (progress * 100.0).clamp(0.0, 100.0);
        let _ = self
            .inner
            .log_info(&format!("{}: {:.1}%", message, percentage))
            .await;
    }

    async fn format_json(&self, data: &Value) -> AuraResult<String> {
        serde_json::to_string_pretty(data)
            .map_err(|e| aura_core::AuraError::invalid(format!("Failed to format JSON: {}", e)))
    }

    async fn format_text(&self, data: &str) -> String {
        // Simple text formatting - could be enhanced with proper formatting
        data.to_string()
    }
}

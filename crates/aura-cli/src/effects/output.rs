//! Output Effect Implementations

use super::OutputEffects;
use anyhow::Result;
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
    E: aura_protocol::ConsoleEffects + Send + Sync,
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
        let percentage = (progress * 100.0).min(100.0).max(0.0);
        self.inner
            .log_info(&format!("{}: {:.1}%", message, percentage))
            .await;
    }

    async fn format_json(&self, data: &Value) -> Result<String> {
        serde_json::to_string_pretty(data)
            .map_err(|e| anyhow::anyhow!("Failed to format JSON: {}", e))
    }

    async fn format_text(&self, data: &str) -> String {
        // Simple text formatting - could be enhanced with proper formatting
        data.to_string()
    }
}

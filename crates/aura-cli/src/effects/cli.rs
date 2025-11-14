//! CLI Effect Implementations
//!
//! Concrete implementations of CLI effects using core effect composition.

use super::CliEffects;
use crate::effects::Result;
use async_trait::async_trait;
use std::path::Path;

/// CLI effect handler that composes core effects
pub struct CliEffectHandler<E> {
    /// The wrapped effect implementation
    inner: E,
}

impl<E> CliEffectHandler<E> {
    /// Create a new CLI effect handler
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<E> CliEffects for CliEffectHandler<E>
where
    E: aura_protocol::ConsoleEffects
        + aura_protocol::StorageEffects
        + aura_protocol::TimeEffects
        + Send
        + Sync,
{
    async fn log_info(&self, message: &str) {
        let _ = self.inner.log_info(&format!("INFO: {}", message)).await;
    }

    async fn log_warning(&self, message: &str) {
        let _ = self.inner.log_warn(&format!("WARN: {}", message)).await;
    }

    async fn log_error(&self, message: &str) {
        let _ = self.inner.log_error(&format!("ERROR: {}", message)).await;
    }

    async fn create_dir_all(&self, path: &Path) -> Result<()> {
        // Use storage effects for file operations
        let path_str = path.display().to_string();

        // Check if directory already exists
        if let Ok(existing) = self.inner.retrieve(&path_str).await {
            if existing.is_some() {
                return Ok(());
            }
        }

        // Create directory marker in storage
        self.inner
            .store(&path_str, b"directory".to_vec())
            .await
            .map_err(|e| {
                aura_core::AuraError::invalid(format!("Failed to create directory: {}", e))
            })
    }

    async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        let path_str = path.display().to_string();
        self.inner
            .store(&path_str, content.to_vec())
            .await
            .map_err(|e| aura_core::AuraError::invalid(format!("Failed to write file: {}", e)))
    }

    async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        let path_str = path.display().to_string();
        match self.inner.retrieve(&path_str).await {
            Ok(Some(data)) => Ok(data),
            Ok(None) => Err(aura_core::AuraError::not_found(format!(
                "File not found: {}",
                path.display()
            ))),
            Err(e) => Err(aura_core::AuraError::invalid(format!(
                "Failed to read file: {}",
                e
            ))),
        }
    }

    async fn file_exists(&self, path: &Path) -> bool {
        let path_str = path.display().to_string();
        self.inner.exists(&path_str).await.unwrap_or_default()
    }

    async fn format_output(&self, data: &str) -> String {
        // Simple formatting TODO fix - For now
        data.to_string()
    }

    async fn current_timestamp(&self) -> u64 {
        self.inner.current_timestamp().await
    }
}

//! CLI Effect Implementations
//!
//! Concrete implementations of CLI effects using core effect composition.

use super::{CliEffects, CliConfig};
use async_trait::async_trait;
use anyhow::Result;
use std::path::PathBuf;

/// CLI effect handler that composes core effects
pub struct CliEffectHandler<E> {
    inner: E,
}

impl<E> CliEffectHandler<E> {
    pub fn new(inner: E) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl<E> CliEffects for CliEffectHandler<E>
where
    E: aura_protocol::ConsoleEffects + 
       aura_protocol::StorageEffects + 
       aura_protocol::TimeEffects + 
       Send + Sync,
{
    async fn log_info(&self, message: &str) {
        self.inner.log_info(&format!("INFO: {}", message), &[]);
    }
    
    async fn log_warning(&self, message: &str) {
        self.inner.log_warn(&format!("WARN: {}", message), &[]);
    }
    
    async fn log_error(&self, message: &str) {
        self.inner.log_error(&format!("ERROR: {}", message), &[]);
    }
    
    async fn create_dir_all(&self, path: &PathBuf) -> Result<()> {
        // Use storage effects for file operations
        let path_str = path.display().to_string();
        
        // Check if directory already exists
        if let Ok(existing) = self.inner.retrieve(&path_str).await {
            if existing.is_some() {
                return Ok(());
            }
        }
        
        // Create directory marker in storage
        self.inner.store(&path_str, b"directory".to_vec()).await
            .map_err(|e| anyhow::anyhow!("Failed to create directory: {}", e))
    }
    
    async fn write_file(&self, path: &PathBuf, content: &[u8]) -> Result<()> {
        let path_str = path.display().to_string();
        self.inner.store(&path_str, content.to_vec()).await
            .map_err(|e| anyhow::anyhow!("Failed to write file: {}", e))
    }
    
    async fn read_file(&self, path: &PathBuf) -> Result<Vec<u8>> {
        let path_str = path.display().to_string();
        match self.inner.retrieve(&path_str).await {
            Ok(Some(data)) => Ok(data),
            Ok(None) => Err(anyhow::anyhow!("File not found: {}", path.display())),
            Err(e) => Err(anyhow::anyhow!("Failed to read file: {}", e)),
        }
    }
    
    async fn file_exists(&self, path: &PathBuf) -> bool {
        let path_str = path.display().to_string();
        match self.inner.exists(&path_str).await {
            Ok(exists) => exists,
            Err(_) => false,
        }
    }
    
    async fn format_output(&self, data: &str) -> String {
        // Simple formatting for now
        data.to_string()
    }
    
    async fn current_timestamp(&self) -> u64 {
        self.inner.current_timestamp().await
    }
}
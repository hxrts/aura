//! Filesystem storage handler for production

use crate::effects::{StorageEffects, StorageError, StorageStats};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;

/// Filesystem storage handler for production use
pub struct FilesystemStorageHandler {
    base_path: PathBuf,
}

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler
    pub fn new(base_path: PathBuf) -> Result<Self, StorageError> {
        Ok(Self { base_path })
    }

    fn key_to_path(&self, key: &str) -> PathBuf {
        // Simple key to path mapping - in production this would be more sophisticated
        self.base_path.join(key.replace('/', "_"))
    }
}

#[async_trait]
impl StorageEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let path = self.key_to_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| StorageError::WriteFailed(format!("I/O error: {}", e)))?;
        }
        fs::write(path, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("I/O error: {}", e)))?;
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let path = self.key_to_path(key);
        match fs::read(path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::ReadFailed(format!("I/O error: {}", e))),
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let path = self.key_to_path(key);
        match fs::remove_file(path).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(StorageError::DeleteFailed(format!("I/O error: {}", e))),
        }
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let mut keys = Vec::new();
        let mut read_dir = fs::read_dir(&self.base_path).await
            .map_err(|e| StorageError::ListFailed(format!("Failed to read directory: {}", e)))?;
        
        while let Some(entry) = read_dir.next_entry().await
            .map_err(|e| StorageError::ListFailed(format!("Failed to read entry: {}", e)))? {
            
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_file() {
                    if let Some(file_name) = entry.file_name().to_str() {
                        let key = file_name.to_string();
                        if let Some(prefix_str) = prefix {
                            if key.starts_with(prefix_str) {
                                keys.push(key);
                            }
                        } else {
                            keys.push(key);
                        }
                    }
                }
            }
        }
        
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let path = self.key_to_path(key);
        Ok(path.exists())
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        for (key, value) in pairs {
            self.store(&key, value).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = self.retrieve(key).await? {
                result.insert(key.clone(), value);
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        // Dangerous operation - only proceed if directory exists and is under our control
        if !self.base_path.exists() {
            return Ok(()); // Nothing to clear
        }
        
        // Safety check: ensure we're only clearing our own storage directory
        if !self.base_path.to_string_lossy().contains("aura") {
            return Err(StorageError::DeleteFailed(
                "Refusing to clear directory that doesn't appear to be Aura storage".to_string()
            ));
        }
        
        let mut read_dir = fs::read_dir(&self.base_path).await
            .map_err(|e| StorageError::DeleteFailed(format!("Failed to read directory: {}", e)))?;
        
        while let Some(entry) = read_dir.next_entry().await
            .map_err(|e| StorageError::DeleteFailed(format!("Failed to read entry: {}", e)))? {
            
            if let Ok(file_type) = entry.file_type().await {
                if file_type.is_file() {
                    fs::remove_file(entry.path()).await
                        .map_err(|e| StorageError::DeleteFailed(format!("Failed to remove file: {}", e)))?;
                }
            }
        }
        
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats {
            key_count: 0,
            total_size: 0,
            available_space: None,
            backend_type: "filesystem".to_string(),
        })
    }
}

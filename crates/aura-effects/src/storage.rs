//! Layer 3: Storage Effect Handlers - Production Only
//!
//! Stateless single-party implementations of StorageEffects from aura-core (Layer 1).
//! These handlers provide production storage operations delegating to filesystem or cloud APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use std::path::PathBuf;
use tokio::fs;
use tokio::fs::DirEntry;

/// Filesystem-based storage handler for production use
///
/// This handler stores data as files on the local filesystem.
/// It is stateless and delegates all storage operations to the filesystem.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    /// Base directory for storage files
    base_path: PathBuf,
}

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Create a new filesystem storage handler with default path
    pub fn with_default_path() -> Self {
        Self::new(PathBuf::from("./storage"))
    }
}

#[async_trait]
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        if key.is_empty() {
            return Err(StorageError::InvalidKey {
                reason: "Key cannot be empty".to_string(),
            });
        }

        let file_path = self.base_path.join(format!("{}.dat", key));
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                StorageError::WriteFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        fs::write(&file_path, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("Failed to write file: {}", e)))?;

        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));

        if !file_path.exists() {
            return Ok(None);
        }

        let data = fs::read(&file_path)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Failed to read file: {}", e)))?;

        Ok(Some(data))
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));

        if !file_path.exists() {
            return Ok(false);
        }

        fs::remove_file(&file_path)
            .await
            .map_err(|e| StorageError::DeleteFailed(format!("Failed to remove file: {}", e)))?;

        Ok(true)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        // Keys may contain path separators (e.g. `journal/facts/...`), so we must
        // traverse the directory tree recursively and strip the `.dat` suffix
        // from persisted filenames.
        let mut keys = Vec::new();
        let mut stack: Vec<PathBuf> = vec![self.base_path.clone()];

        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Failed to read directory: {}",
                        e
                    )))
                }
            };

            while let Some(entry) = entries.next_entry().await.map_err(|e| {
                StorageError::ReadFailed(format!("Failed to read directory entry: {}", e))
            })? {
                Self::visit_entry_for_keys(&self.base_path, entry, prefix, &mut stack, &mut keys)
                    .await?;
            }
        }

        keys.sort();
        Ok(keys)
    }

}

#[async_trait]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let file_path = self.base_path.join(format!("{}.dat", key));
        Ok(file_path.exists())
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        for (k, v) in pairs {
            self.store(&k, v).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        let mut out = std::collections::HashMap::new();
        for key in keys {
            if let Some(val) = self.retrieve(key).await? {
                out.insert(key.clone(), val);
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        match fs::remove_dir_all(&self.base_path).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(StorageError::DeleteFailed(format!(
                    "Failed to remove storage directory: {}",
                    e
                )))
            }
        }

        fs::create_dir_all(&self.base_path).await.map_err(|e| {
            StorageError::WriteFailed(format!("Failed to recreate storage directory: {}", e))
        })?;
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let mut key_count: u64 = 0;
        let mut total_size: u64 = 0;

        let mut stack: Vec<PathBuf> = vec![self.base_path.clone()];
        while let Some(dir) = stack.pop() {
            let mut entries = match fs::read_dir(&dir).await {
                Ok(e) => e,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Failed to read directory: {}",
                        e
                    )))
                }
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let file_type = match entry.file_type().await {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };

                if file_type.is_dir() {
                    stack.push(entry.path());
                    continue;
                }

                if !file_type.is_file() {
                    continue;
                }

                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("dat") {
                    continue;
                }

                key_count += 1;
                if let Ok(metadata) = entry.metadata().await {
                    total_size = total_size.saturating_add(metadata.len());
                }
            }
        }

        Ok(StorageStats {
            key_count,
            total_size,
            available_space: None,
            backend_type: "filesystem".to_string(),
        })
    }
}

impl FilesystemStorageHandler {
    async fn visit_entry_for_keys(
        base: &PathBuf,
        entry: DirEntry,
        prefix: Option<&str>,
        stack: &mut Vec<PathBuf>,
        keys: &mut Vec<String>,
    ) -> Result<(), StorageError> {
        let file_type = entry.file_type().await.map_err(|e| {
            StorageError::ReadFailed(format!("Failed to stat directory entry: {}", e))
        })?;
        let path = entry.path();

        if file_type.is_dir() {
            stack.push(path);
            return Ok(());
        }
        if !file_type.is_file() {
            return Ok(());
        }

        if path.extension().and_then(|e| e.to_str()) != Some("dat") {
            return Ok(());
        }

        let rel = path.strip_prefix(base).map_err(|e| {
            StorageError::ReadFailed(format!("Failed to compute relative key path: {}", e))
        })?;
        let rel = rel.with_extension("");
        let mut key = rel.to_string_lossy().to_string();
        if std::path::MAIN_SEPARATOR != '/' {
            key = key.replace(std::path::MAIN_SEPARATOR, "/");
        }

        if let Some(prefix) = prefix {
            if key.starts_with(prefix) {
                keys.push(key);
            }
        } else {
            keys.push(key);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_filesystem_storage_handler() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().to_path_buf());

        // Test store and retrieve
        let key = "test_key";
        let value = b"test_value".to_vec();

        handler.store(key, value.clone()).await.unwrap();
        let retrieved = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Test exists
        assert!(handler.exists(key).await.unwrap());

        // Test remove
        assert!(handler.remove(key).await.unwrap());
        assert!(!handler.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_and_retrieve() {
        let temp_dir = TempDir::new().unwrap();
        let handler = FilesystemStorageHandler::new(temp_dir.path().to_path_buf());

        let key = "test_key";
        let data = b"test_data".to_vec();

        // Store data
        handler.store(key, data.clone()).await.unwrap();

        // Verify it exists
        let retrieved = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(data));

        // Delete it
        let was_deleted = handler.remove(key).await.unwrap();
        assert!(was_deleted);

        // Verify it's gone
        let retrieved_after = handler.retrieve(key).await.unwrap();
        assert_eq!(retrieved_after, None);
    }
}

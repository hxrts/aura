//! WASM storage shim for environments without filesystem access.
//!
//! This preserves the Layer 3 storage surface on wasm targets while making
//! unsupported filesystem operations explicit at runtime.

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use std::path::PathBuf;

/// Filesystem storage shim for wasm targets.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    #[allow(dead_code)]
    base_path: PathBuf,
}

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler shim.
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Create a new filesystem storage handler with default path.
    pub fn with_default_path() -> Self {
        Self::new(PathBuf::from("./storage"))
    }

    fn unsupported() -> StorageError {
        StorageError::ConfigurationError {
            reason: "Filesystem storage is not supported on wasm32".to_string(),
        }
    }
}

#[async_trait]
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, _key: &str, _value: Vec<u8>) -> Result<(), StorageError> {
        Err(Self::unsupported())
    }

    async fn retrieve(&self, _key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Err(Self::unsupported())
    }

    async fn remove(&self, _key: &str) -> Result<bool, StorageError> {
        Err(Self::unsupported())
    }

    async fn list_keys(&self, _prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        Err(Self::unsupported())
    }
}

#[async_trait]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, _key: &str) -> Result<bool, StorageError> {
        Ok(false)
    }

    async fn store_batch(
        &self,
        _pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        Err(Self::unsupported())
    }

    async fn retrieve_batch(
        &self,
        _keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, StorageError> {
        Err(Self::unsupported())
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        Err(Self::unsupported())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        Ok(StorageStats {
            key_count: 0,
            total_size: 0,
            available_space: None,
            backend_type: "wasm_stub".to_string(),
        })
    }
}

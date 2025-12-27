//! Mock storage effect handlers for testing
//!
//! This module contains stateful storage handlers that were moved from aura-effects
//! to fix architectural violations. These handlers use Arc<RwLock<>> for shared
//! storage state in testing scenarios.

use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::{
    StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats,
};
use std::collections::HashMap;
use std::sync::Arc;

/// Memory storage handler for testing
#[derive(Debug, Clone)]
pub struct MemoryStorageHandler {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl Default for MemoryStorageHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorageHandler {
    /// Create a new memory storage handler
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with initial data
    pub fn with_data(data: HashMap<String, Vec<u8>>) -> Self {
        Self {
            data: Arc::new(RwLock::new(data)),
        }
    }

    /// Get the number of stored keys (for testing)
    pub fn len(&self) -> usize {
        self.data.try_read().map(|g| g.len()).unwrap_or(0)
    }

    /// Check if storage is empty (for testing)
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get all stored data (for testing)
    pub async fn get_all_data(&self) -> HashMap<String, Vec<u8>> {
        self.data.read().await.clone()
    }
}

#[async_trait]
impl StorageCoreEffects for MemoryStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let mut data = self.data.write().await;
        data.insert(key.to_string(), value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let data = self.data.read().await;
        Ok(data.get(key).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let mut data = self.data.write().await;
        Ok(data.remove(key).is_some())
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let data = self.data.read().await;
        let keys = if let Some(prefix) = prefix {
            data.keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            data.keys().cloned().collect()
        };
        Ok(keys)
    }

}

#[async_trait]
impl StorageExtendedEffects for MemoryStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let data = self.data.read().await;
        Ok(data.contains_key(key))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut data = self.data.write().await;
        for (key, value) in pairs {
            data.insert(key, value);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let data = self.data.read().await;
        let mut result = HashMap::new();
        for key in keys {
            if let Some(value) = data.get(key) {
                result.insert(key.clone(), value.clone());
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut data = self.data.write().await;
        data.clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let data = self.data.read().await;
        let total_size = data.values().map(|v| v.len() as u64).sum();
        Ok(StorageStats {
            key_count: data.len() as u64,
            total_size,
            available_space: None,
            backend_type: "memory".to_string(),
        })
    }
}

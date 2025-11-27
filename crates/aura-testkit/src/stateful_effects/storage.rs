//! Mock storage effect handlers for testing
//!
//! This module contains stateful storage handlers that were moved from aura-effects
//! to fix architectural violations. These handlers use Arc<RwLock<>> for shared
//! storage state in testing scenarios.

use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::effects::{StorageEffects, StorageError, StorageStats};
use aura_core::ChunkId;
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
impl StorageEffects for MemoryStorageHandler {
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

/// Encrypted storage handler for testing with chunk-based addressing
#[derive(Debug)]
pub struct EncryptedStorageHandler {
    /// Internal memory storage
    storage: Arc<RwLock<HashMap<ChunkId, Vec<u8>>>>,
}

impl EncryptedStorageHandler {
    /// Create a new encrypted storage handler
    ///
    /// Note: The `storage_path` and `encryption_key` parameters are currently ignored
    /// in the memory implementation. In the future, this would create a filesystem-backed
    /// storage with optional encryption at the message layer (not storage layer).
    pub fn new(_storage_path: String, _encryption_key: Option<Vec<u8>>) -> Self {
        Self {
            storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with existing chunk data
    pub fn with_chunks(chunks: HashMap<ChunkId, Vec<u8>>) -> Self {
        Self {
            storage: Arc::new(RwLock::new(chunks)),
        }
    }

    /// Get information about the storage configuration
    pub fn stack_info(&self) -> HashMap<String, String> {
        let storage_guard = self.storage.try_read();
        let (chunk_count, total_bytes) = storage_guard
            .map(|g| {
                (
                    g.len() as u64,
                    g.values().map(|v| v.len() as u64).sum::<u64>(),
                )
            })
            .unwrap_or((0, 0));

        let mut info = HashMap::new();
        info.insert("backend_type".to_string(), "encrypted_memory".to_string());
        info.insert("chunk_count".to_string(), chunk_count.to_string());
        info.insert("total_bytes".to_string(), total_bytes.to_string());
        info
    }

    /// Get number of chunks stored
    pub async fn chunk_count(&self) -> usize {
        self.storage.read().await.len()
    }

    /// Store a chunk by ID
    pub async fn store_chunk(&self, id: ChunkId, data: Vec<u8>) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;
        storage.insert(id, data);
        Ok(())
    }

    /// Retrieve a chunk by ID
    pub async fn retrieve_chunk(&self, id: &ChunkId) -> Result<Option<Vec<u8>>, StorageError> {
        let storage = self.storage.read().await;
        Ok(storage.get(id).cloned())
    }

    /// Remove a chunk by ID
    pub async fn remove_chunk(&self, id: &ChunkId) -> Result<bool, StorageError> {
        let mut storage = self.storage.write().await;
        Ok(storage.remove(id).is_some())
    }

    /// List all chunk IDs
    pub async fn list_chunks(&self) -> Result<Vec<ChunkId>, StorageError> {
        let storage = self.storage.read().await;
        Ok(storage.keys().cloned().collect())
    }

    /// Clear all chunks
    pub async fn clear_chunks(&self) {
        self.storage.write().await.clear();
    }
}

#[async_trait]
impl StorageEffects for EncryptedStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        storage.insert(chunk_id, value);
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let storage = self.storage.read().await;
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.get(&chunk_id).cloned())
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let mut storage = self.storage.write().await;
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.remove(&chunk_id).is_some())
    }

    async fn list_keys(&self, _prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        // For chunk-based storage, list_keys doesn't make sense since keys are derived from chunks
        Err(StorageError::ListFailed(
            "list_keys not supported in chunk-based storage".to_string(),
        ))
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let storage = self.storage.read().await;
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.contains_key(&chunk_id))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;
        for (key, value) in pairs {
            let chunk_id = ChunkId::from_bytes(key.as_bytes());
            storage.insert(chunk_id, value);
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let storage = self.storage.read().await;
        let mut result = HashMap::new();
        for key in keys {
            let chunk_id = ChunkId::from_bytes(key.as_bytes());
            if let Some(data) = storage.get(&chunk_id) {
                result.insert(key.clone(), data.clone());
            }
        }
        Ok(result)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;
        storage.clear();
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let storage = self.storage.read().await;
        let total_size: u64 = storage.values().map(|v| v.len() as u64).sum();

        Ok(StorageStats {
            key_count: storage.len() as u64,
            total_size,
            available_space: None,
            backend_type: "encrypted_memory".to_string(),
        })
    }
}

//! Storage handler using aura-store's TODO fix - Simplified effect interface

use crate::effects::{StorageEffects, StorageError, StorageStats};
use async_trait::async_trait;
use aura_store::{ChunkId, MemoryStorage, StorageEffects as AuraStorageEffects};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Storage handler using aura-store's pure effect interface
///
/// This wraps aura-store's StorageEffects trait to provide the protocol-level
/// StorageEffects interface. The actual storage implementation (memory, filesystem, etc.)
/// is chosen at construction time.
pub struct EncryptedStorageHandler {
    /// Storage implementation using aura-store's effect interface
    storage: Arc<RwLock<MemoryStorage>>,
}

impl EncryptedStorageHandler {
    /// Create a new storage handler with in-memory storage
    ///
    /// Note: The `storage_path` and `encryption_key` parameters are currently ignored
    /// as we use MemoryStorage. In the future, this would create a filesystem-backed
    /// storage with optional encryption at the message layer (not storage layer).
    pub fn new(_storage_path: String, _encryption_key: Option<Vec<u8>>) -> Self {
        Self {
            storage: Arc::new(RwLock::new(MemoryStorage::new())),
        }
    }

    /// Create with an existing storage implementation
    pub fn with_storage(storage: MemoryStorage) -> Self {
        Self {
            storage: Arc::new(RwLock::new(storage)),
        }
    }

    /// Get information about the storage configuration
    pub async fn stack_info(&self) -> HashMap<String, String> {
        let storage = self.storage.read().await;
        let stats = storage
            .stats()
            .await
            .unwrap_or_else(|_| aura_store::StorageStats {
                chunk_count: 0,
                total_bytes: 0,
                backend_type: "unknown".to_string(),
            });

        let mut info = HashMap::new();
        info.insert("backend_type".to_string(), stats.backend_type);
        info.insert("chunk_count".to_string(), stats.chunk_count.to_string());
        info.insert("total_bytes".to_string(), stats.total_bytes.to_string());
        info
    }
}

#[async_trait]
impl StorageEffects for EncryptedStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;
        let chunk_id = ChunkId::from_string(key);

        storage
            .store_chunk(chunk_id, value)
            .await
            .map_err(|e| StorageError::WriteFailed(format!("Storage error: {}", e)))
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let storage = self.storage.read().await;
        let chunk_id = ChunkId::from_string(key);

        storage
            .fetch_chunk(&chunk_id)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Storage error: {}", e)))
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        let mut storage = self.storage.write().await;
        let chunk_id = ChunkId::from_string(key);

        // Check if it exists first
        let exists = storage
            .chunk_exists(&chunk_id)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Storage error: {}", e)))?;

        if exists {
            storage
                .delete_chunk(&chunk_id)
                .await
                .map_err(|e| StorageError::DeleteFailed(format!("Storage error: {}", e)))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        // Note: MemoryStorage doesn't have a list_keys method in the TODO fix - Simplified API
        // This would need to be implemented at a higher layer or added as an extension
        // TODO fix - For now, we return an error indicating this isn't supported
        let _ = prefix;
        Err(StorageError::ListFailed(
            "list_keys not supported in TODO fix - Simplified storage API - use search layer instead"
                .to_string(),
        ))
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let storage = self.storage.read().await;
        let chunk_id = ChunkId::from_string(key);

        storage
            .chunk_exists(&chunk_id)
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Storage error: {}", e)))
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        let mut storage = self.storage.write().await;

        for (key, value) in pairs {
            let chunk_id = ChunkId::from_string(key);
            storage
                .store_chunk(chunk_id, value)
                .await
                .map_err(|e| StorageError::WriteFailed(format!("Batch store error: {}", e)))?;
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
            let chunk_id = ChunkId::from_string(key);
            match storage.fetch_chunk(&chunk_id).await {
                Ok(Some(data)) => {
                    result.insert(key.clone(), data);
                }
                Ok(None) => {
                    // Skip missing keys in batch operations
                    continue;
                }
                Err(e) => {
                    return Err(StorageError::ReadFailed(format!(
                        "Batch retrieve error for key {}: {}",
                        key, e
                    )));
                }
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
        let aura_stats = storage
            .stats()
            .await
            .map_err(|e| StorageError::ReadFailed(format!("Failed to get stats: {}", e)))?;

        Ok(StorageStats {
            key_count: aura_stats.chunk_count,
            total_size: aura_stats.total_bytes,
            available_space: None,
            backend_type: aura_stats.backend_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_encrypted_storage_basic_operations() {
        // Create storage (encryption_key parameter ignored in current implementation)
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Test basic operations
        let key = "test_key";
        let value = b"test_value".to_vec();

        // Store
        storage.store(key, value.clone()).await.unwrap();

        // Retrieve
        let retrieved = storage.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Exists
        assert!(storage.exists(key).await.unwrap());

        // Remove
        assert!(storage.remove(key).await.unwrap());
        assert!(!storage.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_encrypted_storage_batch_operations() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Prepare batch data
        let mut batch_data = HashMap::new();
        batch_data.insert("key1".to_string(), b"value1".to_vec());
        batch_data.insert("key2".to_string(), b"value2".to_vec());
        batch_data.insert("key3".to_string(), b"value3".to_vec());

        // Store batch
        storage.store_batch(batch_data.clone()).await.unwrap();

        // Retrieve batch
        let keys: Vec<String> = batch_data.keys().cloned().collect();
        let retrieved = storage.retrieve_batch(&keys).await.unwrap();

        assert_eq!(retrieved.len(), 3);
        for (key, expected_value) in batch_data {
            assert_eq!(retrieved.get(&key).unwrap(), &expected_value);
        }
    }

    #[tokio::test]
    async fn test_encrypted_storage_stats() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Initially empty
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.key_count, 0);

        // Add some data
        storage.store("key1", vec![1, 2, 3]).await.unwrap();
        storage.store("key2", vec![4, 5, 6, 7]).await.unwrap();

        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.key_count, 2);
        assert_eq!(stats.total_size, 7); // 3 + 4 bytes
        assert_eq!(stats.backend_type, "memory");
    }

    #[tokio::test]
    async fn test_storage_stack_info() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        let stack_info = storage.stack_info().await;
        assert!(stack_info.contains_key("backend_type"));
        assert!(stack_info.contains_key("chunk_count"));
        assert!(stack_info.contains_key("total_bytes"));
    }

    #[tokio::test]
    async fn test_clear_all() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), None);

        // Add data
        storage.store("key1", b"value1".to_vec()).await.unwrap();
        storage.store("key2", b"value2".to_vec()).await.unwrap();

        // Verify data exists
        assert_eq!(storage.stats().await.unwrap().key_count, 2);

        // Clear all
        storage.clear_all().await.unwrap();

        // Verify cleared
        assert_eq!(storage.stats().await.unwrap().key_count, 0);
        assert!(!storage.exists("key1").await.unwrap());
        assert!(!storage.exists("key2").await.unwrap());
    }
}

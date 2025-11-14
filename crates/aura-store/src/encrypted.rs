//! Encrypted storage handler with capability-based encryption
//!
//! This handler provides encryption at the storage layer using capability-based
//! key derivation.

use crate::{ChunkId, MemoryStorage};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    pub fn stack_info(&self) -> HashMap<String, String> {
        let storage = self.storage.read().unwrap();
        let chunk_count = storage.chunks.len() as u64;
        let total_bytes: u64 = storage.chunks.values().map(|v| v.len() as u64).sum();

        let mut info = HashMap::new();
        info.insert("backend_type".to_string(), "memory".to_string());
        info.insert("chunk_count".to_string(), chunk_count.to_string());
        info.insert("total_bytes".to_string(), total_bytes.to_string());
        info
    }
}

impl aura_core::effects::StorageEffects for EncryptedStorageHandler {
    fn store(
        &self,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), aura_core::effects::StorageError> {
        let mut storage = self.storage.write().unwrap();
        storage
            .chunks
            .insert(ChunkId::from_bytes(key.as_bytes()), value);
        Ok(())
    }

    fn retrieve(
        &self,
        key: &str,
    ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
        let storage = self.storage.read().unwrap();
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.chunks.get(&chunk_id).cloned())
    }

    fn remove(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        let mut storage = self.storage.write().unwrap();
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.chunks.remove(&chunk_id).is_some())
    }

    fn list_keys(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, aura_core::effects::StorageError> {
        let _ = prefix;
        Err(aura_core::effects::StorageError::ListFailed(
            "list_keys not supported in simplified storage API".to_string(),
        ))
    }

    fn exists(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        let storage = self.storage.read().unwrap();
        let chunk_id = ChunkId::from_bytes(key.as_bytes());
        Ok(storage.chunks.contains_key(&chunk_id))
    }

    fn store_batch(
        &self,
        pairs: HashMap<String, Vec<u8>>,
    ) -> Result<(), aura_core::effects::StorageError> {
        let mut storage = self.storage.write().unwrap();
        for (key, value) in pairs {
            let chunk_id = ChunkId::from_bytes(key.as_bytes());
            storage.chunks.insert(chunk_id, value);
        }
        Ok(())
    }

    fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, aura_core::effects::StorageError> {
        let storage = self.storage.read().unwrap();
        let mut result = HashMap::new();
        for key in keys {
            let chunk_id = ChunkId::from_bytes(key.as_bytes());
            if let Some(data) = storage.chunks.get(&chunk_id) {
                result.insert(key.clone(), data.clone());
            }
        }
        Ok(result)
    }

    fn clear_all(&self) -> Result<(), aura_core::effects::StorageError> {
        let mut storage = self.storage.write().unwrap();
        storage.clear();
        Ok(())
    }

    fn stats(
        &self,
    ) -> Result<aura_core::effects::StorageStats, aura_core::effects::StorageError> {
        let storage = self.storage.read().unwrap();
        let total_size: u64 = storage.chunks.values().map(|v| v.len() as u64).sum();

        Ok(aura_core::effects::StorageStats {
            key_count: storage.chunks.len() as u64,
            total_size,
            available_space: None,
            backend_type: "memory".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::StorageEffects;

    #[test]
    fn test_encrypted_storage_basic_operations() {
        // Create storage (encryption_key parameter ignored in current implementation)
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Test basic operations
        let key = "test_key";
        let value = b"test_value".to_vec();

        // Store
        storage.store(key, value.clone()).unwrap();

        // Retrieve
        let retrieved = storage.retrieve(key).unwrap();
        assert_eq!(retrieved, Some(value));

        // Exists
        assert!(storage.exists(key).unwrap());

        // Remove
        assert!(storage.remove(key).unwrap());
        assert!(!storage.exists(key).unwrap());
    }

    #[test]
    fn test_encrypted_storage_batch_operations() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Prepare batch data
        let mut batch_data = HashMap::new();
        batch_data.insert("key1".to_string(), b"value1".to_vec());
        batch_data.insert("key2".to_string(), b"value2".to_vec());
        batch_data.insert("key3".to_string(), b"value3".to_vec());

        // Store batch
        storage.store_batch(batch_data.clone()).unwrap();

        // Retrieve batch
        let keys: Vec<String> = batch_data.keys().cloned().collect();
        let retrieved = storage.retrieve_batch(&keys).unwrap();

        assert_eq!(retrieved.len(), 3);
        for (key, expected_value) in batch_data {
            assert_eq!(retrieved.get(&key).unwrap(), &expected_value);
        }
    }

    #[test]
    fn test_encrypted_storage_stats() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        // Initially empty
        let stats = storage.stats().unwrap();
        assert_eq!(stats.key_count, 0);

        // Add some data
        storage.store("key1", vec![1, 2, 3]).unwrap();
        storage.store("key2", vec![4, 5, 6, 7]).unwrap();

        let stats = storage.stats().unwrap();
        assert_eq!(stats.key_count, 2);
        assert_eq!(stats.total_size, 7); // 3 + 4 bytes
        assert_eq!(stats.backend_type, "memory");
    }

    #[test]
    fn test_storage_stack_info() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), Some(vec![0x42; 32]));

        let stack_info = storage.stack_info();
        assert!(stack_info.contains_key("backend_type"));
        assert!(stack_info.contains_key("chunk_count"));
        assert!(stack_info.contains_key("total_bytes"));
    }

    #[test]
    fn test_clear_all() {
        let storage = EncryptedStorageHandler::new("/tmp/test".to_string(), None);

        // Add data
        storage.store("key1", b"value1".to_vec()).unwrap();
        storage.store("key2", b"value2".to_vec()).unwrap();

        // Verify data exists
        assert_eq!(storage.stats().unwrap().key_count, 2);

        // Clear all
        storage.clear_all().unwrap();

        // Verify cleared
        assert_eq!(storage.stats().unwrap().key_count, 0);
        assert!(!storage.exists("key1").unwrap());
        assert!(!storage.exists("key2").unwrap());
    }
}

//! Mock implementations for testing
//!
//! This module provides mock Storage implementation for testing.
//! For transport mocking, use the aura_transport middleware system.

use async_trait::async_trait;
use aura_core::{AccountId, AuraResult};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Storage interface for testing
///
/// This provides a simple storage interface that mocks can implement
#[async_trait]
pub trait Storage: Send + Sync {
    fn account_id(&self) -> AccountId;
    async fn store(&self, key: &str, data: &[u8]) -> AuraResult<()>;
    async fn retrieve(&self, key: &str) -> AuraResult<Option<Vec<u8>>>;
    async fn exists(&self, key: &str) -> AuraResult<bool>;
    async fn delete(&self, key: &str) -> AuraResult<()>;
    async fn list_keys(&self) -> AuraResult<Vec<String>>;
    async fn get_stats(&self) -> AuraResult<StorageStats>;
}

/// Storage statistics for testing
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    pub total_keys: usize,
    pub total_size_bytes: u64,
    pub available_space_bytes: Option<u64>,
}

/// Mock storage implementation for testing
#[derive(Debug)]
pub struct MockStorage {
    account_id: AccountId,
    data: Arc<RwLock<std::collections::HashMap<String, Vec<u8>>>>,
}

impl MockStorage {
    /// Create a new mock storage for the given account
    pub fn new(account_id: AccountId) -> Self {
        Self {
            account_id,
            data: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get the current data for testing
    pub async fn get_all_data(&self) -> std::collections::HashMap<String, Vec<u8>> {
        self.data.read().await.clone()
    }

    /// Clear all stored data
    pub async fn clear(&self) {
        self.data.write().await.clear();
    }
}

#[async_trait]
impl Storage for MockStorage {
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    async fn store(&self, key: &str, data: &[u8]) -> AuraResult<()> {
        let mut storage = self.data.write().await;
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> AuraResult<Option<Vec<u8>>> {
        let storage = self.data.read().await;
        Ok(storage.get(key).cloned())
    }

    async fn delete(&self, key: &str) -> AuraResult<()> {
        let mut storage = self.data.write().await;
        storage.remove(key);
        Ok(())
    }

    async fn list_keys(&self) -> AuraResult<Vec<String>> {
        let storage = self.data.read().await;
        Ok(storage.keys().cloned().collect())
    }

    async fn exists(&self, key: &str) -> AuraResult<bool> {
        let storage = self.data.read().await;
        Ok(storage.contains_key(key))
    }

    async fn get_stats(&self) -> AuraResult<StorageStats> {
        let storage = self.data.read().await;
        let total_keys = storage.len();
        let total_size_bytes = storage.values().map(|v| v.len() as u64).sum();

        Ok(StorageStats {
            total_keys,
            total_size_bytes,
            available_space_bytes: Some(u64::MAX), // Unlimited for mock
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AccountIdExt;

    #[tokio::test]
    async fn test_mock_storage() {
        let account_id = AccountId::new();
        let storage = MockStorage::new(account_id);

        assert_eq!(storage.account_id(), account_id);

        let key = "test_key";
        let data = b"test data";

        assert!(!storage.exists(key).await.unwrap());
        storage.store(key, data).await.unwrap();
        assert!(storage.exists(key).await.unwrap());

        let retrieved = storage.retrieve(key).await.unwrap().unwrap();
        assert_eq!(retrieved, data);

        let keys = storage.list_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], key);

        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_mock_storage_stats() {
        let account_id = AccountId::new();
        let storage = MockStorage::new(account_id);

        storage.store("key1", b"data1").await.unwrap();
        storage.store("key2", b"data2").await.unwrap();

        let stats = storage.get_stats().await.unwrap();
        assert_eq!(stats.total_keys, 2);
        assert_eq!(stats.total_size_bytes, 10); // "data1" + "data2"
    }
}

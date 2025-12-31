//! Local store implementation
//!
//! Stores CLI/TUI preferences and cached data via `StorageEffects`.
//!
//! Note: Encryption is handled at a lower layer by the unified `EncryptedStorage`
//! handler; LocalStore intentionally does not do any encryption itself.

use std::path::Path;

use aura_core::effects::StorageEffects;

use super::errors::LocalStoreError;
use super::types::{LocalData, LocalStoreConfig};

/// Local store for CLI/TUI preferences.
pub struct LocalStore {
    /// Configuration including path
    config: LocalStoreConfig,

    /// Current data (loaded or default)
    data: LocalData,
}

impl LocalStore {
    /// Create a new local store with default data.
    #[must_use]
    pub fn new(config: LocalStoreConfig) -> Self {
        Self {
            config,
            data: LocalData::default(),
        }
    }

    /// Load existing data from storage.
    ///
    /// If the storage key doesn't exist, returns a store with default data.
    pub async fn load<S: StorageEffects>(
        config: LocalStoreConfig,
        storage: &S,
    ) -> Result<Self, LocalStoreError> {
        let storage_key = config.storage_key();
        let data = if storage
            .exists(&storage_key)
            .await
            .map_err(|e| LocalStoreError::StorageError(e.to_string()))?
        {
            let bytes = storage
                .retrieve(&storage_key)
                .await
                .map_err(|e| LocalStoreError::StorageError(e.to_string()))?
                .ok_or_else(|| LocalStoreError::StorageError("store not found".into()))?;
            serde_json::from_slice(&bytes)
                .map_err(|e| LocalStoreError::DeserializationError(e.to_string()))?
        } else {
            LocalData::default()
        };
        Ok(Self { config, data })
    }

    /// Save data to storage.
    pub async fn save<S: StorageEffects>(&self, storage: &S) -> Result<(), LocalStoreError> {
        let storage_key = self.config.storage_key();
        let bytes = serde_json::to_vec(&self.data)
            .map_err(|e| LocalStoreError::SerializationError(e.to_string()))?;
        storage
            .store(&storage_key, bytes)
            .await
            .map_err(|e| LocalStoreError::StorageError(e.to_string()))
    }

    /// Get a reference to the current data
    #[must_use]
    pub fn data(&self) -> &LocalData {
        &self.data
    }

    /// Get a mutable reference to the current data
    pub fn data_mut(&mut self) -> &mut LocalData {
        &mut self.data
    }

    /// Update data with a closure
    pub fn update<F>(&mut self, f: F)
    where
        F: FnOnce(&mut LocalData),
    {
        f(&mut self.data);
    }

    /// Get the store path
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.config.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::storage::{
        StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats,
    };
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Test-only in-memory storage for testing LocalStore
    struct TestStorage {
        data: RwLock<HashMap<String, Vec<u8>>>,
    }

    impl TestStorage {
        fn new() -> Self {
            Self {
                data: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl StorageCoreEffects for TestStorage {
        async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.insert(key.to_string(), value);
            Ok(())
        }

        async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            Ok(data.get(key).cloned())
        }

        async fn remove(&self, key: &str) -> Result<bool, StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::DeleteFailed(e.to_string()))?;
            Ok(data.remove(key).is_some())
        }

        async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ListFailed(e.to_string()))?;
            let keys: Vec<String> = match prefix {
                Some(p) => data.keys().filter(|k| k.starts_with(p)).cloned().collect(),
                None => data.keys().cloned().collect(),
            };
            Ok(keys)
        }
    }

    #[async_trait]
    impl StorageExtendedEffects for TestStorage {
        async fn exists(&self, key: &str) -> Result<bool, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            Ok(data.contains_key(key))
        }

        async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.extend(pairs);
            Ok(())
        }

        async fn retrieve_batch(
            &self,
            keys: &[String],
        ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            let result: HashMap<String, Vec<u8>> = keys
                .iter()
                .filter_map(|k| data.get(k).map(|v| (k.clone(), v.clone())))
                .collect();
            Ok(result)
        }

        async fn clear_all(&self) -> Result<(), StorageError> {
            let mut data = self
                .data
                .write()
                .map_err(|e| StorageError::WriteFailed(e.to_string()))?;
            data.clear();
            Ok(())
        }

        async fn stats(&self) -> Result<StorageStats, StorageError> {
            let data = self
                .data
                .read()
                .map_err(|e| StorageError::ReadFailed(e.to_string()))?;
            let key_count = data.len() as u64;
            let total_size = data.values().map(|v| v.len() as u64).sum();
            Ok(StorageStats {
                key_count,
                total_size,
                available_space: None,
                backend_type: "memory".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn test_save_and_load() {
        let config = LocalStoreConfig::new("/test/path.store");
        let mut store = LocalStore::new(config.clone());
        let storage = TestStorage::new();

        // Modify data
        store.data_mut().theme = super::super::types::ThemePreference::Light;
        store.data_mut().set_setting("test_key", "test_value");

        // Save
        store.save(&storage).await.unwrap();

        // Load and verify
        let loaded = LocalStore::load(config, &storage).await.unwrap();

        assert_eq!(
            loaded.data().theme,
            super::super::types::ThemePreference::Light
        );
        assert_eq!(
            loaded.data().get_setting("test_key"),
            Some(&"test_value".to_string())
        );
    }

    #[tokio::test]
    async fn test_new_store_has_defaults() {
        let config = LocalStoreConfig::new("/tmp/nonexistent.store");
        let store = LocalStore::new(config);

        assert_eq!(
            store.data().theme,
            super::super::types::ThemePreference::Dark
        );
        assert!(store.data().contacts.is_empty());
    }
}

//! Storage effect handlers
//!
//! This module provides standard implementations of the `StorageEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_core::effects::{StorageEffects, StorageError, StorageStats};
use aura_core::ChunkId;
use aura_macros::aura_effect_handlers;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;

// Generate both memory and filesystem storage handlers using the macro
aura_effect_handlers! {
    trait_name: StorageEffects,
    mock: {
        struct_name: MemoryStorageHandler,
        state: {
            data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            store(key: &str, value: Vec<u8>) -> Result<(), StorageError> => {
                let mut data = self.data.write().await;
                data.insert(key.to_string(), value);
                Ok(())
            },
            retrieve(key: &str) -> Result<Option<Vec<u8>>, StorageError> => {
                let data = self.data.read().await;
                Ok(data.get(key).cloned())
            },
            remove(key: &str) -> Result<bool, StorageError> => {
                let mut data = self.data.write().await;
                Ok(data.remove(key).is_some())
            },
            list_keys(prefix: Option<&str>) -> Result<Vec<String>, StorageError> => {
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
            },
            exists(key: &str) -> Result<bool, StorageError> => {
                let data = self.data.read().await;
                Ok(data.contains_key(key))
            },
            store_batch(pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> => {
                let mut data = self.data.write().await;
                for (key, value) in pairs {
                    data.insert(key, value);
                }
                Ok(())
            },
            retrieve_batch(keys: &[String]) -> Result<HashMap<String, Vec<u8>>, StorageError> => {
                let data = self.data.read().await;
                let mut result = HashMap::new();
                for key in keys {
                    if let Some(value) = data.get(key) {
                        result.insert(key.clone(), value.clone());
                    }
                }
                Ok(result)
            },
            clear_all() -> Result<(), StorageError> => {
                let mut data = self.data.write().await;
                data.clear();
                Ok(())
            },
            stats() -> Result<StorageStats, StorageError> => {
                let data = self.data.read().await;
                let total_size = data.values().map(|v| v.len() as u64).sum();
                Ok(StorageStats {
                    key_count: data.len() as u64,
                    total_size,
                    available_space: None,
                    backend_type: "memory".to_string(),
                })
            },
        },
    },
    real: {
        struct_name: FilesystemStorageHandler,
        state: {
            base_path: PathBuf,
        },
        features: {
            async_trait: true,
            disallowed_methods: true,
        },
        methods: {
            store(key: &str, value: Vec<u8>) -> Result<(), StorageError> => {
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

                fs::write(&file_path, value).await.map_err(|e| {
                    StorageError::WriteFailed(format!("Failed to write file: {}", e))
                })?;

                Ok(())
            },
            retrieve(key: &str) -> Result<Option<Vec<u8>>, StorageError> => {
                let file_path = self.base_path.join(format!("{}.dat", key));

                if !file_path.exists() {
                    return Ok(None);
                }

                let data = fs::read(&file_path).await.map_err(|e| {
                    StorageError::ReadFailed(format!("Failed to read file: {}", e))
                })?;

                Ok(Some(data))
            },
            remove(key: &str) -> Result<bool, StorageError> => {
                let file_path = self.base_path.join(format!("{}.dat", key));

                if !file_path.exists() {
                    return Ok(false);
                }

                fs::remove_file(&file_path).await.map_err(|e| {
                    StorageError::DeleteFailed(format!("Failed to remove file: {}", e))
                })?;

                Ok(true)
            },
            list_keys(prefix: Option<&str>) -> Result<Vec<String>, StorageError> => {
                let mut keys = Vec::new();
                let mut read_dir = fs::read_dir(&self.base_path).await.map_err(|e| {
                    StorageError::ListFailed(format!("Failed to read directory: {}", e))
                })?;

                while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
                    StorageError::ListFailed(format!("Failed to read entry: {}", e))
                })? {
                    if let Ok(file_type) = entry.file_type().await {
                        if file_type.is_file() {
                            let filename = entry.file_name();
                            if let Some(name_str) = filename.to_str() {
                                if name_str.ends_with(".dat") {
                                    let key = name_str.trim_end_matches(".dat");
                                    if let Some(prefix_str) = prefix {
                                        if key.starts_with(prefix_str) {
                                            keys.push(key.to_string());
                                        }
                                    } else {
                                        keys.push(key.to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(keys)
            },
            exists(key: &str) -> Result<bool, StorageError> => {
                let file_path = self.base_path.join(format!("{}.dat", key));
                Ok(file_path.exists())
            },
            store_batch(pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> => {
                for (key, value) in pairs {
                    self.store(&key, value).await?;
                }
                Ok(())
            },
            retrieve_batch(keys: &[String]) -> Result<HashMap<String, Vec<u8>>, StorageError> => {
                let mut result = HashMap::new();
                for key in keys {
                    if let Some(value) = self.retrieve(key).await? {
                        result.insert(key.clone(), value);
                    }
                }
                Ok(result)
            },
            clear_all() -> Result<(), StorageError> => {
                if !self.base_path.exists() {
                    return Ok(());
                }

                let mut read_dir = fs::read_dir(&self.base_path).await.map_err(|e| {
                    StorageError::DeleteFailed(format!("Failed to read directory: {}", e))
                })?;

                while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
                    StorageError::DeleteFailed(format!("Failed to read entry: {}", e))
                })? {
                    if let Ok(file_type) = entry.file_type().await {
                        if file_type.is_file() {
                            fs::remove_file(entry.path()).await.map_err(|e| {
                                StorageError::DeleteFailed(format!("Failed to remove file: {}", e))
                            })?;
                        }
                    }
                }

                Ok(())
            },
            stats() -> Result<StorageStats, StorageError> => {
                let mut key_count = 0u64;
                let mut total_size = 0u64;

                if self.base_path.exists() {
                    let mut read_dir = fs::read_dir(&self.base_path).await.map_err(|e| {
                        StorageError::ListFailed(format!("Failed to read directory: {}", e))
                    })?;

                    while let Some(entry) = read_dir.next_entry().await.map_err(|e| {
                        StorageError::ListFailed(format!("Failed to read entry: {}", e))
                    })? {
                        if let Ok(metadata) = entry.metadata().await {
                            if metadata.is_file() {
                                let filename = entry.file_name();
                                if let Some(name_str) = filename.to_str() {
                                    if name_str.ends_with(".dat") {
                                        key_count += 1;
                                        total_size += metadata.len();
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(StorageStats {
                    key_count,
                    total_size,
                    available_space: Some(1024 * 1024 * 1024), // 1GB placeholder
                    backend_type: "filesystem".to_string(),
                })
            },
        },
    },
}

impl MemoryStorageHandler {
    /// Get the number of stored keys (for testing)
    pub fn len(&self) -> usize {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async { self.data.read().await.len() })
        })
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

impl FilesystemStorageHandler {
    /// Create a new filesystem storage handler with base path
    pub fn with_path(_base_path: PathBuf) -> Self {
        Self::new()
    }

    /// Get the base path
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }
}

/// Enhanced memory storage for encrypted storage use cases
///
/// This handler wraps memory storage with additional features like
/// chunk-based addressing and encryption support.
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
        let runtime = tokio::runtime::Handle::current();
        let chunk_count = runtime.block_on(async { self.storage.read().await.len() as u64 });
        let total_bytes = runtime.block_on(async {
            self.storage
                .read()
                .await
                .values()
                .map(|v| v.len() as u64)
                .sum::<u64>()
        });

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

    /// Clear all chunks
    pub async fn clear_chunks(&self) {
        self.storage.write().await.clear();
    }
}

#[async_trait::async_trait]
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

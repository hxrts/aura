//! Storage effect handlers
//!
//! This module provides standard implementations of the `StorageEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_macros::aura_effect_handlers;
use aura_core::effects::{StorageEffects, StorageError, StorageStats};
use async_trait::async_trait;
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
            tokio::runtime::Handle::current().block_on(async {
                self.data.read().await.len()
            })
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
    pub fn with_path(base_path: PathBuf) -> Self {
        Self::new_deterministic(base_path)
    }

    /// Get the base path
    pub fn base_path(&self) -> &PathBuf {
        &self.base_path
    }
}

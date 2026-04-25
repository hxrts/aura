//! WASM storage handler backed by browser localStorage.
//!
//! On wasm32 targets we keep the existing `FilesystemStorageHandler` name for API
//! compatibility, but route persistence to browser localStorage.

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use std::collections::HashMap;
use std::path::PathBuf;
use web_sys::{window, Storage};

/// WASM storage handler using browser localStorage persistence.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    namespace: String,
}

impl FilesystemStorageHandler {
    /// Create a new handler for the given logical storage path.
    pub fn new(base_path: PathBuf) -> Self {
        let path_str = base_path.to_string_lossy();
        let digest = aura_core::hash::hash(path_str.as_bytes());
        let namespace = format!("aura_storage_{}", hex::encode(&digest[..8]));
        Self { namespace }
    }

    /// Alias for clarity; avoids relying on `new` naming in higher layers.
    pub fn from_path(base_path: PathBuf) -> Self {
        Self::new(base_path)
    }

    /// Create a new handler with a stable default storage namespace.
    pub fn with_default_path() -> Self {
        Self::new(PathBuf::from("./storage"))
    }

    fn invalid_key(key: &str) -> Result<(), StorageError> {
        if key.is_empty() {
            Err(StorageError::InvalidKey {
                reason: "Key cannot be empty".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn storage(&self) -> Result<Storage, StorageError> {
        let win = window().ok_or_else(|| StorageError::ConfigurationError {
            reason: "window is unavailable".to_string(),
        })?;
        let storage = win
            .local_storage()
            .map_err(|err| StorageError::ConfigurationError {
                reason: format!("localStorage lookup failed: {err:?}"),
            })?
            .ok_or_else(|| StorageError::ConfigurationError {
                reason: "localStorage is unavailable".to_string(),
            })?;
        Ok(storage)
    }

    fn storage_key(&self, key: &str) -> String {
        format!("{}::{}", self.namespace, key)
    }

    fn decode_storage_key<'a>(&self, key: &'a str) -> Option<&'a str> {
        let prefix = format!("{}::", self.namespace);
        key.strip_prefix(&prefix)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        Self::invalid_key(key)?;
        let encoded = hex::encode(value);
        self.storage()?
            .set_item(&self.storage_key(key), &encoded)
            .map_err(|err| {
                StorageError::WriteFailed(format!("localStorage set_item failed: {err:?}"))
            })
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Self::invalid_key(key)?;
        match self
            .storage()?
            .get_item(&self.storage_key(key))
            .map_err(|err| {
                StorageError::ReadFailed(format!("localStorage get_item failed: {err:?}"))
            })? {
            Some(value) => hex::decode(&value).map(Some).map_err(|err| {
                StorageError::ReadFailed(format!("localStorage decode failed: {err}"))
            }),
            None => Ok(None),
        }
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        Self::invalid_key(key)?;
        let existed = self.exists(key).await?;
        if existed {
            self.storage()?
                .remove_item(&self.storage_key(key))
                .map_err(|err| {
                    StorageError::DeleteFailed(format!("localStorage remove_item failed: {err:?}"))
                })?;
        }
        Ok(existed)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let storage = self.storage()?;
        let mut keys = Vec::new();
        for index in 0..storage.length().map_err(|err| {
            StorageError::ReadFailed(format!("localStorage length failed: {err:?}"))
        })? {
            let full_key = match storage.key(index).map_err(|err| {
                StorageError::ReadFailed(format!("localStorage key lookup failed: {err:?}"))
            })? {
                Some(value) => value,
                None => continue,
            };
            let Some(decoded) = self.decode_storage_key(&full_key) else {
                continue;
            };
            if let Some(prefix) = prefix {
                if !decoded.starts_with(prefix) {
                    continue;
                }
            }
            keys.push(decoded.to_string());
        }
        keys.sort();
        Ok(keys)
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.retrieve(key).await?.is_some())
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        for (key, value) in pairs {
            self.store(&key, value).await?;
        }
        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError> {
        let mut out = HashMap::new();
        for key in keys {
            if let Some(value) = self.retrieve(key).await? {
                out.insert(key.clone(), value);
            }
        }
        Ok(out)
    }

    async fn clear_all(&self) -> Result<(), StorageError> {
        let keys = self.list_keys(None).await?;
        for key in keys {
            let _ = self.remove(&key).await?;
        }
        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, StorageError> {
        let keys = self.list_keys(None).await?;
        let mut total_size: u64 = 0;
        for key in &keys {
            if let Some(value) = self.retrieve(key).await? {
                total_size = total_size.saturating_add(value.len() as u64);
            }
        }

        Ok(StorageStats {
            key_count: keys.len() as u64,
            total_size,
            available_space: None,
            backend_type: "localstorage".to_string(),
        })
    }
}

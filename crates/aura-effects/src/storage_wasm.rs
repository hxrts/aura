//! WASM storage handler backed by IndexedDB.
//!
//! On wasm32 targets we keep the existing `FilesystemStorageHandler` name for API
//! compatibility, but route persistence to browser IndexedDB.

use async_trait::async_trait;
use aura_core::effects::{StorageCoreEffects, StorageError, StorageExtendedEffects, StorageStats};
use futures::channel::oneshot;
use indexed_db_futures::{
    database::Database,
    prelude::*,
    transaction::{Transaction, TransactionMode},
};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use wasm_bindgen_futures::spawn_local;

const STORAGE_VERSION: u8 = 1;
const OBJECT_STORE_NAME: &str = "kv";

/// WASM storage handler using IndexedDB persistence.
#[derive(Debug, Clone)]
pub struct FilesystemStorageHandler {
    db_name: String,
}

impl FilesystemStorageHandler {
    /// Create a new handler for the given logical storage path.
    pub fn new(base_path: PathBuf) -> Self {
        let path_str = base_path.to_string_lossy();
        let digest = aura_core::hash::hash(path_str.as_bytes());
        let db_name = format!("aura_storage_{}", hex::encode(&digest[..8]));
        Self { db_name }
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

    fn map_read<E: std::fmt::Display>(op: &str, err: E) -> StorageError {
        StorageError::ReadFailed(format!("IndexedDB {op} failed: {err}"))
    }

    fn map_write<E: std::fmt::Display>(op: &str, err: E) -> StorageError {
        StorageError::WriteFailed(format!("IndexedDB {op} failed: {err}"))
    }

    fn map_delete<E: std::fmt::Display>(op: &str, err: E) -> StorageError {
        StorageError::DeleteFailed(format!("IndexedDB {op} failed: {err}"))
    }

    async fn run_local<T, Mk, Fut>(op: &'static str, make_fut: Mk) -> Result<T, StorageError>
    where
        T: 'static,
        Mk: FnOnce() -> Fut + 'static,
        Fut: Future<Output = Result<T, StorageError>> + 'static,
    {
        let (tx, rx) = oneshot::channel();
        spawn_local(async move {
            let _ = tx.send(make_fut().await);
        });

        rx.await.map_err(|_| StorageError::ConfigurationError {
            reason: format!("IndexedDB operation '{op}' task dropped"),
        })?
    }

    async fn open_db(db_name: &str) -> Result<Database, StorageError> {
        Database::open(db_name)
            .with_version(STORAGE_VERSION)
            .with_on_upgrade_needed(|_event, db| {
                let has_store = db
                    .object_store_names()
                    .any(|name| name == OBJECT_STORE_NAME);
                if !has_store {
                    let _ = db.create_object_store(OBJECT_STORE_NAME).build()?;
                }
                Ok(())
            })
            .await
            .map_err(|e| StorageError::ConfigurationError {
                reason: format!("IndexedDB open failed for '{db_name}': {e}"),
            })
    }

    fn open_tx<'a>(
        db: &'a Database,
        mode: TransactionMode,
    ) -> Result<Transaction<'a>, StorageError> {
        db.transaction(OBJECT_STORE_NAME)
            .with_mode(mode)
            .build()
            .map_err(|e| StorageError::ConfigurationError {
                reason: format!("IndexedDB transaction open failed: {e}"),
            })
    }

    async fn store_inner(db_name: String, key: String, value: Vec<u8>) -> Result<(), StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readwrite)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_write("open object_store", e))?;

        store
            .put(value)
            .with_key(key)
            .without_key_type()
            .serde()
            .map_err(|e| Self::map_write("put request", e))?
            .await
            .map_err(|e| Self::map_write("put await", e))?;

        tx.commit()
            .await
            .map_err(|e| Self::map_write("commit", e))?;
        Ok(())
    }

    async fn retrieve_inner(db_name: String, key: String) -> Result<Option<Vec<u8>>, StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readonly)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_read("open object_store", e))?;

        store
            .get(key)
            .serde()
            .map_err(|e| Self::map_read("get request", e))?
            .await
            .map_err(|e| Self::map_read("get await", e))
    }

    async fn delete_inner(db_name: String, key: String) -> Result<(), StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readwrite)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_delete("open object_store", e))?;

        store
            .delete(key)
            .primitive()
            .map_err(|e| Self::map_delete("delete request", e))?
            .await
            .map_err(|e| Self::map_delete("delete await", e))?;

        tx.commit()
            .await
            .map_err(|e| Self::map_delete("commit", e))?;
        Ok(())
    }

    async fn list_keys_inner(db_name: String) -> Result<Vec<String>, StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readonly)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_read("open object_store", e))?;

        let keys_iter = store
            .get_all_keys()
            .primitive()
            .map_err(|e| Self::map_read("get_all_keys request", e))?
            .await
            .map_err(|e| Self::map_read("get_all_keys await", e))?;

        let mut keys: Vec<String> = Vec::new();
        for key_res in keys_iter {
            keys.push(key_res.map_err(|e| Self::map_read("key decode", e))?);
        }
        keys.sort();
        Ok(keys)
    }

    async fn store_batch_inner(
        db_name: String,
        pairs: HashMap<String, Vec<u8>>,
    ) -> Result<(), StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readwrite)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_write("open object_store", e))?;

        for (key, value) in pairs {
            Self::invalid_key(&key)?;
            store
                .put(value)
                .with_key(key)
                .without_key_type()
                .serde()
                .map_err(|e| Self::map_write("batch put request", e))?
                .await
                .map_err(|e| Self::map_write("batch put await", e))?;
        }

        tx.commit()
            .await
            .map_err(|e| Self::map_write("commit", e))?;
        Ok(())
    }

    async fn clear_all_inner(db_name: String) -> Result<(), StorageError> {
        let db = Self::open_db(&db_name).await?;
        let tx = Self::open_tx(&db, TransactionMode::Readwrite)?;
        let store = tx
            .object_store(OBJECT_STORE_NAME)
            .map_err(|e| Self::map_delete("open object_store", e))?;

        store
            .clear()
            .map_err(|e| Self::map_delete("clear request", e))?
            .await
            .map_err(|e| Self::map_delete("clear await", e))?;

        tx.commit()
            .await
            .map_err(|e| Self::map_delete("commit", e))?;
        Ok(())
    }
}

#[async_trait]
impl StorageCoreEffects for FilesystemStorageHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        Self::invalid_key(key)?;
        let db_name = self.db_name.clone();
        let key = key.to_string();
        Self::run_local("store", move || Self::store_inner(db_name, key, value)).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        Self::invalid_key(key)?;
        let db_name = self.db_name.clone();
        let key = key.to_string();
        Self::run_local("retrieve", move || Self::retrieve_inner(db_name, key)).await
    }

    async fn remove(&self, key: &str) -> Result<bool, StorageError> {
        Self::invalid_key(key)?;
        if !self.exists(key).await? {
            return Ok(false);
        }

        let db_name = self.db_name.clone();
        let key = key.to_string();
        Self::run_local("remove", move || Self::delete_inner(db_name, key)).await?;
        Ok(true)
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError> {
        let db_name = self.db_name.clone();
        let mut keys = Self::run_local("list_keys", move || Self::list_keys_inner(db_name)).await?;
        if let Some(prefix) = prefix {
            keys.retain(|k| k.starts_with(prefix));
        }
        Ok(keys)
    }
}

#[async_trait]
impl StorageExtendedEffects for FilesystemStorageHandler {
    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        Ok(self.retrieve(key).await?.is_some())
    }

    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError> {
        if pairs.is_empty() {
            return Ok(());
        }

        let db_name = self.db_name.clone();
        Self::run_local("store_batch", move || {
            Self::store_batch_inner(db_name, pairs)
        })
        .await
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
        let db_name = self.db_name.clone();
        Self::run_local("clear_all", move || Self::clear_all_inner(db_name)).await
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
            backend_type: "indexeddb".to_string(),
        })
    }
}

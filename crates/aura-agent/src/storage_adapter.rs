//! Production implementations of Storage traits

use crate::utils::{time, ResultExt};
use crate::{AgentError, Result, Storage, StorageStats};
use async_trait::async_trait;
use aura_types::AccountId;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

// Define table for key-value storage
const DATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("data");
const METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

#[derive(Debug, Serialize, Deserialize)]
struct StorageMetadata {
    created_at: u64,
    updated_at: u64,
    size_bytes: u64,
    checksum: String,
}

/// Production storage implementation using persistent storage (redb)
#[derive(Debug)]
pub struct ProductionStorage {
    account_id: AccountId,
    storage_path: std::path::PathBuf,
    database: Arc<Mutex<Database>>,
}

impl ProductionStorage {
    /// Create a new production storage
    pub fn new(account_id: AccountId, storage_path: impl Into<std::path::PathBuf>) -> Result<Self> {
        let storage_path = storage_path.into();

        // Create parent directories if they don't exist
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent).storage_context("Create storage directory")?;
        }

        // Open or create redb database
        let database = Database::create(&storage_path)
            .storage_context(&format!("Create database at {:?}", storage_path))?;

        // Initialize tables
        {
            let write_txn = database
                .begin_write()
                .storage_context("Begin write transaction")?;

            write_txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;

            write_txn
                .open_table(METADATA_TABLE)
                .storage_context("Open metadata table")?;

            write_txn
                .commit()
                .storage_context("Commit table creation")?;
        }

        Ok(Self {
            account_id,
            storage_path,
            database: Arc::new(Mutex::new(database)),
        })
    }

    /// Compute blake3 checksum of data
    fn compute_checksum(data: &[u8]) -> String {
        hex::encode(blake3::hash(data).as_bytes())
    }

    /// Execute a function within a write transaction
    async fn with_write_txn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&redb::WriteTransaction) -> Result<R>,
    {
        let database = self.database.lock().await;
        let txn = database
            .begin_write()
            .storage_context("Begin write transaction")?;
        let result = f(&txn)?;
        txn.commit().storage_context("Commit transaction")?;
        Ok(result)
    }

    /// Execute a function within a read transaction
    async fn with_read_txn<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&redb::ReadTransaction) -> Result<R>,
    {
        let database = self.database.lock().await;
        let txn = database
            .begin_read()
            .storage_context("Begin read transaction")?;
        f(&txn)
    }

    /// Initialize the storage (already done in constructor)
    pub async fn initialize(&self) -> Result<()> {
        info!(
            "Production storage already initialized at {:?}",
            self.storage_path
        );
        Ok(())
    }

    /// Cleanup and close storage
    pub async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up production storage");
        // redb automatically handles cleanup when dropped
        Ok(())
    }

    /// Backup storage to a specified location
    pub async fn backup(&self, backup_path: impl Into<std::path::PathBuf>) -> Result<()> {
        let backup_path = backup_path.into();
        info!("Creating storage backup to {:?}", backup_path);

        // Create backup directory
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent).storage_context("Create backup directory")?;
        }

        // Copy database file
        std::fs::copy(&self.storage_path, &backup_path).storage_context("Copy database")?;

        info!("Storage backup completed");
        Ok(())
    }
}

#[async_trait]
impl Storage for ProductionStorage {
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    async fn store(&self, key: &str, data: &[u8]) -> Result<()> {
        let timestamp = time::timestamp_secs();

        let metadata = StorageMetadata {
            created_at: timestamp,
            updated_at: timestamp,
            size_bytes: data.len() as u64,
            checksum: Self::compute_checksum(data),
        };

        let metadata_bytes = bincode::serialize(&metadata).serialize_context("Metadata")?;

        self.with_write_txn(|txn| {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;
            data_table
                .insert(key, data)
                .storage_context("Insert data")?;

            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .storage_context("Open metadata table")?;
            metadata_table
                .insert(key, metadata_bytes.as_slice())
                .storage_context("Insert metadata")?;

            Ok(())
        })
        .await?;

        debug!(
            "Stored {} bytes at key '{}' for account {} (checksum: {})",
            data.len(),
            key,
            self.account_id,
            metadata.checksum
        );
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.with_read_txn(|txn| {
            let data_table = txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;

            match data_table.get(key) {
                Ok(Some(data)) => {
                    let bytes = data.value().to_vec();
                    debug!(
                        "Retrieved {} bytes from key '{}' for account {}",
                        bytes.len(),
                        key,
                        self.account_id
                    );
                    Ok(Some(bytes))
                }
                Ok(None) => {
                    debug!("Key '{}' not found for account {}", key, self.account_id);
                    Ok(None)
                }
                Err(e) => Err(AgentError::storage_failed(format!("Retrieve data: {}", e))),
            }
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.with_write_txn(|txn| {
            let mut data_table = txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;
            data_table.remove(key).storage_context("Delete data")?;

            let mut metadata_table = txn
                .open_table(METADATA_TABLE)
                .storage_context("Open metadata table")?;
            metadata_table
                .remove(key)
                .storage_context("Delete metadata")?;

            Ok(())
        })
        .await?;

        debug!("Deleted key '{}' for account {}", key, self.account_id);
        Ok(())
    }

    async fn list_keys(&self) -> Result<Vec<String>> {
        self.with_read_txn(|txn| {
            let data_table = txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;

            let mut keys = Vec::new();
            let iter = data_table.iter().storage_context("Create iterator")?;

            for result in iter {
                let (key, _) = result.storage_context("Read key")?;
                keys.push(key.value().to_string());
            }

            debug!("Listed {} keys for account {}", keys.len(), self.account_id);
            Ok(keys)
        })
        .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        self.with_read_txn(|txn| {
            let data_table = txn
                .open_table(DATA_TABLE)
                .storage_context("Open data table")?;

            let exists = data_table
                .get(key)
                .storage_context("Check key existence")?
                .is_some();

            Ok(exists)
        })
        .await
    }

    async fn stats(&self) -> Result<StorageStats> {
        self.with_read_txn(|txn| {
            let metadata_table = txn
                .open_table(METADATA_TABLE)
                .storage_context("Open metadata table")?;

            let mut total_keys = 0u64;
            let mut total_size_bytes = 0u64;

            let iter = metadata_table.iter().storage_context("Create iterator")?;

            for result in iter {
                let (_, metadata_bytes) = result.storage_context("Read metadata")?;

                if let Ok(metadata) =
                    bincode::deserialize::<StorageMetadata>(metadata_bytes.value())
                {
                    total_keys += 1;
                    total_size_bytes += metadata.size_bytes;
                }
            }

            // Get filesystem stats for available space
            let available_space_bytes = if self.storage_path.exists() {
                // For simplicity, use a placeholder value
                // In production, you'd use platform-specific APIs
                Some(1_000_000_000) // 1GB placeholder
            } else {
                None
            };

            debug!(
                "Storage stats for account {}: {} keys, {} bytes",
                self.account_id, total_keys, total_size_bytes
            );

            Ok(StorageStats {
                total_keys,
                total_size_bytes,
                available_space_bytes,
            })
        })
        .await
    }
}

/// Factory for creating production storage
pub struct ProductionFactory;

impl ProductionFactory {
    /// Create a production storage instance
    pub async fn create_storage(
        account_id: AccountId,
        storage_path: impl Into<std::path::PathBuf>,
    ) -> Result<ProductionStorage> {
        let storage = ProductionStorage::new(account_id, storage_path)?;
        storage.initialize().await?;
        Ok(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_production_storage() {
        let account_id = AccountId::new();
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("storage.db");

        let storage = ProductionFactory::create_storage(account_id, storage_path)
            .await
            .unwrap();

        // Test basic functionality
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

        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.total_keys, 1);
        assert_eq!(stats.total_size_bytes, data.len() as u64);

        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());

        storage.cleanup().await.unwrap();
    }
}

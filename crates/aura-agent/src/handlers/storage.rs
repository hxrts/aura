//! Storage operations using device storage effects
//!
//! This module provides high-level storage operations that consume device storage
//! effects. All operations are effect-based for testability and simulation support.
//!
//! **Phase 5 Update**: Now integrated with authorization operations system.

use crate::{errors::Result, operations::*};
use aura_core::AuraError;
use aura_core::DeviceId;
use aura_protocol::{
    orchestration::AuraEffectSystem,
    effect_traits::{ConsoleEffects, StorageEffects, TimeEffects},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Storage key with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageKey {
    /// Key name
    pub name: String,
    /// Key namespace
    pub namespace: String,
    /// Creation timestamp
    pub created_at: u64,
    /// Last accessed timestamp
    pub last_accessed: u64,
    /// Key size in bytes
    pub size: usize,
    /// Whether key is encrypted
    pub encrypted: bool,
}

/// Storage operations handler
pub struct StorageOperations {
    /// Effect system for storage operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this instance
    device_id: DeviceId,
    /// Namespace for keys
    namespace: String,
    /// Authorized operations handler
    auth_operations: Option<Arc<AuthorizedAgentOperations>>,
}

impl StorageOperations {
    /// Create new storage operations handler
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        device_id: DeviceId,
        namespace: String,
    ) -> Self {
        Self {
            effects,
            device_id,
            namespace,
            auth_operations: None,
        }
    }

    /// Create new storage operations handler with authorization
    pub fn with_authorization(
        effects: Arc<RwLock<AuraEffectSystem>>,
        device_id: DeviceId,
        namespace: String,
        auth_operations: Arc<AuthorizedAgentOperations>,
    ) -> Self {
        Self {
            effects,
            device_id,
            namespace,
            auth_operations: Some(auth_operations),
        }
    }

    /// Store data with authorization check
    pub async fn store_data_authorized(
        &self,
        request: AgentOperationRequest,
        data: &[u8],
    ) -> Result<String> {
        if let Some(auth_ops) = &self.auth_operations {
            let effects = self.effects.read().await;
            let timestamp = effects.current_timestamp().await;

            let storage_op = StorageOperation::Store {
                key: format!("auto_{}", timestamp),
                data: data.to_vec(),
            };

            let agent_op = AgentOperation::Storage {
                operation: storage_op,
                namespace: self.namespace.clone(),
            };

            let auth_request = AgentOperationRequest {
                identity_proof: request.identity_proof,
                operation: agent_op,
                signed_message: request.signed_message,
                context: request.context,
            };

            let result = auth_ops.execute_operation(auth_request).await?;

            match result {
                AgentOperationResult::Storage {
                    result: StorageResult::Stored { key },
                } => Ok(key),
                _ => Err(AuraError::internal("Unexpected result type")),
            }
        } else {
            // Fallback to direct storage without authorization
            self.store_data_direct(data).await
        }
    }

    /// Store data with automatic key generation (legacy method, kept for compatibility)
    pub async fn store_data(&self, data: &[u8]) -> Result<String> {
        self.store_data_direct(data).await
    }

    /// Store data with automatic key generation (direct, no authorization)
    pub async fn store_data_direct(&self, data: &[u8]) -> Result<String> {
        let effects = self.effects.read().await;

        // Generate unique key
        let timestamp = effects.current_timestamp().await;

        let key = format!(
            "{}:data:{}:{}",
            self.namespace,
            self.device_id.0.simple(),
            timestamp
        );

        // Store data through effects
        effects
            .store(&key, data.to_vec())
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        // Store metadata
        let metadata = StorageKey {
            name: key.clone(),
            namespace: self.namespace.clone(),
            created_at: timestamp,
            last_accessed: timestamp,
            size: data.len(),
            encrypted: true, // Device storage is always encrypted
        };

        let metadata_key = format!("{}:meta", key);
        let metadata_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| AuraError::internal(format!("Metadata serialization failed: {}", e)))?;

        effects
            .store(&metadata_key, metadata_bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        let _ = effects
            .log_debug(&format!("Stored {} bytes with key: {}", data.len(), key))
            .await;
        Ok(key)
    }

    /// Store data with specific key
    pub async fn store_data_with_key(&self, key: &str, data: &[u8]) -> Result<()> {
        let effects = self.effects.read().await;

        let full_key = format!("{}:{}", self.namespace, key);

        // Store data through effects
        effects
            .store(&full_key, data.to_vec())
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        // Store metadata
        let timestamp = effects.current_timestamp().await;

        let metadata = StorageKey {
            name: full_key.clone(),
            namespace: self.namespace.clone(),
            created_at: timestamp,
            last_accessed: timestamp,
            size: data.len(),
            encrypted: true,
        };

        let metadata_key = format!("{}:meta", full_key);
        let metadata_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| AuraError::internal(format!("Metadata serialization failed: {}", e)))?;

        effects
            .store(&metadata_key, metadata_bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        let _ = effects
            .log_debug(&format!(
                "Stored {} bytes with key: {}",
                data.len(),
                full_key
            ))
            .await;
        Ok(())
    }

    /// Retrieve data by key
    pub async fn retrieve_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let effects = self.effects.read().await;

        let full_key = format!("{}:{}", self.namespace, key);

        // Retrieve data through effects
        let data = effects
            .retrieve(&full_key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        if data.is_some() {
            // Update last accessed timestamp in metadata
            self.update_last_accessed(&full_key).await?;
            let _ = effects
                .log_debug(&format!("Retrieved data for key: {}", full_key))
                .await;
        } else {
            let _ = effects
                .log_debug(&format!("No data found for key: {}", full_key))
                .await;
        }

        Ok(data)
    }

    /// Delete data by key
    pub async fn delete_data(&self, key: &str) -> Result<()> {
        let effects = self.effects.read().await;

        let full_key = format!("{}:{}", self.namespace, key);

        // Delete data through effects
        effects
            .remove(&full_key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        // Delete metadata
        let metadata_key = format!("{}:meta", full_key);
        let _ = effects.remove(&metadata_key).await; // Ignore errors for metadata

        let _ = effects
            .log_debug(&format!("Deleted data for key: {}", full_key))
            .await;
        Ok(())
    }

    /// List all stored keys with metadata
    pub async fn list_keys(&self) -> Result<Vec<StorageKey>> {
        let effects = self.effects.read().await;

        // Get all keys through effects
        let all_keys = effects
            .list_keys(None)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        let mut storage_keys = Vec::new();
        let namespace_prefix = format!("{}:", self.namespace);

        for key in all_keys {
            if key.starts_with(&namespace_prefix) && !key.ends_with(":meta") {
                // Try to load metadata
                let metadata_key = format!("{}:meta", key);
                if let Ok(Some(metadata_bytes)) = effects.retrieve(&metadata_key).await {
                    if let Ok(metadata) = serde_json::from_slice::<StorageKey>(&metadata_bytes) {
                        storage_keys.push(metadata);
                    }
                }
            }
        }

        let _ = effects
            .log_debug(&format!(
                "Listed {} keys in namespace {}",
                storage_keys.len(),
                self.namespace
            ))
            .await;
        Ok(storage_keys)
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> Result<StorageStats> {
        let keys = self.list_keys().await?;

        let total_keys = keys.len();
        let total_size = keys.iter().map(|k| k.size).sum();
        let encrypted_keys = keys.iter().filter(|k| k.encrypted).count();

        let mut namespaces = HashMap::new();
        for key in &keys {
            *namespaces.entry(key.namespace.clone()).or_insert(0) += 1;
        }

        let oldest_key = keys.iter().map(|k| k.created_at).min();
        let newest_key = keys.iter().map(|k| k.created_at).max();

        Ok(StorageStats {
            total_keys,
            total_size,
            encrypted_keys,
            namespaces,
            oldest_key_timestamp: oldest_key,
            newest_key_timestamp: newest_key,
        })
    }

    /// Clear all data in namespace
    pub async fn clear_namespace(&self) -> Result<usize> {
        let keys = self.list_keys().await?;
        let count = keys.len();

        for key in keys {
            // Extract the key suffix (without namespace prefix)
            if let Some(suffix) = key.name.strip_prefix(&format!("{}:", self.namespace)) {
                self.delete_data(suffix).await?;
            }
        }

        let effects = self.effects.read().await;
        let _ = effects
            .log_info(&format!(
                "Cleared {} keys from namespace {}",
                count, self.namespace
            ))
            .await;
        Ok(count)
    }

    /// Check if key exists
    pub async fn key_exists(&self, key: &str) -> Result<bool> {
        // If the key already starts with the namespace prefix, use it directly
        // Otherwise, add the namespace prefix
        let full_key = if key.starts_with(&format!("{}:", self.namespace)) {
            key.to_string()
        } else {
            format!("{}:{}", self.namespace, key)
        };

        let effects = self.effects.read().await;
        let data = effects
            .retrieve(&full_key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        Ok(data.is_some())
    }

    /// Get key metadata
    pub async fn get_key_metadata(&self, key: &str) -> Result<Option<StorageKey>> {
        let full_key = format!("{}:{}", self.namespace, key);
        let metadata_key = format!("{}:meta", full_key);

        let effects = self.effects.read().await;
        let metadata_bytes = effects
            .retrieve(&metadata_key)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))?;

        if let Some(bytes) = metadata_bytes {
            let metadata = serde_json::from_slice::<StorageKey>(&bytes).map_err(|e| {
                AuraError::internal(format!("Metadata deserialization failed: {}", e))
            })?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    /// Backup all data in namespace
    pub async fn backup_namespace(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let keys = self.list_keys().await?;
        let mut backup = Vec::new();

        for key in keys {
            if let Some(suffix) = key.name.strip_prefix(&format!("{}:", self.namespace)) {
                if let Ok(Some(data)) = self.retrieve_data(suffix).await {
                    backup.push((suffix.to_string(), data));
                }
            }
        }

        let effects = self.effects.read().await;
        let _ = effects
            .log_info(&format!(
                "Backed up {} keys from namespace {}",
                backup.len(),
                self.namespace
            ))
            .await;
        Ok(backup)
    }

    /// Restore from backup
    pub async fn restore_from_backup(&self, backup: Vec<(String, Vec<u8>)>) -> Result<usize> {
        let mut restored = 0;

        for (key, data) in backup {
            if let Ok(()) = self.store_data_with_key(&key, &data).await {
                restored += 1;
            }
        }

        let effects = self.effects.read().await;
        let _ = effects
            .log_info(&format!(
                "Restored {} keys to namespace {}",
                restored, self.namespace
            ))
            .await;
        Ok(restored)
    }

    // Private helper methods

    async fn update_last_accessed(&self, full_key: &str) -> Result<()> {
        let metadata_key = format!("{}:meta", full_key);

        let effects = self.effects.read().await;
        if let Ok(Some(metadata_bytes)) = effects.retrieve(&metadata_key).await {
            if let Ok(mut metadata) = serde_json::from_slice::<StorageKey>(&metadata_bytes) {
                let timestamp = effects.current_timestamp().await;

                metadata.last_accessed = timestamp;

                if let Ok(updated_bytes) = serde_json::to_vec(&metadata) {
                    let _ = effects.store(&metadata_key, updated_bytes).await;
                }
            }
        }

        Ok(())
    }
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of keys
    pub total_keys: usize,
    /// Total size in bytes
    pub total_size: usize,
    /// Number of encrypted keys
    pub encrypted_keys: usize,
    /// Keys per namespace
    pub namespaces: HashMap<String, usize>,
    /// Oldest key timestamp
    pub oldest_key_timestamp: Option<u64>,
    /// Newest key timestamp
    pub newest_key_timestamp: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;
    use aura_macros::aura_test;
    use aura_protocol::orchestration::AuraEffectSystem;

    #[aura_test]
    async fn test_storage_operations() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let effects = Arc::new(RwLock::new((*fixture.effects()).clone()));
        let storage = StorageOperations::new(effects, device_id, "test".to_string());

        // Store data
        let data = b"test data";
        let key = storage.store_data(data).await.unwrap();
        assert!(!key.is_empty());

        // Check if key exists
        assert!(storage.key_exists(&key).await.unwrap());

        // Store with specific key
        storage.store_data_with_key("specific", data).await.unwrap();

        // Retrieve data
        let retrieved = storage.retrieve_data("specific").await.unwrap();
        assert_eq!(retrieved, Some(data.to_vec()));

        // List keys
        let keys = storage.list_keys().await.unwrap();
        assert!(!keys.is_empty());

        // Get stats
        let stats = storage.get_storage_stats().await.unwrap();
        assert!(stats.total_keys > 0);
        assert!(stats.total_size > 0);

        // Delete data
        storage.delete_data("specific").await?;
        let after_delete = storage.retrieve_data("specific").await?;
        assert_eq!(after_delete, None);

        Ok(())
    }

    #[aura_test]
    async fn test_backup_restore() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
        let effects = Arc::new(RwLock::new((*fixture.effects()).clone()));
        let storage = StorageOperations::new(effects, device_id, "backup_test".to_string());

        // Store some test data
        storage.store_data_with_key("key1", b"data1").await.unwrap();
        storage.store_data_with_key("key2", b"data2").await.unwrap();

        // Backup
        let backup = storage.backup_namespace().await.unwrap();
        assert_eq!(backup.len(), 2);

        // Clear namespace
        let cleared = storage.clear_namespace().await.unwrap();
        assert_eq!(cleared, 2);

        // Verify cleared
        let keys_after_clear = storage.list_keys().await.unwrap();
        assert_eq!(keys_after_clear.len(), 0);

        // Restore
        let restored = storage.restore_from_backup(backup).await.unwrap();
        assert_eq!(restored, 2);

        // Verify restored
        let keys_after_restore = storage.list_keys().await?;
        assert_eq!(keys_after_restore.len(), 2);

        Ok(())
    }
}

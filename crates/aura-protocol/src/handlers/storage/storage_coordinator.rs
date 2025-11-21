//! Multi-handler storage coordination
//!
//! This module provides coordination across multiple storage handlers,
//! enabling composition of different storage backends and capabilities.

use aura_core::effects::{StorageEffects, StorageError, StorageStats};
use aura_core::{AuraResult, identifiers::DeviceId};
use aura_effects::{EncryptedStorageHandler, FilesystemStorageHandler, MemoryStorageHandler};
use std::collections::HashMap;
use std::sync::Arc;

/// Multi-handler storage coordinator that composes different storage backends
pub struct StorageCoordinator {
    /// Primary storage handler
    primary: StorageBackend,
    /// Secondary/backup storage handlers
    replicas: Vec<StorageBackend>,
    /// Device identifier for this coordinator
    device_id: DeviceId,
    /// Handler routing rules
    routing_rules: HashMap<String, String>,
}

/// Storage backend enum wrapping different handler types
#[derive(Clone)]
pub enum StorageBackend {
    /// In-memory storage
    Memory(Arc<MemoryStorageHandler>),
    /// Filesystem storage
    Filesystem(Arc<FilesystemStorageHandler>),
    /// Encrypted memory storage
    Encrypted(Arc<EncryptedStorageHandler>),
}

impl StorageBackend {
    /// Get backend type identifier
    pub fn backend_type(&self) -> &'static str {
        match self {
            StorageBackend::Memory(_) => "memory",
            StorageBackend::Filesystem(_) => "filesystem",
            StorageBackend::Encrypted(_) => "encrypted",
        }
    }

    /// Execute storage operation on this backend
    async fn execute<F, R>(&self, operation: F) -> R
    where
        F: for<'a> FnOnce(
            &'a dyn StorageEffects,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = R> + Send + 'a>>,
    {
        match self {
            StorageBackend::Memory(handler) => operation(handler.as_ref()).await,
            StorageBackend::Filesystem(handler) => operation(handler.as_ref()).await,
            StorageBackend::Encrypted(handler) => operation(handler.as_ref()).await,
        }
    }
}

/// Builder for storage coordinator
pub struct StorageCoordinatorBuilder {
    device_id: DeviceId,
    primary: Option<StorageBackend>,
    replicas: Vec<StorageBackend>,
    routing_rules: HashMap<String, String>,
}

impl StorageCoordinatorBuilder {
    /// Create new builder
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            primary: None,
            replicas: Vec::new(),
            routing_rules: HashMap::new(),
        }
    }

    /// Set primary storage backend
    pub fn with_primary(mut self, backend: StorageBackend) -> Self {
        self.primary = Some(backend);
        self
    }

    /// Add replica storage backend
    pub fn add_replica(mut self, backend: StorageBackend) -> Self {
        self.replicas.push(backend);
        self
    }

    /// Add routing rule (key pattern -> backend type)
    pub fn with_routing_rule(mut self, pattern: String, backend_type: String) -> Self {
        self.routing_rules.insert(pattern, backend_type);
        self
    }

    /// Build the coordinator
    pub fn build(self) -> AuraResult<StorageCoordinator> {
        let primary = self
            .primary
            .ok_or_else(|| aura_core::AuraError::invalid("Primary storage backend is required"))?;

        Ok(StorageCoordinator {
            primary,
            replicas: self.replicas,
            device_id: self.device_id,
            routing_rules: self.routing_rules,
        })
    }
}

impl StorageCoordinator {
    /// Create a simple coordinator with memory storage
    pub fn with_memory(device_id: DeviceId) -> Self {
        let primary = StorageBackend::Memory(Arc::new(MemoryStorageHandler::new()));
        Self {
            primary,
            replicas: Vec::new(),
            device_id,
            routing_rules: HashMap::new(),
        }
    }

    /// Create coordinator with encrypted storage
    pub fn with_encrypted(device_id: DeviceId, encryption_key: Option<Vec<u8>>) -> Self {
        let primary = StorageBackend::Encrypted(Arc::new(EncryptedStorageHandler::new(
            "/tmp/storage".to_string(),
            encryption_key,
        )));
        Self {
            primary,
            replicas: Vec::new(),
            device_id,
            routing_rules: HashMap::new(),
        }
    }

    /// Select appropriate backend for a key
    fn select_backend(&self, key: &str) -> &StorageBackend {
        // Check routing rules
        for (pattern, backend_type) in &self.routing_rules {
            if key.contains(pattern) {
                // Find matching replica
                for replica in &self.replicas {
                    if replica.backend_type() == backend_type {
                        return replica;
                    }
                }
                // Fall back to primary if no matching replica
                if self.primary.backend_type() == backend_type {
                    return &self.primary;
                }
            }
        }

        // Default to primary
        &self.primary
    }

    /// Store data with coordination across backends
    pub async fn coordinated_store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError> {
        let backend = self.select_backend(key);
        let key_owned = key.to_string();

        // Store to selected backend
        let value_for_primary = value.clone();
        backend
            .execute(|storage| {
                let key_ref = key_owned.clone();
                Box::pin(async move { storage.store(&key_ref, value_for_primary).await })
            })
            .await?;

        // Optionally replicate to other backends
        if !self.replicas.is_empty() {
            for replica in &self.replicas {
                if replica.backend_type() != backend.backend_type() {
                    // Async replication (fire and forget for now)
                    let key_ref = key_owned.clone();
                    let value_for_replica = value.clone();
                    let _ = replica
                        .execute(|storage| {
                            Box::pin(
                                async move { storage.store(&key_ref, value_for_replica).await },
                            )
                        })
                        .await;
                }
            }
        }

        Ok(())
    }

    /// Retrieve data with fallback across backends
    pub async fn coordinated_retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let primary_backend = self.select_backend(key);
        let key_owned = key.to_string();

        // Try primary backend first
        if let Some(data) = primary_backend
            .execute(|storage| {
                let key_ref = key_owned.clone();
                Box::pin(async move { storage.retrieve(&key_ref).await })
            })
            .await?
        {
            return Ok(Some(data));
        }

        // Try replicas if primary doesn't have the data
        for replica in &self.replicas {
            if replica.backend_type() != primary_backend.backend_type() {
                let key_ref = key_owned.clone();
                if let Some(data) = replica
                    .execute(|storage| Box::pin(async move { storage.retrieve(&key_ref).await }))
                    .await?
                {
                    // Found in replica, update primary for future reads
                    let key_ref_for_store = key_owned.clone();
                    let data_for_store = data.clone();
                    let _ = primary_backend
                        .execute(|storage| {
                            Box::pin(async move {
                                storage.store(&key_ref_for_store, data_for_store).await
                            })
                        })
                        .await;
                    return Ok(Some(data));
                }
            }
        }

        Ok(None)
    }

    /// Remove from all backends
    pub async fn coordinated_remove(&self, key: &str) -> Result<bool, StorageError> {
        let key_owned = key.to_string();

        // Remove from primary
        let primary_backend = self.select_backend(key);
        let removed = match primary_backend
            .execute(|storage| {
                let key_ref = key_owned.clone();
                Box::pin(async move { storage.remove(&key_ref).await })
            })
            .await
        {
            Ok(was_removed) => was_removed,
            Err(e) => return Err(e),
        };

        // Remove from all replicas
        for replica in &self.replicas {
            let key_ref = key_owned.clone();
            let _ = replica
                .execute(|storage| Box::pin(async move { storage.remove(&key_ref).await }))
                .await;
        }

        Ok(removed)
    }

    /// Get combined statistics across all backends
    pub async fn combined_stats(&self) -> Result<StorageStats, StorageError> {
        let primary_stats = self
            .primary
            .execute(|storage| Box::pin(async move { storage.stats().await }))
            .await?;

        let mut total_key_count = primary_stats.key_count;
        let mut total_size = primary_stats.total_size;

        // Add replica stats (with deduplication consideration)
        for replica in &self.replicas {
            let replica_stats = replica
                .execute(|storage| Box::pin(async move { storage.stats().await }))
                .await?;

            // Note: This is simplified - in practice we'd need to account for duplication
            total_key_count += replica_stats.key_count;
            total_size += replica_stats.total_size;
        }

        Ok(StorageStats {
            key_count: total_key_count,
            total_size,
            available_space: primary_stats.available_space,
            backend_type: format!("coordinated_{}", primary_stats.backend_type),
        })
    }

    /// Get coordinator information
    pub fn info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("device_id".to_string(), self.device_id.to_string());
        info.insert(
            "primary_backend".to_string(),
            self.primary.backend_type().to_string(),
        );
        info.insert("replica_count".to_string(), self.replicas.len().to_string());

        let replica_types: Vec<String> = self
            .replicas
            .iter()
            .map(|r| r.backend_type().to_string())
            .collect();
        info.insert("replica_types".to_string(), replica_types.join(","));

        info
    }

    /// Check consistency across backends
    pub async fn check_consistency(&self, key: &str) -> Result<bool, StorageError> {
        let key_owned = key.to_string();

        let primary_data = self
            .primary
            .execute(|storage| {
                let key_ref = key_owned.clone();
                Box::pin(async move { storage.retrieve(&key_ref).await })
            })
            .await?;

        for replica in &self.replicas {
            let key_ref = key_owned.clone();
            let replica_data = replica
                .execute(|storage| Box::pin(async move { storage.retrieve(&key_ref).await }))
                .await?;

            if primary_data != replica_data {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;

    #[tokio::test]
    async fn test_coordinator_basic_operations() {
        let device_id = DeviceId::new();
        let coordinator = StorageCoordinator::with_memory(device_id);

        // Test store and retrieve
        let key = "test_key";
        let value = b"test_value".to_vec();

        coordinator
            .coordinated_store(key, value.clone())
            .await
            .unwrap();
        let retrieved = coordinator.coordinated_retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Test remove
        assert!(coordinator.coordinated_remove(key).await.unwrap());
        let retrieved_after_remove = coordinator.coordinated_retrieve(key).await.unwrap();
        assert_eq!(retrieved_after_remove, None);
    }

    #[tokio::test]
    async fn test_coordinator_with_replicas() {
        let device_id = DeviceId::new();
        let coordinator = StorageCoordinatorBuilder::new(device_id)
            .with_primary(StorageBackend::Memory(
                Arc::new(MemoryStorageHandler::new()),
            ))
            .add_replica(StorageBackend::Encrypted(Arc::new(
                EncryptedStorageHandler::new("/tmp/test".to_string(), None),
            )))
            .build()
            .unwrap();

        let key = "replicated_key";
        let value = b"replicated_value".to_vec();

        coordinator
            .coordinated_store(key, value.clone())
            .await
            .unwrap();
        let retrieved = coordinator.coordinated_retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(value));

        // Check consistency
        assert!(coordinator.check_consistency(key).await.unwrap());
    }

    #[tokio::test]
    async fn test_routing_rules() {
        let device_id = DeviceId::new();
        let coordinator = StorageCoordinatorBuilder::new(device_id)
            .with_primary(StorageBackend::Memory(
                Arc::new(MemoryStorageHandler::new()),
            ))
            .add_replica(StorageBackend::Encrypted(Arc::new(
                EncryptedStorageHandler::new("/tmp/test".to_string(), None),
            )))
            .with_routing_rule("secret_".to_string(), "encrypted".to_string())
            .build()
            .unwrap();

        let normal_key = "normal_data";
        let secret_key = "secret_data";

        // Normal data should go to primary (memory)
        assert_eq!(
            coordinator.select_backend(normal_key).backend_type(),
            "memory"
        );

        // Secret data should go to encrypted backend
        assert_eq!(
            coordinator.select_backend(secret_key).backend_type(),
            "encrypted"
        );
    }
}

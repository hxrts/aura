//! Storage Backend Abstraction
//!
//! Provides abstract interfaces for storage backend and access control,
//! enabling clean separation of concerns and testing.

use async_trait::async_trait;
use aura_errors::Result;
use aura_types::{AccountId, DeviceId};

/// Content identifier (CID)
pub type ContentId = String;

/// Storage chunk metadata
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
    /// Content identifier
    pub cid: ContentId,
    /// Chunk size in bytes
    pub size: usize,
    /// Creation timestamp
    pub created_at: u64,
    /// Owner account ID
    pub owner: AccountId,
}

/// Storage backend abstraction
///
/// This trait provides an abstract interface for storing and retrieving
/// content-addressed chunks, independent of access control logic.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Store a chunk and return its content ID
    async fn put(&self, data: Vec<u8>, owner: AccountId) -> Result<ContentId>;

    /// Retrieve a chunk by content ID
    async fn get(&self, cid: &ContentId) -> Result<Vec<u8>>;

    /// Check if a chunk exists
    async fn has(&self, cid: &ContentId) -> Result<bool>;

    /// Delete a chunk by content ID
    async fn delete(&self, cid: &ContentId) -> Result<()>;

    /// Get chunk metadata
    async fn metadata(&self, cid: &ContentId) -> Result<ChunkMetadata>;

    /// List chunks owned by an account
    async fn list(&self, owner: AccountId) -> Result<Vec<ChunkMetadata>>;

    /// Get total storage used by an account (in bytes)
    async fn storage_used(&self, owner: AccountId) -> Result<u64>;
}

/// Access control decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access granted
    Allow,
    /// Access denied
    Deny,
}

/// Access control abstraction
///
/// This trait provides an abstract interface for capability-based access control,
/// separated from storage backend concerns.
#[async_trait]
pub trait AccessController: Send + Sync {
    /// Check if a device can read a specific chunk
    async fn can_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<AccessDecision>;

    /// Check if a device can write chunks for an account
    async fn can_write(&self, device_id: DeviceId, account_id: AccountId)
        -> Result<AccessDecision>;

    /// Check if a device can delete a specific chunk
    async fn can_delete(&self, device_id: DeviceId, cid: &ContentId) -> Result<AccessDecision>;

    /// Grant read capability for a chunk to a device
    async fn grant_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<()>;

    /// Grant write capability for an account to a device
    async fn grant_write(&self, device_id: DeviceId, account_id: AccountId) -> Result<()>;

    /// Revoke read capability
    async fn revoke_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<()>;

    /// Revoke write capability
    async fn revoke_write(&self, device_id: DeviceId, account_id: AccountId) -> Result<()>;
}

/// Extension trait for access control operations
#[async_trait]
pub trait AccessControllerExt: AccessController {
    /// Check if access is allowed (convenience method)
    async fn is_allowed(&self, decision: AccessDecision) -> bool {
        decision == AccessDecision::Allow
    }

    /// Require access or return error
    async fn require_access(&self, decision: AccessDecision) -> Result<()> {
        match decision {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny => Err(aura_errors::AuraError::permission_denied("Access denied")),
        }
    }
}

/// Automatically implement AccessControllerExt for all AccessController implementations
impl<T: AccessController + ?Sized> AccessControllerExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    // Mock storage backend for testing
    struct MockStorage {
        chunks: Arc<RwLock<HashMap<ContentId, (Vec<u8>, ChunkMetadata)>>>,
    }

    impl MockStorage {
        fn new() -> Self {
            Self {
                chunks: Arc::new(RwLock::new(HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl StorageBackend for MockStorage {
        async fn put(&self, data: Vec<u8>, owner: AccountId) -> Result<ContentId> {
            let cid = format!("cid_{}", uuid::Uuid::new_v4());
            let metadata = ChunkMetadata {
                cid: cid.clone(),
                size: data.len(),
                created_at: 1000,
                owner,
            };
            self.chunks
                .write()
                .await
                .insert(cid.clone(), (data, metadata));
            Ok(cid)
        }

        async fn get(&self, cid: &ContentId) -> Result<Vec<u8>> {
            self.chunks
                .read()
                .await
                .get(cid)
                .map(|(data, _)| data.clone())
                .ok_or_else(|| aura_errors::AuraError::storage_read_failed("Chunk not found"))
        }

        async fn has(&self, cid: &ContentId) -> Result<bool> {
            Ok(self.chunks.read().await.contains_key(cid))
        }

        async fn delete(&self, cid: &ContentId) -> Result<()> {
            self.chunks.write().await.remove(cid);
            Ok(())
        }

        async fn metadata(&self, cid: &ContentId) -> Result<ChunkMetadata> {
            self.chunks
                .read()
                .await
                .get(cid)
                .map(|(_, metadata)| metadata.clone())
                .ok_or_else(|| aura_errors::AuraError::storage_read_failed("Chunk not found"))
        }

        async fn list(&self, owner: AccountId) -> Result<Vec<ChunkMetadata>> {
            let chunks = self.chunks.read().await;
            let metadata: Vec<ChunkMetadata> = chunks
                .values()
                .filter(|(_, meta)| meta.owner == owner)
                .map(|(_, meta)| meta.clone())
                .collect();
            Ok(metadata)
        }

        async fn storage_used(&self, owner: AccountId) -> Result<u64> {
            let chunks = self.chunks.read().await;
            let total: usize = chunks
                .values()
                .filter(|(_, meta)| meta.owner == owner)
                .map(|(_, meta)| meta.size)
                .sum();
            Ok(total as u64)
        }
    }

    // Mock access controller for testing
    struct MockAccessController;

    #[async_trait]
    impl AccessController for MockAccessController {
        async fn can_read(&self, _device_id: DeviceId, _cid: &ContentId) -> Result<AccessDecision> {
            Ok(AccessDecision::Allow)
        }

        async fn can_write(
            &self,
            _device_id: DeviceId,
            _account_id: AccountId,
        ) -> Result<AccessDecision> {
            Ok(AccessDecision::Allow)
        }

        async fn can_delete(
            &self,
            _device_id: DeviceId,
            _cid: &ContentId,
        ) -> Result<AccessDecision> {
            Ok(AccessDecision::Allow)
        }

        async fn grant_read(&self, _device_id: DeviceId, _cid: &ContentId) -> Result<()> {
            Ok(())
        }

        async fn grant_write(&self, _device_id: DeviceId, _account_id: AccountId) -> Result<()> {
            Ok(())
        }

        async fn revoke_read(&self, _device_id: DeviceId, _cid: &ContentId) -> Result<()> {
            Ok(())
        }

        async fn revoke_write(&self, _device_id: DeviceId, _account_id: AccountId) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_storage_backend() {
        let storage = MockStorage::new();
        let owner = AccountId(uuid::Uuid::new_v4());

        // Put a chunk
        let cid = storage.put(vec![1, 2, 3], owner).await.unwrap();

        // Verify it exists
        assert!(storage.has(&cid).await.unwrap());

        // Get it back
        let data = storage.get(&cid).await.unwrap();
        assert_eq!(data, vec![1, 2, 3]);

        // Check metadata
        let metadata = storage.metadata(&cid).await.unwrap();
        assert_eq!(metadata.size, 3);
        assert_eq!(metadata.owner, owner);

        // Delete it
        storage.delete(&cid).await.unwrap();
        assert!(!storage.has(&cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_access_controller() {
        let controller = MockAccessController;
        let device_id = DeviceId(uuid::Uuid::new_v4());
        let cid = "test_cid".to_string();

        let decision = controller.can_read(device_id, &cid).await.unwrap();
        assert_eq!(decision, AccessDecision::Allow);

        controller.require_access(decision).await.unwrap();
    }
}

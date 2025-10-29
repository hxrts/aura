//! Storage backend abstractions for protocol execution.

use async_trait::async_trait;
use aura_types::AuraError;
use aura_types::{AccountId, AuraResult as Result, DeviceId};
use serde::{Deserialize, Serialize};

/// Content identifier used by storage backends.
pub type ContentId = String;

/// Metadata describing a content chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    /// Content identifier.
    pub cid: ContentId,
    /// Size in bytes.
    pub size: usize,
    /// Creation timestamp.
    pub created_at: u64,
    /// Owner account identifier.
    pub owner: AccountId,
}

/// Storage backend abstraction.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Store data and return its CID.
    async fn put(&self, data: Vec<u8>, owner: AccountId) -> Result<ContentId>;
    /// Retrieve data by CID.
    async fn get(&self, cid: &ContentId) -> Result<Vec<u8>>;
    /// Check for existence.
    async fn has(&self, cid: &ContentId) -> Result<bool>;
    /// Delete content.
    async fn delete(&self, cid: &ContentId) -> Result<()>;
    /// Retrieve metadata.
    async fn metadata(&self, cid: &ContentId) -> Result<ChunkMetadata>;
    /// List content owned by account.
    async fn list(&self, owner: AccountId) -> Result<Vec<ChunkMetadata>>;
    /// Compute total usage for account.
    async fn storage_used(&self, owner: AccountId) -> Result<u64>;
}

/// Access control decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access permitted.
    Allow,
    /// Access denied.
    Deny,
}

/// Capability-based access controller abstraction.
#[async_trait]
pub trait AccessController: Send + Sync {
    /// Check read permission.
    async fn can_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<AccessDecision>;
    /// Check write permission.
    async fn can_write(&self, device_id: DeviceId, account_id: AccountId)
        -> Result<AccessDecision>;
    /// Check delete permission.
    async fn can_delete(&self, device_id: DeviceId, cid: &ContentId) -> Result<AccessDecision>;
    /// Grant read capability.
    async fn grant_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<()>;
    /// Grant write capability.
    async fn grant_write(&self, device_id: DeviceId, account_id: AccountId) -> Result<()>;
    /// Revoke read capability.
    async fn revoke_read(&self, device_id: DeviceId, cid: &ContentId) -> Result<()>;
    /// Revoke write capability.
    async fn revoke_write(&self, device_id: DeviceId, account_id: AccountId) -> Result<()>;
}

/// Convenience helpers for access controllers.
#[async_trait]
#[allow(dead_code)]
pub trait AccessControllerExt: AccessController {
    /// Ensure decision is `Allow`.
    async fn require(&self, decision: AccessDecision) -> Result<()> {
        match decision {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny => Err(AuraError::permission_denied("capability check denied")),
        }
    }
}

#[allow(dead_code)]
impl<T: AccessController + ?Sized> AccessControllerExt for T {}

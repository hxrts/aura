//! Agent trait definitions for the unified agent implementation

use crate::{DerivedIdentity, Result};
use async_trait::async_trait;
use aura_types::{AccountId, DeviceId};

/// Core Agent trait that defines the public-facing API
///
/// This trait abstracts over different agent implementations and states,
/// providing a unified interface for agent operations.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Derive a new identity for a specific application and context
    async fn derive_identity(&self, app_id: &str, context: &str) -> Result<DerivedIdentity>;

    /// Store data with capability-based access control
    async fn store_data(&self, data: &[u8], capabilities: Vec<String>) -> Result<String>;

    /// Retrieve data with capability verification
    async fn retrieve_data(&self, data_id: &str) -> Result<Vec<u8>>;

    /// Get the device identifier
    fn device_id(&self) -> DeviceId;

    /// Get the account identifier
    fn account_id(&self) -> AccountId;
}

/// Extended agent trait for protocol coordination
#[async_trait]
pub trait CoordinatingAgent: Agent {
    /// Initiate a recovery protocol
    async fn initiate_recovery(&mut self, recovery_params: serde_json::Value) -> Result<()>;

    /// Initiate a resharing protocol
    async fn initiate_resharing(
        &mut self,
        new_threshold: u16,
        new_participants: Vec<DeviceId>,
    ) -> Result<()>;

    /// Check the status of any running protocol
    async fn check_protocol_status(&self) -> Result<crate::ProtocolStatus>;
}

/// Agent capability for identity management
#[async_trait]
pub trait IdentityAgent: Send + Sync {
    /// Issue an authentication credential
    async fn issue_authentication_credential(
        &self,
        app_id: &str,
        user_context: &str,
    ) -> Result<Vec<u8>>;

    /// Verify an authentication credential
    async fn verify_authentication(&self, credential: &[u8], app_id: &str) -> Result<bool>;

    /// Issue an authorization token
    async fn issue_authorization_token(&self, capabilities: Vec<String>) -> Result<Vec<u8>>;

    /// Check authorization for a specific capability
    async fn check_authorization(&self, token: &[u8], capability: &str) -> Result<bool>;
}

/// Agent capability for group management
#[async_trait]
pub trait GroupAgent: Send + Sync {
    /// Create a new group
    async fn create_group(&self, group_config: serde_json::Value) -> Result<String>;

    /// Join an existing group
    async fn join_group(&self, group_id: &str, invitation: &[u8]) -> Result<()>;

    /// Leave a group
    async fn leave_group(&self, group_id: &str) -> Result<()>;

    /// List groups this agent is a member of
    async fn list_groups(&self) -> Result<Vec<String>>;
}

/// Agent capability for network operations
#[async_trait]
pub trait NetworkAgent: Send + Sync {
    /// Connect to a peer over the network
    async fn network_connect(&self, peer_id: DeviceId) -> Result<()>;

    /// Disconnect from a peer
    async fn network_disconnect(&self, peer_id: DeviceId) -> Result<()>;

    /// Get list of connected peers
    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>>;

    /// Get network statistics
    async fn get_network_stats(&self) -> Result<serde_json::Value>;
}

/// Agent capability for storage operations
#[async_trait]
pub trait StorageAgent: Send + Sync {
    /// Store encrypted data with metadata
    async fn store_encrypted(&self, data: &[u8], metadata: serde_json::Value) -> Result<String>;

    /// Retrieve encrypted data
    async fn retrieve_encrypted(&self, data_id: &str) -> Result<(Vec<u8>, serde_json::Value)>;

    /// Delete stored data
    async fn delete_data(&self, data_id: &str) -> Result<()>;

    /// Get storage statistics
    async fn get_storage_stats(&self) -> Result<serde_json::Value>;

    /// Replicate data to peer devices
    async fn replicate_data(
        &self,
        data_id: &str,
        peer_device_ids: Vec<String>,
    ) -> Result<Vec<String>>;

    /// Retrieve replicated data from peer devices
    async fn retrieve_replica(&self, data_id: &str, peer_device_id: &str) -> Result<Vec<u8>>;

    /// List all available replicas for a data ID
    async fn list_replicas(&self, data_id: &str) -> Result<Vec<String>>;

    /// Simulate data tampering for testing purposes
    async fn simulate_data_tamper(&self, data_id: &str) -> Result<()>;

    /// Verify data integrity using cryptographic checks
    async fn verify_data_integrity(&self, data_id: &str) -> Result<bool>;

    /// Set storage quota limit for a device or capability scope
    async fn set_storage_quota(&self, scope: &str, limit_bytes: u64) -> Result<()>;

    /// Get current storage usage and quota information
    async fn get_storage_quota_info(&self, scope: &str) -> Result<serde_json::Value>;

    /// Enforce storage quota and trigger eviction if needed
    async fn enforce_storage_quota(&self, scope: &str) -> Result<bool>;

    /// Get list of eviction candidates based on LRU policy
    async fn get_eviction_candidates(&self, scope: &str, bytes_needed: u64) -> Result<Vec<String>>;

    /// Grant storage capability to a device for specific data
    async fn grant_storage_capability(
        &self,
        data_id: &str,
        grantee_device: DeviceId,
        permissions: Vec<String>,
    ) -> Result<String>;

    /// Revoke storage capability from a device
    async fn revoke_storage_capability(&self, capability_id: &str, reason: &str) -> Result<()>;

    /// Verify if a device has capability to access specific data
    async fn verify_storage_capability(
        &self,
        data_id: &str,
        requesting_device: DeviceId,
        required_permission: &str,
    ) -> Result<bool>;

    /// List active capabilities for a data item
    async fn list_storage_capabilities(&self, data_id: &str) -> Result<serde_json::Value>;

    /// Test access to data using device credentials (simulates cross-device access)
    async fn test_access_with_device(&self, data_id: &str, device_id: DeviceId) -> Result<bool>;
}

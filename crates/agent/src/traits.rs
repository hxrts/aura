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
}

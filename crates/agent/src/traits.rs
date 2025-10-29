//! Agent trait definitions for the unified agent implementation

use crate::{AgentError, DerivedIdentity, Result, StorageStats};
use async_trait::async_trait;
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};

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

    /// Get detailed status of all active sessions
    async fn get_detailed_session_status(&self) -> Result<Vec<aura_protocol::SessionStatusInfo>>;

    /// Check if any sessions are in a failed state that requires intervention
    async fn has_failed_sessions(&self) -> Result<bool>;

    /// Get the time remaining before any active sessions timeout
    async fn get_session_timeout_info(&self) -> Result<Option<std::time::Duration>>;
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

/// Transport layer trait for network communication
///
/// This is a simplified adapter over the `aura-transport` crate's Transport trait,
/// providing a cleaner API for the agent layer.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Get the device ID for this transport
    fn device_id(&self) -> DeviceId;

    /// Send a message to a peer
    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()>;

    /// Receive messages (non-blocking, with default timeout)
    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>>;

    /// Connect to a peer
    async fn connect(&self, peer_id: DeviceId, endpoint: &str) -> Result<()>;

    /// Disconnect from a peer
    async fn disconnect(&self, peer_id: DeviceId) -> Result<()>;

    /// Get list of connected peers
    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>>;

    /// Check if connected to a peer
    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool>;
}

/// Adapter to use transport crate implementations with agent Transport trait
///
/// This allows us to use `aura-transport` implementations (like NoiseTcpTransport)
/// with the agent's Transport trait API.
pub struct TransportAdapter<T: aura_transport::Transport + 'static> {
    inner: std::sync::Arc<T>,
    device_id: DeviceId,
    receive_timeout: std::time::Duration,
}

impl<T: aura_transport::Transport + 'static> TransportAdapter<T> {
    /// Create a new transport adapter
    pub fn new(transport: std::sync::Arc<T>, device_id: DeviceId) -> Self {
        Self {
            inner: transport,
            device_id,
            receive_timeout: std::time::Duration::from_millis(100),
        }
    }

    /// Set the receive timeout
    pub fn with_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.receive_timeout = timeout;
        self
    }
}

#[async_trait]
impl<T: aura_transport::Transport + 'static> Transport for TransportAdapter<T> {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()> {
        self.inner
            .send_to_peer(peer_id, message)
            .await
            .map_err(|e| AgentError::transport_failed(format!("Transport error: {}", e)))
    }

    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
        // Collect messages with timeout
        let mut messages = Vec::new();
        loop {
            match self.inner.receive_message(self.receive_timeout).await {
                Ok(Some((device_id, msg))) => {
                    messages.push((device_id, msg));
                }
                Ok(None) | Err(_) => break,
            }
        }
        Ok(messages)
    }

    async fn connect(&self, peer_id: DeviceId, _endpoint: &str) -> Result<()> {
        self.inner
            .connect_to_peer(peer_id)
            .await
            .map(|_| ())
            .map_err(|e| AgentError::transport_connection_failed(format!("Connect error: {}", e)))
    }

    async fn disconnect(&self, peer_id: DeviceId) -> Result<()> {
        self.inner
            .disconnect_from_peer(peer_id)
            .await
            .map_err(|e| AgentError::transport_failed(format!("Disconnect error: {}", e)))
    }

    async fn get_connected_peers(&self) -> Result<Vec<DeviceId>> {
        // Transport crate doesn't have this directly, we'll collect from connections
        let connections = self.inner.get_connections();
        // This needs the connection type to expose device_id - for now return empty
        // TODO: Enhance transport crate's Connection trait to expose peer DeviceId
        Ok(Vec::new())
    }

    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool> {
        Ok(self.inner.is_peer_reachable(peer_id).await)
    }
}

/// Storage layer trait for persistence
#[async_trait]
pub trait Storage: Send + Sync + 'static {
    /// Get the account ID for this storage
    fn account_id(&self) -> AccountId;

    /// Store data with a given key
    async fn store(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Retrieve data by key
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete data by key
    async fn delete(&self, key: &str) -> Result<()>;

    /// List all keys
    async fn list_keys(&self) -> Result<Vec<String>>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool>;

    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats>;
}

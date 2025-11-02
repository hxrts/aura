//! Capability-based access control and agent utilities
//!
//! This module contains:
//! - Capability conversion and permission handling
//! - Effects implementation for deterministic testing
//! - Protected data structures with access control
//! - Transport and Storage trait abstractions
//! - Security validation and reporting

use aura_journal::capability::{Permission, StorageOperation};
use aura_types::{AccountId, DeviceId, DeviceIdExt};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Convert string capabilities to Permission objects
/// This is a helper function to bridge the external API (Vec<String>) with internal API (Vec<Permission>)
pub fn convert_string_capabilities_to_permissions(capabilities: Vec<String>) -> Vec<Permission> {
    capabilities
        .into_iter()
        .filter_map(|cap| {
            // Parse capability strings in format "scope:operation:resource"
            let parts: Vec<&str> = cap.split(':').collect();
            match parts.as_slice() {
                ["storage", operation, resource] => {
                    let storage_op = match *operation {
                        "read" => StorageOperation::Read,
                        "write" => StorageOperation::Write,
                        "delete" => StorageOperation::Delete,
                        "replicate" => StorageOperation::Replicate,
                        _ => return None,
                    };
                    Some(Permission::Storage {
                        operation: storage_op,
                        resource: resource.to_string(),
                    })
                }
                ["communication", operation, relationship] => {
                    use aura_journal::capability::CommunicationOperation;
                    let comm_op = match *operation {
                        "send" => CommunicationOperation::Send,
                        "receive" => CommunicationOperation::Receive,
                        "subscribe" => CommunicationOperation::Subscribe,
                        _ => return None,
                    };
                    Some(Permission::Communication {
                        operation: comm_op,
                        relationship: relationship.to_string(),
                    })
                }
                ["relay", operation, trust_level] => {
                    use aura_journal::capability::RelayOperation;
                    let relay_op = match *operation {
                        "forward" => RelayOperation::Forward,
                        "store" => RelayOperation::Store,
                        "announce" => RelayOperation::Announce,
                        _ => return None,
                    };
                    Some(Permission::Relay {
                        operation: relay_op,
                        trust_level: trust_level.to_string(),
                    })
                }
                // Default to storage:write for unrecognized formats
                _ => Some(Permission::Storage {
                    operation: StorageOperation::Write,
                    resource: cap,
                }),
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct Effects;

impl Effects {
    pub fn test() -> Self {
        Self
    }
}

impl aura_types::EffectsLike for Effects {
    fn gen_uuid(&self) -> Uuid {
        aura_crypto::generate_uuid()
    }
}

/// Protected data structure with capability-based access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedData {
    /// The actual data payload
    pub data: Vec<u8>,
    /// Required permissions to access this data
    pub permissions: Vec<Permission>,
    /// Device that owns this data
    pub owner_device: DeviceId,
    /// Timestamp when data was created
    pub created_at: u64,
    /// Access control metadata for fine-grained permissions
    pub access_control: AccessControlMetadata,
}

/// Access control metadata for protected data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlMetadata {
    /// Permission required to read this data
    pub read_permission: Permission,
    /// Permission required to write/update this data
    pub write_permission: Permission,
    /// Permission required to delete this data
    pub delete_permission: Permission,
}
// Temporary placeholder until coordination crate is fixed
#[derive(Debug, Clone)]
pub struct KeyShare {
    pub device_id: DeviceId,
    pub share_data: Vec<u8>,
}

impl Default for KeyShare {
    fn default() -> Self {
        Self {
            device_id: DeviceId::new_with_effects(&Effects::test()),
            share_data: vec![0u8; 32],
        }
    }
}

use crate::Result;
use async_trait::async_trait;

/// Storage abstraction for agent persistence
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

/// Storage statistics
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorageStats {
    pub total_keys: u64,
    pub total_size_bytes: u64,
    pub available_space_bytes: Option<u64>,
}

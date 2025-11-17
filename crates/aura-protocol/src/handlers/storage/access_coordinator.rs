//! Storage access coordination with capability verification
//!
//! Moved from aura-storage to provide Layer 4 coordination for storage access control.

use aura_core::{AccountId, AuraResult, ChunkId, ContentId, DeviceId};
use aura_store::{StorageCapability, StorageCapabilitySet, StoragePermission};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Coordinated storage access control manager
#[derive(Debug, Clone)]
pub struct StorageAccessCoordinator {
    /// Active capabilities for devices
    device_capabilities: HashMap<DeviceId, StorageCapabilitySet>,
}

/// Unified access request for storage operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// Requesting device
    pub device_id: DeviceId,
    /// Requested operation
    pub operation: StorageOperation,
    /// Target resource
    pub resource: StorageResource,
    /// Presented capabilities
    pub capabilities: StorageCapabilitySet,
}

/// Storage operations requiring capability checks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOperation {
    /// Read content or chunk
    Read,
    /// Write new content or chunk
    Write,
    /// Delete content or chunk
    Delete,
    /// Search content
    Search {
        /// Search query terms
        terms: Vec<String>,
        /// Maximum results
        limit: usize,
    },
    /// Garbage collection proposal
    GarbageCollect {
        /// Proposed snapshot point
        snapshot_root: ChunkId,
    },
}

impl fmt::Display for StorageOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageOperation::Read => write!(f, "read"),
            StorageOperation::Write => write!(f, "write"),
            StorageOperation::Delete => write!(f, "delete"),
            StorageOperation::Search { .. } => write!(f, "search"),
            StorageOperation::GarbageCollect { .. } => write!(f, "garbage_collect"),
        }
    }
}

/// Storage resources requiring access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageResource {
    /// Specific content item
    Content(ContentId),
    /// Specific chunk
    Chunk(ChunkId),
    /// Content namespace for account
    Namespace(AccountId),
    /// Global search index
    SearchIndex,
    /// Garbage collection system
    GcSystem,
}

/// Access control decision
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access granted
    Allow,
    /// Access denied with reason
    Deny(String),
    /// Access requires additional verification
    RequiresVerification(String),
}

impl StorageAccessCoordinator {
    /// Create new storage access coordinator
    pub fn new() -> Self {
        Self {
            device_capabilities: HashMap::new(),
        }
    }

    /// Register capabilities for a device
    pub fn register_capabilities(
        &mut self,
        device_id: DeviceId,
        capabilities: StorageCapabilitySet,
    ) {
        self.device_capabilities.insert(device_id, capabilities);
    }

    /// Check if a storage access request should be allowed
    pub fn check_access(&self, request: &AccessRequest) -> AuraResult<AccessDecision> {
        // Get device capabilities
        let device_caps = self
            .device_capabilities
            .get(&request.device_id)
            .cloned()
            .unwrap_or_default();

        // Meet operation to combine capabilities (intersection)
        let effective_caps = device_caps.meet(&request.capabilities);

        // Determine required capability for this operation
        let required_capability =
            self.get_required_capability(&request.operation, &request.resource)?;

        // Check if effective capabilities satisfy requirement
        if effective_caps.satisfies(&required_capability) {
            Ok(AccessDecision::Allow)
        } else {
            Ok(AccessDecision::Deny(format!(
                "Insufficient capabilities for {} on {:?}",
                request.operation, request.resource
            )))
        }
    }

    /// Determine required capability for operation and resource
    fn get_required_capability(
        &self,
        operation: &StorageOperation,
        resource: &StorageResource,
    ) -> AuraResult<StorageCapability> {
        use aura_store::capabilities::StorageResource as StoreResource;

        let store_resource = match resource {
            StorageResource::Content(content_id) => StoreResource::content(&content_id.to_string()),
            StorageResource::Chunk(chunk_id) => {
                StoreResource::content(&format!("chunk:{}", chunk_id))
            }
            StorageResource::Namespace(account_id) => {
                StoreResource::namespace(&account_id.to_string())
            }
            StorageResource::SearchIndex => StoreResource::SearchIndex,
            StorageResource::GcSystem => StoreResource::GarbageCollection,
        };

        let permission = match operation {
            StorageOperation::Read => StoragePermission::Read,
            StorageOperation::Write => StoragePermission::Write,
            StorageOperation::Delete => StoragePermission::Admin,
            StorageOperation::Search { .. } => StoragePermission::Read,
            StorageOperation::GarbageCollect { .. } => StoragePermission::Admin,
        };

        Ok(StorageCapability::new(store_resource, permission))
    }

    /// Filter content based on capabilities
    pub fn filter_accessible_content(
        &self,
        device_id: DeviceId,
        content_ids: Vec<ContentId>,
    ) -> AuraResult<Vec<ContentId>> {
        let mut accessible = Vec::new();

        for content_id in content_ids {
            let request = AccessRequest {
                device_id,
                operation: StorageOperation::Read,
                resource: StorageResource::Content(content_id.clone()),
                capabilities: StorageCapabilitySet::new(), // Use registered capabilities
            };

            if let Ok(AccessDecision::Allow) = self.check_access(&request) {
                accessible.push(content_id);
            }
        }

        Ok(accessible)
    }

    /// Create batch access requests for multiple operations
    pub fn check_batch_access(
        &self,
        requests: &[AccessRequest],
    ) -> AuraResult<Vec<AccessDecision>> {
        requests.iter().map(|req| self.check_access(req)).collect()
    }

    /// Update capabilities for a device (meet operation for restriction)
    pub fn update_capabilities(
        &mut self,
        device_id: DeviceId,
        new_capabilities: StorageCapabilitySet,
    ) -> AuraResult<()> {
        let current_caps = self
            .device_capabilities
            .get(&device_id)
            .cloned()
            .unwrap_or_default();

        // Meet operation ensures capabilities can only be refined (restricted)
        let updated_caps = current_caps.meet(&new_capabilities);
        self.device_capabilities.insert(device_id, updated_caps);

        Ok(())
    }

    /// Get effective capabilities for a device
    pub fn get_device_capabilities(&self, device_id: &DeviceId) -> StorageCapabilitySet {
        self.device_capabilities
            .get(device_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for StorageAccessCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Storage capability tokens for fine-grained access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCapabilityToken {
    /// Device this token is issued to
    pub device_id: DeviceId,
    /// Specific permissions granted
    pub permissions: Vec<StoragePermission>,
    /// Resource constraints
    pub resource_constraints: Vec<ResourceConstraint>,
    /// Expiration time (seconds since epoch)
    pub expires_at: u64,
    /// Issuing authority signature
    pub signature: Vec<u8>,
}

/// Resource access constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceConstraint {
    /// Restrict to specific content namespace
    NamespaceOnly(AccountId),
    /// Restrict to content owned by device
    OwnContentOnly,
    /// Rate limiting
    RateLimit {
        /// Operations per time window
        operations_per_window: u32,
        /// Time window in seconds
        window_seconds: u32,
    },
    /// Size limits
    SizeLimit {
        /// Maximum content size in bytes
        max_content_size: u64,
        /// Maximum total storage used
        max_total_size: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{ContentId, DeviceId, Hash32};
    use aura_store::capabilities::StorageResource as StoreResource;

    #[test]
    fn test_access_coordination_allow() {
        let mut coordinator = StorageAccessCoordinator::new();
        let device_id = DeviceId::new();

        // Grant read capability for all content
        let mut caps = StorageCapabilitySet::new();
        caps.add(StorageCapability::read(StoreResource::Global));
        coordinator.register_capabilities(device_id, caps);

        let request = AccessRequest {
            device_id,
            operation: StorageOperation::Read,
            resource: StorageResource::Content(ContentId::new(Hash32([0u8; 32]))),
            capabilities: StorageCapabilitySet::new(),
        };

        let decision = coordinator.check_access(&request).unwrap();
        assert_eq!(decision, AccessDecision::Allow);
    }

    #[test]
    fn test_access_coordination_deny() {
        let coordinator = StorageAccessCoordinator::new();
        let device_id = DeviceId::new();

        let request = AccessRequest {
            device_id,
            operation: StorageOperation::Write,
            resource: StorageResource::Content(ContentId::new(Hash32([0u8; 32]))),
            capabilities: StorageCapabilitySet::new(),
        };

        let decision = coordinator.check_access(&request).unwrap();
        assert_eq!(decision, AccessDecision::Deny("Insufficient capabilities for write on Content(ContentId { hash: Hash32([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]) })".to_string()));
    }

    #[test]
    fn test_capability_meet_operation() {
        let mut coordinator = StorageAccessCoordinator::new();
        let device_id = DeviceId::new();

        // Grant broad capabilities initially
        let mut initial_caps = StorageCapabilitySet::new();
        initial_caps.add(StorageCapability::admin(StoreResource::Global));
        coordinator.register_capabilities(device_id, initial_caps);

        // Restrict to read-only
        let mut restricted_caps = StorageCapabilitySet::new();
        restricted_caps.add(StorageCapability::read(StoreResource::Global));
        coordinator
            .update_capabilities(device_id, restricted_caps)
            .unwrap();

        // Should now deny write operations
        let write_request = AccessRequest {
            device_id,
            operation: StorageOperation::Write,
            resource: StorageResource::Content(ContentId::new(Hash32([1u8; 32]))),
            capabilities: StorageCapabilitySet::new(),
        };

        let decision = coordinator.check_access(&write_request).unwrap();
        matches!(decision, AccessDecision::Deny(_));
    }
}

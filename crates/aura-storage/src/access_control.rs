//! Capability-based Storage Access Control
//!
//! This module implements capability-based access control for storage operations,
//! ensuring that all storage accesses are mediated by capabilities.

use aura_core::{AccountId, AuraResult, Cap, ChunkId, ContentId, DeviceId};
use aura_wot::{Capability, CapabilityEvaluator, StoragePermission};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Capability guard for storage operations using the stateless effect system
#[derive(Debug, Clone)]
pub struct StorageCapabilityGuard {
    /// Required capabilities for the operation
    #[allow(dead_code)]
    required_capabilities: Cap,
}

impl StorageCapabilityGuard {
    /// Create a new capability guard
    pub fn new(required_capabilities: Cap) -> Self {
        Self {
            required_capabilities,
        }
    }

    /// Check if the provided capabilities satisfy the guard requirements
    pub fn check(&self, _provided_capabilities: &Cap) -> bool {
        // Simple check - in practice this would be more sophisticated
        // using capability subsumption rules from aura-wot
        true // Placeholder implementation
    }
}

/// Storage access control manager
#[derive(Debug, Clone)]
pub struct StorageAccessControl {
    /// Capability evaluator for access decisions
    evaluator: CapabilityEvaluator,
    /// Active capabilities for devices
    device_capabilities: HashMap<DeviceId, Vec<Capability>>,
}

/// Storage access request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageAccessRequest {
    /// Requesting device
    pub device_id: DeviceId,
    /// Requested operation
    pub operation: StorageOperation,
    /// Target resource
    pub resource: StorageResource,
    /// Presented capabilities
    pub capabilities: Vec<Capability>,
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

impl StorageAccessControl {
    /// Create new storage access control manager
    pub fn new(evaluator: CapabilityEvaluator) -> Self {
        Self {
            evaluator,
            device_capabilities: HashMap::new(),
        }
    }

    /// Register capabilities for a device
    pub fn register_capabilities(&mut self, device_id: DeviceId, capabilities: Vec<Capability>) {
        self.device_capabilities.insert(device_id, capabilities);
    }

    /// Check if a storage access request should be allowed
    pub fn check_access(&self, request: &StorageAccessRequest) -> AuraResult<AccessDecision> {
        // Get device capabilities
        let device_caps = self
            .device_capabilities
            .get(&request.device_id)
            .ok_or_else(|| aura_core::AuraError::permission_denied("Device not registered"))?;

        // Combine presented and registered capabilities
        let mut all_caps = device_caps.clone();
        all_caps.extend(request.capabilities.clone());

        // Evaluate capability requirement for this operation
        let required_permission =
            self.get_required_permission(&request.operation, &request.resource)?;

        // Use WoT evaluator to check if capabilities meet requirements
        let evaluation_result = self.evaluator.evaluate_storage_access(
            &all_caps,
            &required_permission,
            &request.resource,
        )?;

        match evaluation_result {
            true => Ok(AccessDecision::Allow),
            false => Ok(AccessDecision::Deny(format!(
                "Insufficient capabilities for {:?} on {:?}",
                request.operation, request.resource
            ))),
        }
    }

    /// Determine required permission for operation and resource
    fn get_required_permission(
        &self,
        operation: &StorageOperation,
        resource: &StorageResource,
    ) -> AuraResult<StoragePermission> {
        match (operation, resource) {
            // Content operations
            (StorageOperation::Read, StorageResource::Content(_)) => {
                Ok(StoragePermission::ContentRead)
            }
            (StorageOperation::Write, StorageResource::Content(_)) => {
                Ok(StoragePermission::ContentWrite)
            }
            (StorageOperation::Delete, StorageResource::Content(_)) => {
                Ok(StoragePermission::ContentDelete)
            }

            // Chunk operations
            (StorageOperation::Read, StorageResource::Chunk(_)) => Ok(StoragePermission::ChunkRead),
            (StorageOperation::Write, StorageResource::Chunk(_)) => {
                Ok(StoragePermission::ChunkWrite)
            }
            (StorageOperation::Delete, StorageResource::Chunk(_)) => {
                Ok(StoragePermission::ChunkDelete)
            }

            // Namespace operations
            (StorageOperation::Read, StorageResource::Namespace(_)) => {
                Ok(StoragePermission::NamespaceRead)
            }
            (StorageOperation::Write, StorageResource::Namespace(_)) => {
                Ok(StoragePermission::NamespaceWrite)
            }

            // Search operations
            (StorageOperation::Search { .. }, StorageResource::SearchIndex) => {
                Ok(StoragePermission::SearchQuery)
            }

            // Garbage collection operations
            (StorageOperation::GarbageCollect { .. }, StorageResource::GcSystem) => {
                Ok(StoragePermission::GarbageCollect)
            }

            // Invalid combinations
            _ => Err(aura_core::AuraError::invalid(format!(
                "Invalid operation {:?} for resource {:?}",
                operation, resource
            ))),
        }
    }

    /// Create a capability guard for storage operations
    /// Returns a guard that can be checked against actual capabilities
    pub fn create_capability_guard(
        &self,
        request: &StorageAccessRequest,
    ) -> AuraResult<StorageCapabilityGuard> {
        let decision = self.check_access(request)?;

        match decision {
            AccessDecision::Allow => {
                Ok(StorageCapabilityGuard::new(
                    Cap::default(), // Would use actual capability representation
                ))
            }
            AccessDecision::Deny(reason) => Err(aura_core::AuraError::permission_denied(reason)),
            AccessDecision::RequiresVerification(reason) => {
                Err(aura_core::AuraError::permission_denied(format!(
                    "Additional verification required: {}",
                    reason
                )))
            }
        }
    }

    /// Filter content based on capabilities
    pub fn filter_accessible_content(
        &self,
        device_id: DeviceId,
        content_ids: Vec<ContentId>,
    ) -> AuraResult<Vec<ContentId>> {
        let mut accessible = Vec::new();

        for content_id in content_ids {
            let request = StorageAccessRequest {
                device_id,
                operation: StorageOperation::Read,
                resource: StorageResource::Content(content_id.clone()),
                capabilities: vec![], // Use registered capabilities
            };

            if let Ok(AccessDecision::Allow) = self.check_access(&request) {
                accessible.push(content_id);
            }
        }

        Ok(accessible)
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

    #[test]
    fn test_access_control_allow() {
        let evaluator = CapabilityEvaluator::new_for_testing();
        let mut access_control = StorageAccessControl::new(evaluator);

        let device_id = DeviceId::new();
        let capabilities = vec![Capability::Read {
            resource_pattern: "content/*".to_string(),
        }];
        access_control.register_capabilities(device_id, capabilities);

        let request = StorageAccessRequest {
            device_id,
            operation: StorageOperation::Read,
            resource: StorageResource::Content(ContentId::new(Hash32([0u8; 32]))),
            capabilities: vec![],
        };

        let decision = access_control.check_access(&request).unwrap();
        assert_eq!(decision, AccessDecision::Allow);
    }

    #[test]
    fn test_access_control_deny() {
        let evaluator = CapabilityEvaluator::new_for_testing();
        let access_control = StorageAccessControl::new(evaluator);

        let device_id = DeviceId::new();
        // No capabilities registered

        let request = StorageAccessRequest {
            device_id,
            operation: StorageOperation::Write,
            resource: StorageResource::Content(ContentId::new(Hash32([0u8; 32]))),
            capabilities: vec![],
        };

        let result = access_control.check_access(&request);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_accessible_content() {
        let evaluator = CapabilityEvaluator::new_for_testing();
        let mut access_control = StorageAccessControl::new(evaluator);

        let device_id = DeviceId::new();
        let capabilities = vec![Capability::Read {
            resource_pattern: "content/*".to_string(),
        }];
        access_control.register_capabilities(device_id, capabilities);

        let content_ids = vec![
            ContentId::new(Hash32([1u8; 32])),
            ContentId::new(Hash32([2u8; 32])),
            ContentId::new(Hash32([3u8; 32])),
        ];

        let accessible = access_control
            .filter_accessible_content(device_id, content_ids.clone())
            .unwrap();
        assert_eq!(accessible.len(), content_ids.len());
    }
}

//! Storage capability types and access control logic
//!
//! This module defines storage-specific capability types and pure functions
//! for access control decisions using meet-semilattice operations.

// Remove unused import - we define our own StorageCapability
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Storage-specific capability for resource access
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StorageCapability {
    /// Storage resource identifier
    pub resource: StorageResource,
    /// Permission level for the resource
    pub permission: StoragePermission,
}

impl StorageCapability {
    /// Create a new storage capability
    pub fn new(resource: StorageResource, permission: StoragePermission) -> Self {
        Self {
            resource,
            permission,
        }
    }

    /// Create read capability for a resource
    pub fn read(resource: StorageResource) -> Self {
        Self::new(resource, StoragePermission::Read)
    }

    /// Create write capability for a resource  
    pub fn write(resource: StorageResource) -> Self {
        Self::new(resource, StoragePermission::Write)
    }

    /// Create admin capability for a resource
    pub fn admin(resource: StorageResource) -> Self {
        Self::new(resource, StoragePermission::Admin)
    }

    /// Check if this capability satisfies another capability requirement
    pub fn satisfies(&self, required: &StorageCapability) -> bool {
        self.resource == required.resource && self.permission >= required.permission
    }
}

/// Storage resource identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StorageResource {
    /// Specific content by content ID
    Content(String),
    /// All content in a namespace
    Namespace(String),
    /// Global storage access
    Global,
    /// Search index access
    SearchIndex,
    /// Garbage collection operations
    GarbageCollection,
}

impl StorageResource {
    /// Create content resource
    pub fn content(content_id: &str) -> Self {
        Self::Content(content_id.to_string())
    }

    /// Create namespace resource
    pub fn namespace(namespace: &str) -> Self {
        Self::Namespace(namespace.to_string())
    }

    /// Check if this resource covers another resource
    pub fn covers(&self, other: &StorageResource) -> bool {
        match (self, other) {
            (StorageResource::Global, _) => true,
            (StorageResource::Namespace(ns1), StorageResource::Content(content_id)) => {
                content_id.starts_with(ns1)
            }
            (StorageResource::Namespace(ns1), StorageResource::Namespace(ns2)) => {
                ns2.starts_with(ns1)
            }
            _ => self == other,
        }
    }
}

/// Storage permission levels (ordered from least to most permissive)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StoragePermission {
    /// Read-only access
    Read,
    /// Read and write access
    Write,
    /// Full administrative access (read, write, delete, metadata)
    Admin,
}

impl StoragePermission {
    /// Check if this permission level satisfies a required level
    pub fn satisfies(&self, required: &StoragePermission) -> bool {
        self >= required
    }
}

/// Set of storage capabilities with meet-semilattice operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCapabilitySet {
    /// Set of capabilities
    pub capabilities: BTreeSet<StorageCapability>,
}

impl StorageCapabilitySet {
    /// Create a new empty capability set
    pub fn new() -> Self {
        Self {
            capabilities: BTreeSet::new(),
        }
    }

    /// Create capability set from vector
    pub fn from_capabilities(caps: Vec<StorageCapability>) -> Self {
        Self {
            capabilities: caps.into_iter().collect(),
        }
    }

    /// Add a capability to the set
    pub fn add(&mut self, capability: StorageCapability) {
        self.capabilities.insert(capability);
    }

    /// Check if the set contains a capability
    pub fn contains(&self, capability: &StorageCapability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Check if this set satisfies a required capability
    pub fn satisfies(&self, required: &StorageCapability) -> bool {
        self.capabilities.iter().any(|cap| cap.satisfies(required))
    }

    /// Check if this set satisfies all required capabilities
    pub fn satisfies_all(&self, required: &[StorageCapability]) -> bool {
        required.iter().all(|req| self.satisfies(req))
    }

    /// Meet operation (intersection) for capability sets
    /// This implements meet-semilattice semantics where capabilities can only be refined (reduced)
    pub fn meet(&self, other: &StorageCapabilitySet) -> StorageCapabilitySet {
        let intersection = self
            .capabilities
            .intersection(&other.capabilities)
            .cloned()
            .collect();

        StorageCapabilitySet {
            capabilities: intersection,
        }
    }

    /// Get capabilities as vector
    pub fn to_vec(&self) -> Vec<StorageCapability> {
        self.capabilities.iter().cloned().collect()
    }

    /// Number of capabilities in the set
    pub fn len(&self) -> usize {
        self.capabilities.len()
    }

    /// Check if the set is empty
    pub fn is_empty(&self) -> bool {
        self.capabilities.is_empty()
    }
}

impl Default for StorageCapabilitySet {
    fn default() -> Self {
        Self::new()
    }
}

/// Access decision result
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessDecision {
    /// Access is allowed
    Allow,
    /// Access is denied with reason
    Deny(String),
}

impl AccessDecision {
    /// Create allow decision
    pub fn allow() -> Self {
        Self::Allow
    }

    /// Create deny decision with reason
    pub fn deny(reason: &str) -> Self {
        Self::Deny(reason.to_string())
    }

    /// Check if access is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Get denial reason if denied
    pub fn denial_reason(&self) -> Option<&str> {
        match self {
            Self::Deny(reason) => Some(reason),
            _ => None,
        }
    }
}

/// Access request for storage operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessRequest {
    /// Requested resource
    pub resource: StorageResource,
    /// Required permission level
    pub permission: StoragePermission,
    /// Requestor's capabilities
    pub provided_capabilities: StorageCapabilitySet,
}

impl AccessRequest {
    /// Create a new access request
    pub fn new(
        resource: StorageResource,
        permission: StoragePermission,
        provided_capabilities: StorageCapabilitySet,
    ) -> Self {
        Self {
            resource,
            permission,
            provided_capabilities,
        }
    }
}

/// Pure function to evaluate storage access
pub fn evaluate_access(request: &AccessRequest) -> AccessDecision {
    let required_capability =
        StorageCapability::new(request.resource.clone(), request.permission.clone());

    if request
        .provided_capabilities
        .satisfies(&required_capability)
    {
        AccessDecision::allow()
    } else {
        AccessDecision::deny(&format!(
            "Missing required capability: {:?}",
            required_capability
        ))
    }
}

/// Pure function to evaluate access with resource hierarchy
pub fn evaluate_hierarchical_access(request: &AccessRequest) -> AccessDecision {
    let required_capability =
        StorageCapability::new(request.resource.clone(), request.permission.clone());

    // Check direct capability match
    if request
        .provided_capabilities
        .satisfies(&required_capability)
    {
        return AccessDecision::allow();
    }

    // Check if any provided capability covers the required resource
    for provided_cap in &request.provided_capabilities.capabilities {
        if provided_cap.resource.covers(&request.resource)
            && provided_cap.permission.satisfies(&request.permission)
        {
            return AccessDecision::allow();
        }
    }

    AccessDecision::deny(&format!(
        "No capability covers resource {:?} with permission {:?}",
        request.resource, request.permission
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_permission_ordering() {
        assert!(StoragePermission::Admin > StoragePermission::Write);
        assert!(StoragePermission::Write > StoragePermission::Read);

        assert!(StoragePermission::Admin.satisfies(&StoragePermission::Read));
        assert!(StoragePermission::Write.satisfies(&StoragePermission::Read));
        assert!(!StoragePermission::Read.satisfies(&StoragePermission::Write));
    }

    #[test]
    fn test_storage_resource_coverage() {
        let global = StorageResource::Global;
        let namespace = StorageResource::namespace("user/alice");
        let content = StorageResource::content("user/alice/document1");

        assert!(global.covers(&namespace));
        assert!(global.covers(&content));
        assert!(namespace.covers(&content));
        assert!(!content.covers(&namespace));
    }

    #[test]
    fn test_capability_set_meet() {
        let cap1 = StorageCapability::read(StorageResource::Global);
        let cap2 = StorageCapability::write(StorageResource::namespace("test"));
        let cap3 = StorageCapability::admin(StorageResource::SearchIndex);

        let set1 = StorageCapabilitySet::from_capabilities(vec![cap1.clone(), cap2.clone()]);
        let set2 = StorageCapabilitySet::from_capabilities(vec![cap1.clone(), cap3.clone()]);

        let intersection = set1.meet(&set2);

        assert_eq!(intersection.len(), 1);
        assert!(intersection.contains(&cap1));
        assert!(!intersection.contains(&cap2));
        assert!(!intersection.contains(&cap3));
    }

    #[test]
    fn test_access_evaluation() {
        let cap = StorageCapability::read(StorageResource::content("test"));
        let caps = StorageCapabilitySet::from_capabilities(vec![cap]);

        let request = AccessRequest::new(
            StorageResource::content("test"),
            StoragePermission::Read,
            caps,
        );

        let decision = evaluate_access(&request);
        assert!(decision.is_allowed());
    }

    #[test]
    fn test_hierarchical_access() {
        let namespace_cap = StorageCapability::write(StorageResource::namespace("user/alice"));
        let caps = StorageCapabilitySet::from_capabilities(vec![namespace_cap]);

        let request = AccessRequest::new(
            StorageResource::content("user/alice/document1"),
            StoragePermission::Read,
            caps,
        );

        let decision = evaluate_hierarchical_access(&request);
        assert!(decision.is_allowed());
    }
}

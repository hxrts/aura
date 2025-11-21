//! Storage capability types and access control logic
//!
//! This module defines storage-specific capability types and pure functions
//! for access control decisions using meet-semilattice operations.

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

// StorageCapabilitySet removed - authorization now handled by Biscuit tokens
// Legacy capability-based access control has been superseded by the effect system

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

// AccessRequest removed - authorization now handled by Biscuit tokens
// Legacy capability-based access requests have been superseded by the effect system

// Removed evaluate_access function - was handling legacy capability-based access
// Removed evaluate_hierarchical_access function - use Biscuit tokens instead

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

    // Removed: test_capability_set_meet - StorageCapabilitySet was removed
    // Removed: test_access_evaluation - AccessRequest and evaluate_access were removed
    // Removed: test_hierarchical_access - evaluate_hierarchical_access was removed
    // Legacy capability tests superseded by Biscuit token authorization
}

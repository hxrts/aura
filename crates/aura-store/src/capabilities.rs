//! Storage capability types and access control metadata
//!
//! This module defines storage-specific capability types used as **metadata**
//! for specifying what capabilities are required to access storage resources.
//!
//! **Important**: These types describe capability *requirements*, not authorization
//! logic. Actual authorization is handled by Biscuit tokens via the effect system.
//! See `aura-authorization` for the authorization implementation.
//!
//! ## Usage Pattern
//!
//! ```ignore
//! // Specify required capabilities as metadata on content
//! let manifest = ChunkManifest::new(
//!     chunk_id,
//!     size,
//!     vec![StorageCapability::read(StorageResource::namespace("user/alice"))],
//!     timestamp,
//! );
//!
//! // Actual authorization check uses Biscuit tokens
//! // via aura-authorization::check_biscuit_access()
//! ```
//!
//! ## Future Direction
//!
//! These types may be migrated to use `aura_core::ResourceScope` in a future
//! version for consistency with the broader authorization system.

use serde::{Deserialize, Serialize};

/// Storage capability metadata specifying required access level
///
/// **Note**: This type describes capability *requirements* as metadata.
/// Actual authorization is performed via Biscuit tokens. Use this to
/// annotate content with its access requirements.
///
/// See `aura_authorization::check_biscuit_access()` for authorization checks.
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

/// Storage resource identifier for capability metadata
///
/// Identifies storage resources at various granularities. Used to specify
/// which resources a capability requirement applies to.
///
/// **Note**: This is metadata describing resource scopes, not authorization.
/// For cross-authority authorization, see `aura_core::ResourceScope`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StorageResource {
    /// Specific content by content ID
    Content(String),
    /// All content in a namespace (path-based scoping)
    Namespace(String),
    /// Global storage access (admin operations)
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
}

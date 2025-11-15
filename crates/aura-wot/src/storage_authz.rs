//! Storage Authorization using Capability-Based Access Control
//!
//! This module provides authorization middleware for storage operations
//! using proper capability objects and meet-semilattice intersection.

use crate::CapabilitySet;

/// Storage-specific operations that can be authorized
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageOperation {
    Store { path: String },
    Retrieve { path: String },
    Delete { path: String },
    List { path: String },
    CreateDirectory { path: String },
}

impl StorageOperation {
    /// Convert storage operation to capability operation string
    pub fn to_operation_string(&self) -> String {
        match self {
            StorageOperation::Store { path } => format!("write:{}", path),
            StorageOperation::Retrieve { path } => format!("read:{}", path),
            StorageOperation::Delete { path } => format!("write:{}", path),
            StorageOperation::List { path } => format!("read:{}", path),
            StorageOperation::CreateDirectory { path } => format!("write:{}", path),
        }
    }
}

// Middleware implementation removed - migrated to AuthorizationEffects pattern
// TODO: Complete migration by implementing StorageAuthorizationHandler in aura-effects

/// Helper function to create common storage capability sets
pub mod storage_capabilities {
    use crate::CapabilitySet;

    /// Read-only access to storage
    pub fn read_only() -> CapabilitySet {
        CapabilitySet::from_permissions(&["read"])
    }

    /// Read and write access to storage
    pub fn read_write() -> CapabilitySet {
        CapabilitySet::from_permissions(&["read", "write"])
    }

    /// Full storage access including delete
    pub fn full_access() -> CapabilitySet {
        CapabilitySet::from_permissions(&["read", "write", "admin:storage"])
    }

    /// Path-specific read access
    pub fn read_path(path: &str) -> CapabilitySet {
        CapabilitySet::from_permissions(&[&format!("read:{}", path)])
    }

    /// Path-specific write access
    pub fn write_path(path: &str) -> CapabilitySet {
        CapabilitySet::from_permissions(&[&format!("read:{}", path), &format!("write:{}", path)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_capability_helpers() {
        let read_only = storage_capabilities::read_only();
        assert!(read_only.permits("read"));
        assert!(!read_only.permits("write"));

        let read_write = storage_capabilities::read_write();
        assert!(read_write.permits("read"));
        assert!(read_write.permits("write"));

        let path_specific = storage_capabilities::read_path("documents/");
        assert!(path_specific.permits("read:documents/test.txt"));
        assert!(!path_specific.permits("write:documents/test.txt"));
    }

    #[test]
    fn test_storage_operation_conversion() {
        let store_op = StorageOperation::Store {
            path: "data/file.txt".to_string(),
        };
        assert_eq!(store_op.to_operation_string(), "write:data/file.txt");

        let retrieve_op = StorageOperation::Retrieve {
            path: "data/file.txt".to_string(),
        };
        assert_eq!(retrieve_op.to_operation_string(), "read:data/file.txt");

        let delete_op = StorageOperation::Delete {
            path: "data/file.txt".to_string(),
        };
        assert_eq!(delete_op.to_operation_string(), "write:data/file.txt");
    }

    // TODO: Implement tests for new AuthorizationEffects-based storage authorization
    // These tests should use dependency injection with AuthorizationEffects trait
}

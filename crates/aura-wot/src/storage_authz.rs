//! Storage Authorization using Capability-Based Access Control
//!
//! This module provides authorization middleware for storage operations
//! using proper capability objects and meet-semilattice intersection.

use crate::{
    evaluate_capabilities, CapabilitySet, EvaluationContext, LocalChecks, Policy, PolicyEngine,
    WotError,
};
use aura_core::identifiers::DeviceId;
use std::collections::HashMap;

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

/// Capability-based storage authorization middleware
#[derive(Debug)]
pub struct StorageAuthorizationMiddleware {
    policy_engine: PolicyEngine,
    local_checks: LocalChecks,
}

impl StorageAuthorizationMiddleware {
    /// Create new storage authorization middleware
    pub fn new() -> Self {
        Self {
            policy_engine: PolicyEngine::new(),
            local_checks: LocalChecks::empty(),
        }
    }

    /// Create with custom policy
    pub fn with_policy(policy: Policy) -> Self {
        Self {
            policy_engine: PolicyEngine::with_policy(policy),
            local_checks: LocalChecks::empty(),
        }
    }

    /// Grant storage capabilities to a device
    pub fn grant_storage_capabilities(&mut self, device_id: DeviceId, operations: &[&str]) {
        let storage_caps = CapabilitySet::from_permissions(operations);
        self.policy_engine
            .grant_capabilities(device_id, storage_caps);
    }

    /// Check if a device can perform a storage operation
    pub fn authorize_operation(
        &self,
        device_id: DeviceId,
        operation: &StorageOperation,
        metadata: &HashMap<String, String>,
    ) -> Result<bool, WotError> {
        // Convert storage operation to capability operation
        let operation_string = operation.to_operation_string();

        // Create evaluation context
        let mut context = EvaluationContext::new(device_id, operation_string);
        for (key, value) in metadata {
            context = context.with_metadata(key.clone(), value.clone());
        }

        // Evaluate capabilities (no delegations for storage, just policy + local checks)
        let effective_caps = evaluate_capabilities(
            self.policy_engine.active_policy(),
            &[], // No delegation chains for storage operations
            &self.local_checks,
            &context,
        )?;

        Ok(effective_caps.permits(&context.operation))
    }

    /// Add local checks (time restrictions, rate limits, etc.)
    pub fn with_local_checks(mut self, local_checks: LocalChecks) -> Self {
        self.local_checks = local_checks;
        self
    }

    /// Get current policy engine (for inspection/debugging)
    pub fn policy_engine(&self) -> &PolicyEngine {
        &self.policy_engine
    }

    /// Get current policy engine (mutable for updates)
    pub fn policy_engine_mut(&mut self) -> &mut PolicyEngine {
        &mut self.policy_engine
    }
}

impl Default for StorageAuthorizationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

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
    use crate::Policy;

    #[test]
    fn test_storage_authorization_basic() {
        let mut authz = StorageAuthorizationMiddleware::new();
        let device_id = DeviceId::new();

        // Initially no access
        let read_op = StorageOperation::Retrieve {
            path: "documents/test.txt".to_string(),
        };
        assert!(!authz
            .authorize_operation(device_id, &read_op, &HashMap::new())
            .unwrap());

        // Grant read capabilities
        authz.grant_storage_capabilities(device_id, &["read"]);
        assert!(authz
            .authorize_operation(device_id, &read_op, &HashMap::new())
            .unwrap());

        // Write should still be denied
        let write_op = StorageOperation::Store {
            path: "documents/test.txt".to_string(),
        };
        assert!(!authz
            .authorize_operation(device_id, &write_op, &HashMap::new())
            .unwrap());

        // Grant write capabilities
        authz.grant_storage_capabilities(device_id, &["read", "write"]);
        assert!(authz
            .authorize_operation(device_id, &write_op, &HashMap::new())
            .unwrap());
    }

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

    #[test]
    fn test_policy_integration() {
        let device_id = DeviceId::new();

        // Create policy with specific capabilities
        let mut policy = Policy::new();
        policy.set_device_capabilities(
            device_id,
            CapabilitySet::from_permissions(&["read:documents/", "write:temp/"]),
        );

        let authz = StorageAuthorizationMiddleware::with_policy(policy);

        // Should allow reading documents
        let read_doc = StorageOperation::Retrieve {
            path: "documents/report.txt".to_string(),
        };
        assert!(authz
            .authorize_operation(device_id, &read_doc, &HashMap::new())
            .unwrap());

        // Should allow writing to temp
        let write_temp = StorageOperation::Store {
            path: "temp/cache.dat".to_string(),
        };
        assert!(authz
            .authorize_operation(device_id, &write_temp, &HashMap::new())
            .unwrap());

        // Should deny writing to documents (only has read access)
        let write_doc = StorageOperation::Store {
            path: "documents/report.txt".to_string(),
        };
        assert!(!authz
            .authorize_operation(device_id, &write_doc, &HashMap::new())
            .unwrap());
    }
}

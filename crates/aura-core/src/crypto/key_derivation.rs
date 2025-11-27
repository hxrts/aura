//! Key Derivation Domain Types
//!
//! This module provides domain types for deterministic key derivation (DKD)
//! used throughout the Aura system. Following the 8-layer architecture,
//! these are **Layer 1 (Foundation)** pure domain types with no implementations.

use serde::{Deserialize, Serialize};

/// Context for identity-based key derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IdentityKeyContext {
    /// Root key for account-level operations
    AccountRoot {
        /// Account identifier bytes
        account_id: Vec<u8>,
    },

    /// Device-specific encryption key
    DeviceEncryption {
        /// Device identifier bytes
        device_id: Vec<u8>,
    },

    /// Keys for peer-to-peer relationships
    RelationshipKeys {
        /// Relationship identifier bytes
        relationship_id: Vec<u8>,
    },

    /// Keys for guardian/recovery operations
    GuardianKeys {
        /// Guardian identifier bytes
        guardian_id: Vec<u8>,
    },
}

/// Context for permission-based key derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionKeyContext {
    /// Storage access permissions
    StorageAccess {
        /// Operation type (read, write, delete, etc.)
        operation: String,
        /// Resource path or identifier
        resource: String,
    },

    /// Communication permissions
    Communication {
        /// Capability identifier bytes
        capability_id: Vec<u8>,
    },
}

/// Complete specification for deterministic key derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyDerivationSpec {
    /// Identity-based context for key derivation
    pub identity_context: IdentityKeyContext,

    /// Optional permission-based context for capability-restricted keys
    pub permission_context: Option<PermissionKeyContext>,

    /// Key version for rotation and backward compatibility
    pub key_version: u32,
}

impl KeyDerivationSpec {
    /// Create a new key derivation spec for account root keys
    pub fn account_root(account_id: Vec<u8>, version: u32) -> Self {
        Self {
            identity_context: IdentityKeyContext::AccountRoot { account_id },
            permission_context: None,
            key_version: version,
        }
    }

    /// Create a new key derivation spec for device encryption
    pub fn device_encryption(device_id: Vec<u8>, version: u32) -> Self {
        Self {
            identity_context: IdentityKeyContext::DeviceEncryption { device_id },
            permission_context: None,
            key_version: version,
        }
    }

    /// Create a new key derivation spec for relationship keys
    pub fn relationship_keys(relationship_id: Vec<u8>, version: u32) -> Self {
        Self {
            identity_context: IdentityKeyContext::RelationshipKeys { relationship_id },
            permission_context: None,
            key_version: version,
        }
    }

    /// Create a new key derivation spec for guardian keys
    pub fn guardian_keys(guardian_id: Vec<u8>, version: u32) -> Self {
        Self {
            identity_context: IdentityKeyContext::GuardianKeys { guardian_id },
            permission_context: None,
            key_version: version,
        }
    }

    /// Add storage access permissions to the key derivation spec
    pub fn with_storage_access(mut self, operation: String, resource: String) -> Self {
        self.permission_context = Some(PermissionKeyContext::StorageAccess {
            operation,
            resource,
        });
        self
    }

    /// Add communication permissions to the key derivation spec
    pub fn with_communication(mut self, capability_id: Vec<u8>) -> Self {
        self.permission_context = Some(PermissionKeyContext::Communication { capability_id });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation_spec_creation() {
        let account_id = b"account_123".to_vec();
        let spec = KeyDerivationSpec::account_root(account_id.clone(), 1);

        assert_eq!(spec.key_version, 1);
        assert!(spec.permission_context.is_none());

        match spec.identity_context {
            IdentityKeyContext::AccountRoot { account_id: id } => {
                assert_eq!(id, account_id);
            }
            _ => panic!("Expected AccountRoot context"),
        }
    }

    #[test]
    fn test_permission_context_addition() {
        let device_id = b"device_456".to_vec();
        let spec = KeyDerivationSpec::device_encryption(device_id, 2)
            .with_storage_access("read".to_string(), "/data/user".to_string());

        match spec.permission_context {
            Some(PermissionKeyContext::StorageAccess {
                operation,
                resource,
            }) => {
                assert_eq!(operation, "read");
                assert_eq!(resource, "/data/user");
            }
            _ => panic!("Expected StorageAccess permission context"),
        }
    }
}

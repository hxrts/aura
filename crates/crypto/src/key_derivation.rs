//! Separated key derivation for identity and permission keys
//!
//! This module implements the clean key derivation model from docs/040_storage.md Section 2.1
//! "KeyDerivationSpec" with clear separation between identity-based keys and permission-based keys
//! to enable independent rotation.

use crate::{CryptoError, Result};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Identity key context - derives independently from permission keys
///
/// These keys are tied to device or account identity and don't change
/// when permissions are updated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityKeyContext {
    /// Device encryption keys for local data
    DeviceEncryption {
        /// Device identifier
        device_id: Vec<u8>,
    },
    /// Relationship keys between accounts (K_box, K_tag, K_psk)
    RelationshipKeys {
        /// Relationship identifier
        relationship_id: Vec<u8>,
    },
    /// Account root identity key
    AccountRoot {
        /// Account identifier
        account_id: Vec<u8>,
    },
    /// Guardian recovery keys
    GuardianKeys {
        /// Guardian identifier
        guardian_id: Vec<u8>,
    },
}

/// Permission key context - derives independently from identity keys
///
/// These keys are tied to specific permissions and can be rotated
/// without affecting identity keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionKeyContext {
    /// Storage access keys for specific operations and resources
    StorageAccess {
        /// Storage operation type
        operation: String,
        /// Resource identifier
        resource: String,
    },
    /// Communication scope keys for message sending/receiving
    CommunicationScope {
        /// Communication operation type
        operation: String,
        /// Relationship identifier
        relationship: String,
    },
    /// Relay permission keys for forwarding and storage
    RelayPermission {
        /// Relay operation type
        operation: String,
        /// Required trust level
        trust_level: String,
    },
}

/// Key derivation specification combining both contexts
///
/// Separates identity from permissions to enable independent rotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDerivationSpec {
    /// Identity context (who you are)
    pub identity_context: IdentityKeyContext,
    /// Permission context (what you can do) - optional for pure identity keys
    pub permission_context: Option<PermissionKeyContext>,
    /// Key version for rotation tracking
    pub key_version: u32,
}

impl KeyDerivationSpec {
    /// Create identity-only key spec (no permissions)
    pub fn identity_only(identity: IdentityKeyContext) -> Self {
        Self {
            identity_context: identity,
            permission_context: None,
            key_version: 0,
        }
    }

    /// Create identity + permission key spec
    pub fn with_permission(identity: IdentityKeyContext, permission: PermissionKeyContext) -> Self {
        Self {
            identity_context: identity,
            permission_context: Some(permission),
            key_version: 0,
        }
    }

    /// Set key version for rotation
    pub fn with_version(mut self, version: u32) -> Self {
        self.key_version = version;
        self
    }

    /// Generate HKDF info string for this spec
    ///
    /// Format: "aura:v1:<identity_type>:<identity_data>[:<permission_type>:<permission_data>]:v<version>"
    pub fn to_info_string(&self) -> Vec<u8> {
        let mut info = String::from("aura:v1:");

        // Add identity context
        match &self.identity_context {
            IdentityKeyContext::DeviceEncryption { device_id } => {
                info.push_str("device:");
                info.push_str(&hex::encode(device_id));
            }
            IdentityKeyContext::RelationshipKeys { relationship_id } => {
                info.push_str("relationship:");
                info.push_str(&hex::encode(relationship_id));
            }
            IdentityKeyContext::AccountRoot { account_id } => {
                info.push_str("account:");
                info.push_str(&hex::encode(account_id));
            }
            IdentityKeyContext::GuardianKeys { guardian_id } => {
                info.push_str("guardian:");
                info.push_str(&hex::encode(guardian_id));
            }
        }

        // Add permission context if present
        if let Some(perm) = &self.permission_context {
            info.push(':');
            match perm {
                PermissionKeyContext::StorageAccess {
                    operation,
                    resource,
                } => {
                    info.push_str("storage:");
                    info.push_str(operation);
                    info.push(':');
                    info.push_str(resource);
                }
                PermissionKeyContext::CommunicationScope {
                    operation,
                    relationship,
                } => {
                    info.push_str("comm:");
                    info.push_str(operation);
                    info.push(':');
                    info.push_str(relationship);
                }
                PermissionKeyContext::RelayPermission {
                    operation,
                    trust_level,
                } => {
                    info.push_str("relay:");
                    info.push_str(operation);
                    info.push(':');
                    info.push_str(trust_level);
                }
            }
        }

        // Add version
        info.push_str(":v");
        info.push_str(&self.key_version.to_string());

        info.into_bytes()
    }
}

/// Derive key material using HKDF with separated contexts
///
/// Uses HKDF-SHA256 with the derivation spec as info string.
/// Returns raw key material that can be used for various key types.
pub fn derive_key_material(
    root_key: &[u8],
    spec: &KeyDerivationSpec,
    output_length: usize,
) -> Result<Vec<u8>> {
    if output_length == 0 || output_length > 255 * 32 {
        return Err(CryptoError::crypto_operation_failed(
            "Invalid output length for key derivation".to_string(),
        ));
    }

    let info = spec.to_info_string();

    // HKDF-Extract: derive pseudorandom key from root key
    let hkdf = Hkdf::<Sha256>::new(None, root_key);

    // HKDF-Expand: expand to desired output length
    let mut output = vec![0u8; output_length];
    hkdf.expand(&info, &mut output).map_err(|e| {
        CryptoError::key_derivation_failed(format!("HKDF expansion failed: {:?}", e))
    })?;

    Ok(output)
}

/// Derive symmetric encryption key (32 bytes)
pub fn derive_encryption_key(root_key: &[u8], spec: &KeyDerivationSpec) -> Result<[u8; 32]> {
    let material = derive_key_material(root_key, spec, 32)?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&material);
    Ok(key)
}

/// Derive Ed25519 signing key (32 bytes)
pub fn derive_signing_key(root_key: &[u8], spec: &KeyDerivationSpec) -> Result<[u8; 32]> {
    let material = derive_key_material(root_key, spec, 32)?;
    let mut key = [0u8; 32];
    key.copy_from_slice(&material);
    Ok(key)
}

/// Derive relationship keys (K_box, K_tag, K_psk) from relationship ID
///
/// Returns (K_box, K_tag, K_psk) as 32-byte keys for:
/// - K_box: Encryption key (XChaCha20-Poly1305)
/// - K_tag: Routing tag HMAC key
/// - K_psk: Handshake pre-shared key
pub fn derive_relationship_keys(
    root_key: &[u8],
    relationship_id: &[u8],
    version: u32,
) -> Result<([u8; 32], [u8; 32], [u8; 32])> {
    let spec = KeyDerivationSpec {
        identity_context: IdentityKeyContext::RelationshipKeys {
            relationship_id: relationship_id.to_vec(),
        },
        permission_context: None,
        key_version: version,
    };

    // Derive 96 bytes: 32 for K_box, 32 for K_tag, 32 for K_psk
    let material = derive_key_material(root_key, &spec, 96)?;

    let mut k_box = [0u8; 32];
    let mut k_tag = [0u8; 32];
    let mut k_psk = [0u8; 32];

    k_box.copy_from_slice(&material[0..32]);
    k_tag.copy_from_slice(&material[32..64]);
    k_psk.copy_from_slice(&material[64..96]);

    Ok((k_box, k_tag, k_psk))
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    fn test_root_key() -> Vec<u8> {
        // Deterministic test key
        vec![0x42; 32]
    }

    #[test]
    fn test_identity_only_derivation() {
        let root = test_root_key();
        let device_id = b"device_123".to_vec();

        let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: device_id.clone(),
        });

        let key1 = derive_encryption_key(&root, &spec).unwrap();
        let key2 = derive_encryption_key(&root, &spec).unwrap();

        // Same spec produces same key
        assert_eq!(key1, key2);

        // Different device produces different key
        let spec2 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"device_456".to_vec(),
        });
        let key3 = derive_encryption_key(&root, &spec2).unwrap();
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_permission_derivation() {
        let root = test_root_key();
        let device_id = b"device_123".to_vec();

        let spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: "file1".to_string(),
            },
        );

        let key = derive_encryption_key(&root, &spec).unwrap();

        // Same spec produces same key
        let key2 = derive_encryption_key(&root, &spec).unwrap();
        assert_eq!(key, key2);

        // Different permission produces different key
        let spec2 = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption { device_id },
            PermissionKeyContext::StorageAccess {
                operation: "write".to_string(),
                resource: "file1".to_string(),
            },
        );
        let key3 = derive_encryption_key(&root, &spec2).unwrap();
        assert_ne!(key, key3);
    }

    #[test]
    fn test_key_rotation_via_version() {
        let root = test_root_key();
        let device_id = b"device_123".to_vec();

        let spec_v0 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: device_id.clone(),
        })
        .with_version(0);

        let spec_v1 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: device_id.clone(),
        })
        .with_version(1);

        let key_v0 = derive_encryption_key(&root, &spec_v0).unwrap();
        let key_v1 = derive_encryption_key(&root, &spec_v1).unwrap();

        // Different versions produce different keys
        assert_ne!(key_v0, key_v1);
    }

    #[test]
    fn test_relationship_keys_derivation() {
        let root = test_root_key();
        let relationship_id = b"alice-bob".to_vec();

        let (k_box, k_tag, k_psk) = derive_relationship_keys(&root, &relationship_id, 0).unwrap();

        // All three keys should be different
        assert_ne!(k_box, k_tag);
        assert_ne!(k_box, k_psk);
        assert_ne!(k_tag, k_psk);

        // Same input produces same keys
        let (k_box2, k_tag2, k_psk2) =
            derive_relationship_keys(&root, &relationship_id, 0).unwrap();
        assert_eq!(k_box, k_box2);
        assert_eq!(k_tag, k_tag2);
        assert_eq!(k_psk, k_psk2);

        // Different relationship produces different keys
        let (k_box3, _, _) = derive_relationship_keys(&root, b"bob-charlie", 0).unwrap();
        assert_ne!(k_box, k_box3);
    }

    #[test]
    fn test_info_string_format() {
        let spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: b"test".to_vec(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: "file1".to_string(),
            },
        )
        .with_version(1);

        let info = String::from_utf8(spec.to_info_string()).unwrap();

        // Should contain all components
        assert!(info.starts_with("aura:v1:"));
        assert!(info.contains("device:"));
        assert!(info.contains("storage:read:file1"));
        assert!(info.ends_with(":v1"));
    }

    #[test]
    fn test_independent_rotation() {
        let root = test_root_key();
        let device_id = b"device".to_vec();

        // Identity key
        let identity_spec =
            KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            });

        // Permission key
        let permission_spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: "file1".to_string(),
            },
        );

        let identity_key_v0 = derive_encryption_key(&root, &identity_spec).unwrap();
        let permission_key_v0 = derive_encryption_key(&root, &permission_spec).unwrap();

        // Rotate permission key
        let permission_spec_v1 = permission_spec.clone().with_version(1);
        let permission_key_v1 = derive_encryption_key(&root, &permission_spec_v1).unwrap();

        // Identity key unchanged, permission key changed
        let identity_key_still = derive_encryption_key(&root, &identity_spec).unwrap();
        assert_eq!(identity_key_v0, identity_key_still);
        assert_ne!(permission_key_v0, permission_key_v1);
    }
}

//! Key Derivation for Aura
//!
//! This module implements secure key derivation using SHA256-based KDF following
//! the principles outlined in the key derivation security tests. It provides
//! context-aware key derivation for identity and permission keys with proper
//! separation and collision resistance.

use aura_core::hash;

/// Context for identity key derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentityKeyContext {
    /// Account root context for master keys
    AccountRoot {
        /// The account identifier
        account_id: Vec<u8>,
    },
    /// Device-specific identity keys for encryption
    DeviceEncryption {
        /// The device identifier
        device_id: Vec<u8>,
    },
    /// Relationship-specific identity keys
    RelationshipKeys {
        /// The relationship identifier
        relationship_id: Vec<u8>,
    },
    /// Guardian-specific identity keys
    GuardianKeys {
        /// The guardian identifier
        guardian_id: Vec<u8>,
    },
}

/// Context for permission key derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PermissionKeyContext {
    /// Storage access permission key
    StorageAccess {
        /// The operation type (read, write, delete, etc.)
        operation: String,
        /// The resource being accessed
        resource: String,
    },
    /// Communication permission key
    Communication {
        /// The capability identifier
        capability_id: Vec<u8>,
    },
}

/// Key derivation specification combining identity context, permission context, and versioning
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDerivationSpec {
    /// Identity context for the key
    pub identity_context: IdentityKeyContext,
    /// Optional permission context for additional authorization
    pub permission_context: Option<PermissionKeyContext>,
    /// Key version for rotation support
    pub key_version: u64,
}

impl KeyDerivationSpec {
    /// Create a specification for identity-only keys
    pub fn identity_only(identity_context: IdentityKeyContext) -> Self {
        Self {
            identity_context,
            permission_context: None,
            key_version: 0,
        }
    }

    /// Create a specification for keys with both identity and permission contexts
    pub fn with_permission(
        identity_context: IdentityKeyContext,
        permission_context: PermissionKeyContext,
    ) -> Self {
        Self {
            identity_context,
            permission_context: Some(permission_context),
            key_version: 0,
        }
    }

    /// Set the key version for rotation
    pub fn with_version(mut self, version: u64) -> Self {
        self.key_version = version;
        self
    }
}

/// Derive an encryption key using the specified context and version
///
/// This function provides secure key derivation with proper context separation
/// and collision resistance. It uses BLAKE3 for cryptographic hashing.
pub fn derive_encryption_key(root_key: &[u8], spec: &KeyDerivationSpec) -> crate::Result<[u8; 32]> {
    derive_key_material(root_key, spec, 32).map(|bytes| {
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes[0..32]);
        result
    })
}

/// Derive key material of arbitrary length
///
/// This is the core key derivation function that can produce keys of any length.
/// It uses HKDF-like expansion with BLAKE3 for consistency across different lengths.
pub fn derive_key_material(
    root_key: &[u8],
    spec: &KeyDerivationSpec,
    output_length: usize,
) -> crate::Result<Vec<u8>> {
    if output_length == 0 {
        return Err(crate::AuraError::invalid(
            "Output length must be greater than 0",
        ));
    }

    if output_length > 255 * 32 {
        return Err(crate::AuraError::invalid(
            "Output length too large for HKDF expansion",
        ));
    }

    // Build context string for domain separation
    let mut context_bytes = Vec::new();

    // Add identity context
    context_bytes.extend_from_slice(b"aura.key_derivation.v1:");
    context_bytes.extend_from_slice(b"identity:");

    match &spec.identity_context {
        IdentityKeyContext::AccountRoot { account_id } => {
            context_bytes.extend_from_slice(b"account_root:");
            context_bytes.extend_from_slice(account_id);
        }
        IdentityKeyContext::DeviceEncryption { device_id } => {
            context_bytes.extend_from_slice(b"device_encryption:");
            context_bytes.extend_from_slice(device_id);
        }
        IdentityKeyContext::RelationshipKeys { relationship_id } => {
            context_bytes.extend_from_slice(b"relationship:");
            context_bytes.extend_from_slice(relationship_id);
        }
        IdentityKeyContext::GuardianKeys { guardian_id } => {
            context_bytes.extend_from_slice(b"guardian:");
            context_bytes.extend_from_slice(guardian_id);
        }
    }

    // Add permission context if present
    if let Some(permission_context) = &spec.permission_context {
        context_bytes.extend_from_slice(b":permission:");

        match permission_context {
            PermissionKeyContext::StorageAccess {
                operation,
                resource,
            } => {
                context_bytes.extend_from_slice(b"storage:");
                context_bytes.extend_from_slice(operation.as_bytes());
                context_bytes.extend_from_slice(b":");
                context_bytes.extend_from_slice(resource.as_bytes());
            }
            PermissionKeyContext::Communication { capability_id } => {
                context_bytes.extend_from_slice(b"communication:");
                context_bytes.extend_from_slice(capability_id);
            }
        }
    }

    // Add version for key rotation
    context_bytes.extend_from_slice(b":version:");
    context_bytes.extend_from_slice(&spec.key_version.to_le_bytes());

    // Extract: Combine root key with context
    let mut extract_input = Vec::new();
    extract_input.extend_from_slice(root_key);
    extract_input.extend_from_slice(&context_bytes);

    let prk = hash::hash(&extract_input);

    // Expand: Generate output material using HKDF-like expansion
    let mut output = Vec::with_capacity(output_length);
    let num_blocks = output_length.div_ceil(32);

    for i in 0..num_blocks {
        let mut expand_input = Vec::new();
        expand_input.extend_from_slice(&prk);
        expand_input.extend_from_slice(&context_bytes);
        expand_input.push(i as u8 + 1); // HKDF counter (1-indexed)

        let block = hash::hash(&expand_input);
        output.extend_from_slice(&block);
    }

    // Truncate to requested length
    output.truncate(output_length);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation_deterministic() {
        let root_key = [1u8; 32];
        let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"test-device".to_vec(),
        });

        let key1 = derive_encryption_key(&root_key, &spec).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec).unwrap();

        assert_eq!(key1, key2, "Key derivation should be deterministic");
    }

    #[test]
    fn test_different_contexts_produce_different_keys() {
        let root_key = [2u8; 32];

        let spec1 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"device-1".to_vec(),
        });
        let spec2 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"device-2".to_vec(),
        });

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        assert_ne!(
            key1, key2,
            "Different contexts should produce different keys"
        );
    }

    #[test]
    fn test_key_versioning() {
        let root_key = [3u8; 32];
        let base_spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"test-device".to_vec(),
        });

        let key_v0 = derive_encryption_key(&root_key, &base_spec).unwrap();
        let key_v1 = derive_encryption_key(&root_key, &base_spec.clone().with_version(1)).unwrap();

        assert_ne!(
            key_v0, key_v1,
            "Different versions should produce different keys"
        );
    }

    #[test]
    fn test_permission_context_separation() {
        let root_key = [4u8; 32];
        let identity_context = IdentityKeyContext::DeviceEncryption {
            device_id: b"test-device".to_vec(),
        };

        let identity_spec = KeyDerivationSpec::identity_only(identity_context.clone());
        let permission_spec = KeyDerivationSpec::with_permission(
            identity_context,
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: "test-resource".to_string(),
            },
        );

        let identity_key = derive_encryption_key(&root_key, &identity_spec).unwrap();
        let permission_key = derive_encryption_key(&root_key, &permission_spec).unwrap();

        assert_ne!(
            identity_key, permission_key,
            "Identity and permission keys should be different"
        );
    }

    #[test]
    fn test_variable_output_lengths() {
        let root_key = [5u8; 32];
        let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"test-device".to_vec(),
        });

        let key_16 = derive_key_material(&root_key, &spec, 16).unwrap();
        let key_32 = derive_key_material(&root_key, &spec, 32).unwrap();
        let key_64 = derive_key_material(&root_key, &spec, 64).unwrap();

        assert_eq!(key_16.len(), 16);
        assert_eq!(key_32.len(), 32);
        assert_eq!(key_64.len(), 64);

        // HKDF property: shorter keys should be prefixes of longer ones
        assert_eq!(&key_16[..], &key_32[0..16]);
        assert_eq!(&key_32[..], &key_64[0..32]);
    }

    #[test]
    fn test_error_handling() {
        let root_key = [6u8; 32];
        let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: b"test-device".to_vec(),
        });

        // Test zero-length output
        let result = derive_key_material(&root_key, &spec, 0);
        assert!(result.is_err(), "Zero-length output should fail");

        // Test excessively large output
        let result = derive_key_material(&root_key, &spec, 256 * 32);
        assert!(result.is_err(), "Excessively large output should fail");
    }
}

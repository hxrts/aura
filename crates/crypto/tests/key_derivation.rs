#![allow(warnings, clippy::all)]
//! Unit Tests: Separated Key Derivation
//!
//! Tests key derivation with separated identity and permission contexts.
//! SSB derives relationship keys (K_box, K_tag, K_psk) from identity context.
//! Storage derives encryption keys from permission context.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 4.1
//! Reference: docs/040_storage_mvp.md - Separated Key Derivation
//! Reference: docs/051_rendezvous.md - Relationship Keys

use aura_crypto::dkd::{derive_keys, hash_to_scalar, participant_dkd_phase, point_to_seed};
use blake3;

// TODO: These types will be added to aura-crypto/src/types.rs
// For now, we define them here to specify the expected behavior

/// Identity-based key derivation contexts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityKeyContext {
    /// Relationship keys for SSB (K_box, K_tag, K_psk)
    RelationshipKeys { relationship_id: Vec<u8> },
    /// Guardian keys for recovery
    GuardianKeys { guardian_id: Vec<u8> },
}

/// Permission-based key derivation contexts
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionKeyContext {
    /// Storage encryption keys
    StorageAccess { operation: String, resource: String },
    /// Communication keys
    Communication { capability_id: Vec<u8> },
}

/// Derived key types for different purposes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedKeyType {
    /// Box key for envelope encryption (SSB)
    BoxKey,
    /// Tag key for routing tags (SSB)
    TagKey,
    /// PSK key for mutual auth (SSB)
    PskKey,
    /// Encryption key for storage
    EncryptionKey,
    /// Signing key
    SigningKey,
}

impl DerivedKeyType {
    fn context_string(&self) -> &[u8] {
        match self {
            DerivedKeyType::BoxKey => b"aura.relationship.box_key.v1",
            DerivedKeyType::TagKey => b"aura.relationship.tag_key.v1",
            DerivedKeyType::PskKey => b"aura.relationship.psk_key.v1",
            DerivedKeyType::EncryptionKey => b"aura.storage.encryption_key.v1",
            DerivedKeyType::SigningKey => b"aura.signing_key.v1",
        }
    }
}

/// Derive key from identity context
fn derive_identity_key(
    root_seed: &[u8],
    context: &IdentityKeyContext,
    key_type: DerivedKeyType,
) -> [u8; 32] {
    let mut context_bytes = Vec::new();
    context_bytes.extend_from_slice(b"identity:");

    match context {
        IdentityKeyContext::RelationshipKeys { relationship_id } => {
            context_bytes.extend_from_slice(b"relationship:");
            context_bytes.extend_from_slice(relationship_id);
        }
        IdentityKeyContext::GuardianKeys { guardian_id } => {
            context_bytes.extend_from_slice(b"guardian:");
            context_bytes.extend_from_slice(guardian_id);
        }
    }

    context_bytes.extend_from_slice(b":");
    context_bytes.extend_from_slice(key_type.context_string());

    let combined = blake3::hash(&context_bytes);
    let hash_bytes = combined.as_bytes();

    // Derive from root seed + context
    let mut final_input = Vec::new();
    final_input.extend_from_slice(root_seed);
    final_input.extend_from_slice(hash_bytes);

    *blake3::hash(&final_input).as_bytes()
}

/// Derive key from permission context
fn derive_permission_key(
    root_seed: &[u8],
    context: &PermissionKeyContext,
    key_type: DerivedKeyType,
) -> [u8; 32] {
    let mut context_bytes = Vec::new();
    context_bytes.extend_from_slice(b"permission:");

    match context {
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

    context_bytes.extend_from_slice(b":");
    context_bytes.extend_from_slice(key_type.context_string());

    let combined = blake3::hash(&context_bytes);
    let hash_bytes = combined.as_bytes();

    // Derive from root seed + context
    let mut final_input = Vec::new();
    final_input.extend_from_slice(root_seed);
    final_input.extend_from_slice(hash_bytes);

    *blake3::hash(&final_input).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_root_seed() -> [u8; 32] {
        *blake3::hash(b"test_root_seed").as_bytes()
    }

    #[test]
    fn test_identity_context_key_derivation() {
        let root_seed = create_test_root_seed();

        // Derive key with IdentityKeyContext::RelationshipKeys
        let relationship_id_1 = b"alice_bob_relationship".to_vec();
        let context_1 = IdentityKeyContext::RelationshipKeys {
            relationship_id: relationship_id_1.clone(),
        };

        let key_1a = derive_identity_key(&root_seed, &context_1, DerivedKeyType::BoxKey);
        let key_1b = derive_identity_key(&root_seed, &context_1, DerivedKeyType::BoxKey);

        // Assert: Key is deterministic for same inputs
        assert_eq!(
            key_1a, key_1b,
            "Same context should produce same key (determinism)"
        );

        // Derive key for different relationship
        let relationship_id_2 = b"alice_charlie_relationship".to_vec();
        let context_2 = IdentityKeyContext::RelationshipKeys {
            relationship_id: relationship_id_2,
        };

        let key_2 = derive_identity_key(&root_seed, &context_2, DerivedKeyType::BoxKey);

        // Assert: Different relationship IDs produce different keys
        assert_ne!(
            key_1a, key_2,
            "Different relationship IDs should produce different keys"
        );

        // Derive different key types for same relationship
        let tag_key = derive_identity_key(&root_seed, &context_1, DerivedKeyType::TagKey);
        let psk_key = derive_identity_key(&root_seed, &context_1, DerivedKeyType::PskKey);

        // Assert: Different key types produce different keys
        assert_ne!(key_1a, tag_key, "BoxKey and TagKey should be different");
        assert_ne!(key_1a, psk_key, "BoxKey and PskKey should be different");
        assert_ne!(tag_key, psk_key, "TagKey and PskKey should be different");

        println!("[OK] test_identity_context_key_derivation PASSED");
    }

    #[test]
    fn test_permission_context_key_derivation() {
        let root_seed = create_test_root_seed();

        // Derive key with PermissionKeyContext::StorageAccess
        let context_1 = PermissionKeyContext::StorageAccess {
            operation: "write".to_string(),
            resource: "/docs/private".to_string(),
        };

        let key_1a = derive_permission_key(&root_seed, &context_1, DerivedKeyType::EncryptionKey);
        let key_1b = derive_permission_key(&root_seed, &context_1, DerivedKeyType::EncryptionKey);

        // Assert: Key is deterministic
        assert_eq!(
            key_1a, key_1b,
            "Same permission context should produce same key"
        );

        // Derive key for different operation
        let context_2 = PermissionKeyContext::StorageAccess {
            operation: "read".to_string(),
            resource: "/docs/private".to_string(),
        };

        let key_2 = derive_permission_key(&root_seed, &context_2, DerivedKeyType::EncryptionKey);

        // Assert: Different operations produce different keys
        assert_ne!(
            key_1a, key_2,
            "Different operations should produce different keys"
        );

        // Derive key for different resource
        let context_3 = PermissionKeyContext::StorageAccess {
            operation: "write".to_string(),
            resource: "/docs/public".to_string(),
        };

        let key_3 = derive_permission_key(&root_seed, &context_3, DerivedKeyType::EncryptionKey);

        // Assert: Different resources produce different keys
        assert_ne!(
            key_1a, key_3,
            "Different resources should produce different keys"
        );

        println!("[OK] test_permission_context_key_derivation PASSED");
    }

    #[test]
    fn test_separated_key_rotation() {
        let root_seed_v1 = create_test_root_seed();

        // Derive identity key and permission key with version 1
        let identity_context = IdentityKeyContext::RelationshipKeys {
            relationship_id: b"alice_bob".to_vec(),
        };
        let permission_context = PermissionKeyContext::StorageAccess {
            operation: "write".to_string(),
            resource: "/shared".to_string(),
        };

        let identity_key_v1 =
            derive_identity_key(&root_seed_v1, &identity_context, DerivedKeyType::BoxKey);
        let permission_key_v1 = derive_permission_key(
            &root_seed_v1,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        // Rotate identity key (new root seed for identity)
        let root_seed_identity_v2 = *blake3::hash(b"rotated_identity_seed").as_bytes();
        let identity_key_v2 = derive_identity_key(
            &root_seed_identity_v2,
            &identity_context,
            DerivedKeyType::BoxKey,
        );

        // Permission key with original seed (unchanged)
        let permission_key_after_identity_rotation = derive_permission_key(
            &root_seed_v1,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        // Assert: Identity key changed
        assert_ne!(
            identity_key_v1, identity_key_v2,
            "Identity key should change after rotation"
        );

        // Assert: Permission key unchanged
        assert_eq!(
            permission_key_v1, permission_key_after_identity_rotation,
            "Permission key should remain unchanged when identity key rotates"
        );

        // Rotate permission key (new root seed for permission)
        let root_seed_permission_v2 = *blake3::hash(b"rotated_permission_seed").as_bytes();
        let permission_key_v2 = derive_permission_key(
            &root_seed_permission_v2,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        // Identity key with original seed (unchanged)
        let identity_key_after_permission_rotation =
            derive_identity_key(&root_seed_v1, &identity_context, DerivedKeyType::BoxKey);

        // Assert: Permission key changed
        assert_ne!(
            permission_key_v1, permission_key_v2,
            "Permission key should change after rotation"
        );

        // Assert: Identity key unchanged
        assert_eq!(
            identity_key_v1, identity_key_after_permission_rotation,
            "Identity key should remain unchanged when permission key rotates"
        );

        println!("[OK] test_separated_key_rotation PASSED");
    }

    #[test]
    fn test_ssb_relationship_keys_derive_independently() {
        // Test that SSB relationship keys (K_box, K_tag, K_psk) can be derived independently
        let root_seed = create_test_root_seed();
        let relationship_id = b"alice_bob_ssb".to_vec();
        let context = IdentityKeyContext::RelationshipKeys { relationship_id };

        // Derive all three SSB keys
        let k_box = derive_identity_key(&root_seed, &context, DerivedKeyType::BoxKey);
        let k_tag = derive_identity_key(&root_seed, &context, DerivedKeyType::TagKey);
        let k_psk = derive_identity_key(&root_seed, &context, DerivedKeyType::PskKey);

        // Assert: All keys are different
        assert_ne!(k_box, k_tag, "K_box and K_tag must be different");
        assert_ne!(k_box, k_psk, "K_box and K_psk must be different");
        assert_ne!(k_tag, k_psk, "K_tag and K_psk must be different");

        // Assert: All keys are deterministic
        let k_box_2 = derive_identity_key(&root_seed, &context, DerivedKeyType::BoxKey);
        let k_tag_2 = derive_identity_key(&root_seed, &context, DerivedKeyType::TagKey);
        let k_psk_2 = derive_identity_key(&root_seed, &context, DerivedKeyType::PskKey);

        assert_eq!(k_box, k_box_2, "K_box should be deterministic");
        assert_eq!(k_tag, k_tag_2, "K_tag should be deterministic");
        assert_eq!(k_psk, k_psk_2, "K_psk should be deterministic");

        println!("[OK] test_ssb_relationship_keys_derive_independently PASSED");
    }

    #[test]
    fn test_guardian_keys_separate_from_relationship_keys() {
        // Test that guardian keys are independent from relationship keys
        let root_seed = create_test_root_seed();
        let id_bytes = b"alice_bob".to_vec();

        // Same ID used for relationship and guardian (different contexts)
        let relationship_context = IdentityKeyContext::RelationshipKeys {
            relationship_id: id_bytes.clone(),
        };
        let guardian_context = IdentityKeyContext::GuardianKeys {
            guardian_id: id_bytes,
        };

        // Derive keys from both contexts
        let relationship_key =
            derive_identity_key(&root_seed, &relationship_context, DerivedKeyType::BoxKey);
        let guardian_key =
            derive_identity_key(&root_seed, &guardian_context, DerivedKeyType::BoxKey);

        // Assert: Different contexts produce different keys even with same ID
        assert_ne!(
            relationship_key, guardian_key,
            "Relationship and guardian contexts must produce different keys"
        );

        println!("[OK] test_guardian_keys_separate_from_relationship_keys PASSED");
    }
}

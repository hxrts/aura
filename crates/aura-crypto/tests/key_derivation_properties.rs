#![allow(clippy::expect_used)]

//! Property Tests: Key Derivation Properties
//!
//! Tests fundamental properties that must hold for key derivation.
//! Uses proptest to verify determinism, collision resistance, and rotation independence.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 4.2

use proptest::prelude::*;
use std::collections::HashSet;

// Use types from aura-crypto
use aura_core::hash;
use aura_crypto::{IdentityKeyContext, PermissionKeyContext};

/// Types of derived cryptographic keys
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DerivedKeyType {
    /// Box key for NaCl-style public-key encryption
    BoxKey,
    /// Tag key for message authentication
    TagKey,
    /// Pre-shared key
    PskKey,
    /// Encryption key
    EncryptionKey,
    /// Signing key
    SigningKey,
}

impl DerivedKeyType {
    #[allow(dead_code)] // Kept for potential future use
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

use aura_crypto::{derive_encryption_key, KeyDerivationSpec};

fn derive_identity_key(
    root_seed: &[u8],
    context: &IdentityKeyContext,
    key_type: DerivedKeyType,
) -> [u8; 32] {
    // Use key type as version to differentiate different key types
    let key_version = match key_type {
        DerivedKeyType::BoxKey => 1,
        DerivedKeyType::TagKey => 2,
        DerivedKeyType::PskKey => 3,
        DerivedKeyType::EncryptionKey => 4,
        DerivedKeyType::SigningKey => 5,
    };
    let spec = KeyDerivationSpec::identity_only(context.clone()).with_version(key_version);
    derive_encryption_key(root_seed, &spec).expect("Key derivation should succeed")
}

fn derive_permission_key(
    root_seed: &[u8],
    context: &PermissionKeyContext,
    key_type: DerivedKeyType,
) -> [u8; 32] {
    // Use a default identity context for permission keys
    let identity_context = IdentityKeyContext::DeviceEncryption {
        device_id: b"default-device".to_vec(),
    };
    // Use key type as version to differentiate different key types
    let key_version = match key_type {
        DerivedKeyType::BoxKey => 1,
        DerivedKeyType::TagKey => 2,
        DerivedKeyType::PskKey => 3,
        DerivedKeyType::EncryptionKey => 4,
        DerivedKeyType::SigningKey => 5,
    };
    let spec = KeyDerivationSpec::with_permission(identity_context, context.clone())
        .with_version(key_version);
    derive_encryption_key(root_seed, &spec).expect("Key derivation should succeed")
}

// Proptest generators

fn arb_seed() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 16..64)
}

fn arb_relationship_id() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 8..32)
}

fn arb_guardian_id() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 8..32)
}

fn arb_identity_context() -> impl Strategy<Value = IdentityKeyContext> {
    prop_oneof![
        arb_relationship_id().prop_map(|id| IdentityKeyContext::AccountRoot { account_id: id }),
        arb_relationship_id().prop_map(|id| IdentityKeyContext::DeviceEncryption { device_id: id }),
        arb_relationship_id().prop_map(|id| IdentityKeyContext::RelationshipKeys {
            relationship_id: id
        }),
        arb_guardian_id().prop_map(|id| IdentityKeyContext::GuardianKeys { guardian_id: id }),
    ]
}

fn arb_operation() -> impl Strategy<Value = String> {
    #[allow(clippy::unwrap_used)]
    let regex = prop::string::string_regex("(read|write|delete|admin)").unwrap();
    regex
}

fn arb_resource() -> impl Strategy<Value = String> {
    #[allow(clippy::unwrap_used)]
    let regex = prop::string::string_regex("(/[a-z]+)+").unwrap();
    regex
}

fn arb_capability_id() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 16..32)
}

fn arb_permission_context() -> impl Strategy<Value = PermissionKeyContext> {
    prop_oneof![
        (arb_operation(), arb_resource()).prop_map(|(op, res)| {
            PermissionKeyContext::StorageAccess {
                operation: op,
                resource: res,
            }
        }),
        arb_capability_id()
            .prop_map(|id| PermissionKeyContext::Communication { capability_id: id }),
    ]
}

fn arb_key_type() -> impl Strategy<Value = DerivedKeyType> {
    prop_oneof![
        Just(DerivedKeyType::BoxKey),
        Just(DerivedKeyType::TagKey),
        Just(DerivedKeyType::PskKey),
        Just(DerivedKeyType::EncryptionKey),
        Just(DerivedKeyType::SigningKey),
    ]
}

// Property Tests

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: Key derivation is deterministic
    ///
    /// Invariant: Same inputs always produce same output
    #[test]
    fn prop_key_derivation_deterministic(
        seed in arb_seed(),
        context in arb_identity_context(),
        key_type in arb_key_type(),
    ) {
        // Derive key twice with same inputs
        let key1 = derive_identity_key(&seed, &context, key_type.clone());
        let key2 = derive_identity_key(&seed, &context, key_type);

        prop_assert_eq!(key1, key2, "Key derivation must be deterministic");
    }

    /// Property: Different contexts produce different keys
    ///
    /// Invariant: Collision resistance for different contexts
    #[test]
    fn prop_key_derivation_collision_resistant(
        seed in arb_seed(),
        contexts in prop::collection::vec(arb_identity_context(), 2..20),
        key_type in arb_key_type(),
    ) {
        let mut keys = HashSet::new();
        let mut context_set = HashSet::new();

        for context in contexts {
            // Only test unique contexts
            if context_set.insert(context.clone()) {
                let key = derive_identity_key(&seed, &context, key_type.clone());

                // Check for collision
                prop_assert!(
                    keys.insert(key),
                    "Different contexts must produce different keys (collision detected)"
                );
            }
        }
    }

    /// Property: Different seeds produce different keys
    ///
    /// Invariant: Seed independence
    #[test]
    fn prop_different_seeds_produce_different_keys(
        seed1 in arb_seed(),
        seed2 in arb_seed(),
        context in arb_identity_context(),
        key_type in arb_key_type(),
    ) {
        prop_assume!(seed1 != seed2);

        let key1 = derive_identity_key(&seed1, &context, key_type.clone());
        let key2 = derive_identity_key(&seed2, &context, key_type);

        prop_assert_ne!(
            key1, key2,
            "Different seeds must produce different keys"
        );
    }

    /// Property: Rotation independence - rotating one context doesn't affect others
    ///
    /// Invariant: Independent rotation
    #[test]
    fn prop_key_rotation_preserves_other_contexts(
        seed_identity in arb_seed(),
        seed_permission in arb_seed(),
        identity_context in arb_identity_context(),
        permission_context in arb_permission_context(),
    ) {
        // Derive initial keys
        let identity_key_v1 = derive_identity_key(
            &seed_identity,
            &identity_context,
            DerivedKeyType::BoxKey,
        );
        let permission_key_v1 = derive_permission_key(
            &seed_permission,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        // Rotate identity seed
        let rotated_identity_seed = hash::hash(&seed_identity);
        let identity_key_v2 = derive_identity_key(
            &rotated_identity_seed,
            &identity_context,
            DerivedKeyType::BoxKey,
        );

        // Permission key should be unchanged
        let permission_key_after_identity_rotation = derive_permission_key(
            &seed_permission,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        prop_assert_ne!(
            identity_key_v1, identity_key_v2,
            "Identity key should change after rotation"
        );
        prop_assert_eq!(
            permission_key_v1, permission_key_after_identity_rotation,
            "Permission key should remain unchanged when identity rotates"
        );

        // Rotate permission seed
        let rotated_permission_seed = hash::hash(&seed_permission);
        let permission_key_v2 = derive_permission_key(
            &rotated_permission_seed,
            &permission_context,
            DerivedKeyType::EncryptionKey,
        );

        // Identity key (original seed) should be unchanged
        let identity_key_after_permission_rotation = derive_identity_key(
            &seed_identity,
            &identity_context,
            DerivedKeyType::BoxKey,
        );

        prop_assert_ne!(
            permission_key_v1, permission_key_v2,
            "Permission key should change after rotation"
        );
        prop_assert_eq!(
            identity_key_v1, identity_key_after_permission_rotation,
            "Identity key should remain unchanged when permission rotates"
        );
    }

    /// Property: All SSB key types (K_box, K_tag, K_psk) are distinct
    ///
    /// Invariant: Key type separation
    #[test]
    fn prop_ssb_key_types_always_distinct(
        seed in arb_seed(),
        relationship_id in arb_relationship_id(),
    ) {
        let context = IdentityKeyContext::RelationshipKeys { relationship_id };

        let k_box = derive_identity_key(&seed, &context, DerivedKeyType::BoxKey);
        let k_tag = derive_identity_key(&seed, &context, DerivedKeyType::TagKey);
        let k_psk = derive_identity_key(&seed, &context, DerivedKeyType::PskKey);

        prop_assert_ne!(k_box, k_tag, "K_box and K_tag must be different");
        prop_assert_ne!(k_box, k_psk, "K_box and K_psk must be different");
        prop_assert_ne!(k_tag, k_psk, "K_tag and K_psk must be different");
    }

    /// Property: Permission context key collisions are extremely unlikely
    ///
    /// Invariant: Collision resistance for permission contexts
    #[test]
    fn prop_permission_context_collision_resistant(
        seed in arb_seed(),
        contexts in prop::collection::vec(arb_permission_context(), 2..20),
    ) {
        let mut keys = HashSet::new();
        let mut context_set = HashSet::new();

        for context in contexts {
            // Only test unique contexts
            if context_set.insert(context.clone()) {
                let key = derive_permission_key(&seed, &context, DerivedKeyType::EncryptionKey);

                prop_assert!(
                    keys.insert(key),
                    "Different permission contexts must produce different keys"
                );
            }
        }
    }
}

#[cfg(test)]
mod manual_tests {
    #[test]
    fn test_property_tests_compile_and_run() {
        println!("[OK] Key derivation property tests compile successfully");
    }
}

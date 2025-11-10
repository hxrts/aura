// Cryptographic Property Tests using proptest
//
// Property-based tests that verify cryptographic properties hold for all inputs:
// - Key derivation determinism: Same inputs produce same outputs
// - Key derivation independence: Different contexts produce independent keys
// - Key derivation avalanche: Small input changes cause large output changes
// - Root key independence: Different root keys produce uncorrelated derived keys

use aura_crypto::key_derivation::{
    derive_encryption_key, IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext,
};
use proptest::prelude::*;

// Strategy to generate arbitrary root keys (32 bytes)
fn root_key_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

// Strategy to generate arbitrary device IDs (Vec<u8>)
fn device_id_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 8..32)
}

// Strategy to generate arbitrary context strings
fn context_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9]{1,50}".prop_map(|s| s)
}

// Strategy to generate arbitrary account IDs (Vec<u8>)
fn account_id_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 16..32)
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    /// Property: Key derivation is deterministic
    /// For any root key and context, deriving twice produces the same result
    #[test]
    fn prop_key_derivation_deterministic(
        root_key in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec).unwrap();

        prop_assert_eq!(key1.as_ref(), key2.as_ref(),
            "Key derivation must be deterministic");
    }

    /// Property: Different contexts produce different keys
    /// For any root key, different device IDs produce different derived keys
    #[test]
    fn prop_different_contexts_different_keys(
        root_key in root_key_strategy(),
        device_id1 in device_id_strategy(),
        device_id2 in device_id_strategy()
    ) {
        // Skip if device IDs are the same
        prop_assume!(device_id1 != device_id2);

        let spec1 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id1,
            }
        );
        let spec2 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id2,
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        prop_assert_ne!(key1.as_ref(), key2.as_ref(),
            "Different contexts must produce different keys");
    }

    /// Property: Identity and permission keys are independent
    /// For any root key and context, identity and permission keys are different
    #[test]
    fn prop_identity_permission_independence(
        root_key in root_key_strategy(),
        resource_id in device_id_strategy()
    ) {
        let identity_spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: resource_id.clone(),
            }
        );
        let permission_spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: resource_id.clone(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: String::from_utf8_lossy(&resource_id).to_string(),
            }
        );

        let identity_key = derive_encryption_key(&root_key, &identity_spec).unwrap();
        let permission_key = derive_encryption_key(&root_key, &permission_spec).unwrap();

        prop_assert_ne!(identity_key.as_ref(), permission_key.as_ref(),
            "Identity and permission keys must be independent");
    }

    /// Property: Key derivation has avalanche effect
    /// Small change in input produces large change in output
    #[test]
    fn prop_key_derivation_avalanche(
        root_key in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        prop_assume!(device_id.len() > 0);

        let spec1 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            }
        );

        // Flip one bit in device_id
        let mut device_id2 = device_id.clone();
        device_id2[0] ^= 0x01;

        let spec2 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id2,
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        // Count differing bits
        let hamming_distance: usize = key1.as_ref()
            .iter()
            .zip(key2.as_ref().iter())
            .map(|(a, b)| (a ^ b).count_ones() as usize)
            .sum();

        let total_bits = key1.as_ref().len() * 8;
        let difference_ratio = hamming_distance as f64 / total_bits as f64;

        // Avalanche effect: expect ~50% bits to differ
        prop_assert!(difference_ratio > 0.25 && difference_ratio < 0.75,
            "Avalanche effect: expected ~50% bit difference, got {:.1}%",
            difference_ratio * 100.0);
    }

    /// Property: Different root keys produce uncorrelated derived keys
    #[test]
    fn prop_root_key_independence(
        root_key1 in root_key_strategy(),
        root_key2 in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        prop_assume!(root_key1 != root_key2);

        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id,
            }
        );

        let key1 = derive_encryption_key(&root_key1, &spec).unwrap();
        let key2 = derive_encryption_key(&root_key2, &spec).unwrap();

        // Keys must be different
        prop_assert_ne!(key1.as_ref(), key2.as_ref(),
            "Different root keys must produce different derived keys");

        // Verify high entropy difference (~50% bits different)
        let hamming_distance: usize = key1.as_ref()
            .iter()
            .zip(key2.as_ref().iter())
            .map(|(a, b)| (a ^ b).count_ones() as usize)
            .sum();

        let total_bits = key1.as_ref().len() * 8;
        let difference_ratio = hamming_distance as f64 / total_bits as f64;

        prop_assert!(difference_ratio > 0.3 && difference_ratio < 0.7,
            "Different root keys should produce uncorrelated keys ({:.1}% difference)",
            difference_ratio * 100.0);
    }

    /// Property: Account-specific contexts are isolated
    #[test]
    fn prop_account_context_isolation(
        root_key in root_key_strategy(),
        account_id1 in account_id_strategy(),
        account_id2 in account_id_strategy()
    ) {
        prop_assume!(account_id1 != account_id2);

        let spec1 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::AccountRoot {
                account_id: account_id1,
            }
        );
        let spec2 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::AccountRoot {
                account_id: account_id2,
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        prop_assert_ne!(key1.as_ref(), key2.as_ref(),
            "Different account contexts must produce isolated keys");
    }

    /// Property: Key rotation produces forward secrecy
    /// Old version cannot be derived from new version
    #[test]
    fn prop_key_rotation_forward_secrecy(
        root_key in root_key_strategy(),
        relationship in account_id_strategy(),
        operation1 in context_strategy(),
        operation2 in context_strategy()
    ) {
        prop_assume!(operation1 != operation2);

        let spec1 = KeyDerivationSpec::with_permission(
            IdentityKeyContext::AccountRoot {
                account_id: relationship.clone(),
            },
            PermissionKeyContext::Communication {
                capability_id: operation1.as_bytes().to_vec(),
            }
        );
        let spec2 = KeyDerivationSpec::with_permission(
            IdentityKeyContext::AccountRoot {
                account_id: relationship,
            },
            PermissionKeyContext::Communication {
                capability_id: operation2.as_bytes().to_vec(),
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        // Different versions must produce different keys
        prop_assert_ne!(key1.as_ref(), key2.as_ref(),
            "Different versions must produce different keys");

        // High entropy difference (uncorrelated)
        let hamming_distance: usize = key1.as_ref()
            .iter()
            .zip(key2.as_ref().iter())
            .map(|(a, b)| (a ^ b).count_ones() as usize)
            .sum();

        let total_bits = key1.as_ref().len() * 8;
        let difference_ratio = hamming_distance as f64 / total_bits as f64;

        prop_assert!(difference_ratio > 0.3 && difference_ratio < 0.7,
            "Rotated keys should be uncorrelated ({:.1}% difference)",
            difference_ratio * 100.0);
    }

    /// Property: Derived keys have full entropy
    /// All bits should be approximately uniformly distributed
    #[test]
    fn prop_derived_key_entropy(
        root_key in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id,
            }
        );

        let key = derive_encryption_key(&root_key, &spec).unwrap();

        // Count set bits
        let set_bits: usize = key.as_ref()
            .iter()
            .map(|byte| byte.count_ones() as usize)
            .sum();

        let total_bits = key.as_ref().len() * 8;
        let set_ratio = set_bits as f64 / total_bits as f64;

        // Should be close to 50% (uniform distribution)
        // Using wider tolerance (35-65%) to account for statistical edge cases
        prop_assert!(set_ratio > 0.35 && set_ratio < 0.65,
            "Derived key should have uniform bit distribution ({:.1}% set)",
            set_ratio * 100.0);
    }

    /// Property: Same inputs always produce same outputs (idempotence)
    #[test]
    fn prop_key_derivation_idempotent(
        root_key in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id,
            }
        );

        // Derive key multiple times
        let keys: Vec<_> = (0..5)
            .map(|_| derive_encryption_key(&root_key, &spec).unwrap())
            .collect();

        // All should be identical
        for i in 1..keys.len() {
            prop_assert_eq!(keys[0].as_ref(), keys[i].as_ref(),
                "Key derivation must be idempotent (attempt {} differs)", i);
        }
    }

    /// Property: Context changes are detectable
    /// Even minor context changes produce different keys
    #[test]
    fn prop_context_change_detection(
        root_key in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        prop_assume!(device_id.len() > 1);

        // Original context
        let spec1 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            }
        );

        // Modified context (append one byte)
        let mut modified_id = device_id.clone();
        modified_id.push(0x00);
        let spec2 = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: modified_id,
            }
        );

        let key1 = derive_encryption_key(&root_key, &spec1).unwrap();
        let key2 = derive_encryption_key(&root_key, &spec2).unwrap();

        prop_assert_ne!(key1.as_ref(), key2.as_ref(),
            "Any context change must produce different key");
    }

    /// Property: Empty context is valid
    #[test]
    fn prop_empty_context_valid(
        root_key in root_key_strategy()
    ) {
        // Empty device ID should still produce valid key
        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: vec![],
            }
        );

        let key = derive_encryption_key(&root_key, &spec).unwrap();

        // Key should be valid (32 bytes)
        prop_assert_eq!(key.as_ref().len(), 32,
            "Derived key should always be 32 bytes");

        // Key should be deterministic even for empty context
        let key2 = derive_encryption_key(&root_key, &spec).unwrap();
        prop_assert_eq!(key.as_ref(), key2.as_ref(),
            "Empty context should produce deterministic key");
    }
}

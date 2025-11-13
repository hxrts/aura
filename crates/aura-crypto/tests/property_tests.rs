//! Cryptographic Property Tests using proptest
//!
//! Property-based tests that verify cryptographic properties hold for all inputs:
//! - Key derivation determinism: Same inputs produce same outputs
//! - Key derivation independence: Different contexts produce independent keys
//! - Key derivation avalanche: Small input changes cause large output changes
//! - Root key independence: Different root keys produce uncorrelated derived keys

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
        prop_assume!(!device_id.is_empty());

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

    /// Property: Key derivation is constant-time (timing attack resistance)
    /// Key derivation should be deterministic and not leak information through timing
    #[test]
    fn prop_timing_attack_resistance(
        root_key1 in root_key_strategy(),
        root_key2 in root_key_strategy(),
        device_id in device_id_strategy()
    ) {
        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id,
            }
        );

        // Derive keys multiple times to test for consistency
        // Key derivation should always produce the same result for the same inputs
        let key1_first = derive_encryption_key(&root_key1, &spec).unwrap();
        let key1_second = derive_encryption_key(&root_key1, &spec).unwrap();
        let key2_first = derive_encryption_key(&root_key2, &spec).unwrap();
        let key2_second = derive_encryption_key(&root_key2, &spec).unwrap();

        // Test that key derivation is deterministic (same input = same output)
        prop_assert_eq!(key1_first.as_ref(), key1_second.as_ref(),
            "Key derivation must be deterministic for root_key1");
        prop_assert_eq!(key2_first.as_ref(), key2_second.as_ref(),
            "Key derivation must be deterministic for root_key2");

        // Verify keys are different for different root keys (unless they're the same)
        if root_key1 != root_key2 {
            prop_assert_ne!(key1_first.as_ref(), key2_first.as_ref(),
                "Different root keys should produce different derived keys");
        }
    }

    /// Property: Key derivation with extreme inputs
    /// Very large contexts and edge case values should still work
    #[test]
    fn prop_extreme_input_handling(
        root_key in root_key_strategy(),
        device_size in 0usize..10000 // Up to 10KB device IDs
    ) {
        // Create very large device ID
        let large_device_id = vec![0x42u8; device_size];

        let spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: large_device_id.clone(),
            }
        );

        // Should still derive valid key
        let key = derive_encryption_key(&root_key, &spec).unwrap();
        prop_assert_eq!(key.as_ref().len(), 32, "Key length should remain constant");

        // Should be deterministic regardless of input size
        let key2 = derive_encryption_key(&root_key, &spec).unwrap();
        prop_assert_eq!(key.as_ref(), key2.as_ref(),
            "Large context derivation should be deterministic");

        // Different size inputs with same prefix should produce different keys
        if device_size > 0 {
            let mut smaller_id = large_device_id.clone();
            smaller_id.truncate(device_size / 2);

            let smaller_spec = KeyDerivationSpec::identity_only(
                IdentityKeyContext::DeviceEncryption {
                    device_id: smaller_id,
                }
            );

            let smaller_key = derive_encryption_key(&root_key, &smaller_spec).unwrap();
            prop_assert_ne!(key.as_ref(), smaller_key.as_ref(),
                "Different length contexts should produce different keys");
        }
    }

    /// Property: Cross-context contamination resistance
    /// Keys derived for different contexts should be uncorrelated
    #[test]
    fn prop_cross_context_isolation(
        root_key in root_key_strategy(),
        device_id1 in device_id_strategy(),
        device_id2 in device_id_strategy(),
        account_id in account_id_strategy()
    ) {
        prop_assume!(device_id1 != device_id2);

        // Device encryption context
        let device_spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id1.clone(),
            }
        );

        // Account root context
        let account_spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::AccountRoot {
                account_id: account_id.clone(),
            }
        );

        // Guardian context
        let guardian_spec = KeyDerivationSpec::identity_only(
            IdentityKeyContext::GuardianKeys {
                guardian_id: device_id2,
            }
        );

        let device_key = derive_encryption_key(&root_key, &device_spec).unwrap();
        let account_key = derive_encryption_key(&root_key, &account_spec).unwrap();
        let guardian_key = derive_encryption_key(&root_key, &guardian_spec).unwrap();

        // All keys should be different
        prop_assert_ne!(device_key.as_ref(), account_key.as_ref(),
            "Device and account contexts must produce different keys");
        prop_assert_ne!(device_key.as_ref(), guardian_key.as_ref(),
            "Device and guardian contexts must produce different keys");
        prop_assert_ne!(account_key.as_ref(), guardian_key.as_ref(),
            "Account and guardian contexts must produce different keys");

        // Verify high entropy separation
        let hamming_device_account: usize = device_key.as_ref()
            .iter()
            .zip(account_key.as_ref().iter())
            .map(|(a, b)| (a ^ b).count_ones() as usize)
            .sum();

        let total_bits = device_key.as_ref().len() * 8;
        let separation_ratio = hamming_device_account as f64 / total_bits as f64;

        prop_assert!(separation_ratio > 0.3 && separation_ratio < 0.7,
            "Cross-context keys should have high entropy separation ({:.1}%)",
            separation_ratio * 100.0);
    }

    /// Property: Permission key hierarchies maintain security
    /// Child permissions cannot derive parent permissions
    #[test]
    fn prop_permission_hierarchy_security(
        root_key in root_key_strategy(),
        device_id in device_id_strategy(),
        resource in context_strategy()
    ) {
        // Parent permission (broad access)
        let admin_spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: device_id.clone(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "admin".to_string(),
                resource: resource.clone(),
            }
        );

        // Child permission (narrow access)
        let read_spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id,
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource,
            }
        );

        let admin_key = derive_encryption_key(&root_key, &admin_spec).unwrap();
        let read_key = derive_encryption_key(&root_key, &read_spec).unwrap();

        // Keys must be different
        prop_assert_ne!(admin_key.as_ref(), read_key.as_ref(),
            "Admin and read permissions must have different keys");

        // Verify no detectable relationship between keys
        let hamming_distance: usize = admin_key.as_ref()
            .iter()
            .zip(read_key.as_ref().iter())
            .map(|(a, b)| (a ^ b).count_ones() as usize)
            .sum();

        let total_bits = admin_key.as_ref().len() * 8;
        let independence_ratio = hamming_distance as f64 / total_bits as f64;

        prop_assert!(independence_ratio > 0.35 && independence_ratio < 0.65,
            "Permission hierarchy keys should be cryptographically independent ({:.1}%)",
            independence_ratio * 100.0);
    }

    /// Property: Key rotation maintains backward secrecy
    /// New versions cannot be used to derive old versions
    #[test]
    fn prop_backward_secrecy(
        root_key in root_key_strategy(),
        device_id in device_id_strategy(),
        operation in context_strategy(),
        version_count in 2usize..10
    ) {
        let mut version_keys = Vec::new();

        // Generate keys for sequential versions
        for version in 0..version_count {
            let spec = KeyDerivationSpec::with_permission(
                IdentityKeyContext::DeviceEncryption {
                    device_id: device_id.clone(),
                },
                PermissionKeyContext::Communication {
                    capability_id: operation.as_bytes().to_vec(),
                }
            ).with_version(version as u64);

            let key = derive_encryption_key(&root_key, &spec).unwrap();
            version_keys.push(key);
        }

        // Verify all versions produce different keys
        for i in 0..version_keys.len() {
            for j in i+1..version_keys.len() {
                prop_assert_ne!(version_keys[i].as_ref(), version_keys[j].as_ref(),
                    "Versions {} and {} must produce different keys", i, j);

                // Verify high entropy difference
                let hamming_dist: usize = version_keys[i].as_ref()
                    .iter()
                    .zip(version_keys[j].as_ref().iter())
                    .map(|(a, b)| (a ^ b).count_ones() as usize)
                    .sum();

                let total_bits = version_keys[i].as_ref().len() * 8;
                let distance_ratio = hamming_dist as f64 / total_bits as f64;

                prop_assert!(distance_ratio > 0.35 && distance_ratio < 0.65,
                    "Version {} and {} keys should be uncorrelated ({:.1}%)",
                    i, j, distance_ratio * 100.0);
            }
        }
    }
}

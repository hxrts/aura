// Key Derivation Security Tests
//
// Tests security properties of the key derivation system:
// - Independence: Identity keys and permission keys are cryptographically independent
// - Forward secrecy: Key rotation invalidates old keys
// - Context binding: Keys are bound to specific contexts
// - Collision resistance: Different contexts produce different keys

use aura_crypto::key_derivation::{
    derive_encryption_key, derive_key_material, IdentityKeyContext, KeyDerivationSpec,
    PermissionKeyContext,
};
use ed25519_dalek::SigningKey;

/// Test that identity keys and permission keys for the same context are independent
#[test]
fn test_identity_permission_key_independence() {
    let root_key = SigningKey::from_bytes(&[1u8; 32]);
    let device_id = b"device-123".to_vec();

    // Derive identity-only key
    let identity_spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: device_id.clone(),
    });
    let identity_key = derive_encryption_key(root_key.as_bytes(), &identity_spec).unwrap();

    // Derive identity + permission key
    let permission_spec = KeyDerivationSpec::with_permission(
        IdentityKeyContext::DeviceEncryption {
            device_id: device_id.clone(),
        },
        PermissionKeyContext::StorageAccess {
            operation: "read".to_string(),
            resource: String::from_utf8(device_id.clone()).unwrap(),
        },
    );
    let permission_key = derive_encryption_key(root_key.as_bytes(), &permission_spec).unwrap();

    // Keys must be different
    assert_ne!(
        identity_key, permission_key,
        "Identity and permission keys must be independent"
    );

    // Verify high entropy difference
    let hamming_distance: usize = identity_key
        .iter()
        .zip(permission_key.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum();

    let total_bits = identity_key.len() * 8;
    let match_ratio = hamming_distance as f64 / total_bits as f64;

    assert!(
        match_ratio > 0.4 && match_ratio < 0.6,
        "Keys should appear independent (got {:.1}% different bits)",
        match_ratio * 100.0
    );
}

/// Test that rotating a key invalidates the old key
#[test]
fn test_key_rotation_forward_secrecy() {
    let root_key = SigningKey::from_bytes(&[2u8; 32]);
    let relationship_id = b"relationship-456".to_vec();

    // Derive key with version 0
    let spec_v0 = KeyDerivationSpec {
        identity_context: IdentityKeyContext::RelationshipKeys {
            relationship_id: relationship_id.clone(),
        },
        permission_context: None,
        key_version: 0,
    };
    let key_v0 = derive_encryption_key(root_key.as_bytes(), &spec_v0).unwrap();

    // Derive key with version 1 (rotated)
    let spec_v1 = spec_v0.clone().with_version(1);
    let key_v1 = derive_encryption_key(root_key.as_bytes(), &spec_v1).unwrap();

    // Keys must be different
    assert_ne!(key_v0, key_v1, "Rotated keys must be different");

    // Verify high entropy difference (uncorrelated)
    let hamming_distance: usize = key_v0
        .iter()
        .zip(key_v1.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum();

    let total_bits = key_v0.len() * 8;
    let distance_ratio = hamming_distance as f64 / total_bits as f64;

    assert!(
        distance_ratio > 0.4 && distance_ratio < 0.6,
        "Rotated keys should be uncorrelated (got {:.1}% different bits)",
        distance_ratio * 100.0
    );
}

/// Test that keys are bound to their specific contexts
#[test]
fn test_context_binding() {
    let root_key = SigningKey::from_bytes(&[3u8; 32]);

    // Derive keys for different contexts
    let spec1 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: b"device-1".to_vec(),
    });
    let key1 = derive_encryption_key(root_key.as_bytes(), &spec1).unwrap();

    let spec2 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: b"device-2".to_vec(),
    });
    let key2 = derive_encryption_key(root_key.as_bytes(), &spec2).unwrap();

    let spec3 = KeyDerivationSpec::identity_only(IdentityKeyContext::AccountRoot {
        account_id: b"account-1".to_vec(),
    });
    let key3 = derive_encryption_key(root_key.as_bytes(), &spec3).unwrap();

    // All keys must be different
    assert_ne!(
        key1, key2,
        "Different device IDs must produce different keys"
    );
    assert_ne!(
        key1, key3,
        "Different context types must produce different keys"
    );
    assert_ne!(key2, key3, "Different contexts must produce different keys");

    // Verify each key is tightly bound to its context (deterministic)
    let key1_verify = derive_encryption_key(root_key.as_bytes(), &spec1).unwrap();
    assert_eq!(key1, key1_verify, "Same context must produce same key");
}

/// Test collision resistance: different inputs produce different keys
#[test]
fn test_collision_resistance() {
    let root_key = SigningKey::from_bytes(&[4u8; 32]);
    let mut seen_keys = std::collections::HashSet::new();

    // Generate 100 keys for different contexts
    for i in 0..100 {
        let device_id = format!("device-{}", i).into_bytes();
        let spec =
            KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption { device_id });
        let key = derive_encryption_key(root_key.as_bytes(), &spec).unwrap();

        assert!(
            seen_keys.insert(key.to_vec()),
            "Collision detected at iteration {}: key {:?}",
            i,
            key
        );
    }

    assert_eq!(seen_keys.len(), 100, "All 100 keys must be unique");
}

/// Test that similar contexts produce uncorrelated keys (avalanche effect)
#[test]
fn test_avalanche_effect() {
    let root_key = SigningKey::from_bytes(&[5u8; 32]);

    // Two contexts that differ by a single character
    let device_id1 = b"device-000".to_vec();
    let device_id2 = b"device-001".to_vec(); // Last digit differs by 1

    let spec1 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: device_id1,
    });
    let key1 = derive_encryption_key(root_key.as_bytes(), &spec1).unwrap();

    let spec2 = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: device_id2,
    });
    let key2 = derive_encryption_key(root_key.as_bytes(), &spec2).unwrap();

    // Keys should differ in ~50% of bits (avalanche effect)
    let hamming_dist: usize = key1
        .iter()
        .zip(key2.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum();

    let total_bits = key1.len() * 8;
    let distance_ratio = hamming_dist as f64 / total_bits as f64;

    assert!(
        distance_ratio > 0.3 && distance_ratio < 0.7,
        "Small input change should cause large output change (got {:.1}% different bits)",
        distance_ratio * 100.0
    );
}

/// Test that different root keys produce uncorrelated derived keys
#[test]
fn test_root_key_independence() {
    let root_key1 = SigningKey::from_bytes(&[6u8; 32]);
    let root_key2 = SigningKey::from_bytes(&[7u8; 32]);

    let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: b"device-123".to_vec(),
    });

    let key1 = derive_encryption_key(root_key1.as_bytes(), &spec).unwrap();
    let key2 = derive_encryption_key(root_key2.as_bytes(), &spec).unwrap();

    // Keys must be different
    assert_ne!(
        key1, key2,
        "Different root keys must produce different derived keys"
    );

    // Verify high entropy difference
    let hamming_dist: usize = key1
        .iter()
        .zip(key2.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum();

    let total_bits = key1.len() * 8;
    let distance_ratio = hamming_dist as f64 / total_bits as f64;

    assert!(
        distance_ratio > 0.4 && distance_ratio < 0.6,
        "Keys from different roots should be uncorrelated (got {:.1}% different bits)",
        distance_ratio * 100.0
    );
}

/// Test that permission key versions provide proper versioning
#[test]
fn test_permission_key_versioning() {
    let root_key = SigningKey::from_bytes(&[8u8; 32]);
    let resource = "storage-789".to_string();

    let mut keys = Vec::new();

    // Derive keys for versions 0 through 9
    for version in 0..10 {
        let spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: b"device-1".to_vec(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: resource.clone(),
            },
        )
        .with_version(version);

        let key = derive_encryption_key(root_key.as_bytes(), &spec).unwrap();
        keys.push((version, key));
    }

    // Verify all versions produce different keys
    let key0 = &keys[0].1;
    for (version, key) in &keys[1..] {
        assert_ne!(
            key0, key,
            "Version {} should produce different key",
            version
        );
    }

    // Verify determinism
    for (version, original_key) in &keys {
        let spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::DeviceEncryption {
                device_id: b"device-1".to_vec(),
            },
            PermissionKeyContext::StorageAccess {
                operation: "read".to_string(),
                resource: resource.clone(),
            },
        )
        .with_version(*version);

        let verify_key = derive_encryption_key(root_key.as_bytes(), &spec).unwrap();
        assert_eq!(
            original_key, &verify_key,
            "Key derivation must be deterministic for version {}",
            version
        );
    }
}

/// Test that account-specific contexts produce isolated keys
#[test]
fn test_account_isolation() {
    let root_key = SigningKey::from_bytes(&[9u8; 32]);

    // Derive keys for different accounts
    let spec_account1 = KeyDerivationSpec::identity_only(IdentityKeyContext::AccountRoot {
        account_id: b"account-1".to_vec(),
    });
    let key_account1 = derive_encryption_key(root_key.as_bytes(), &spec_account1).unwrap();

    let spec_account2 = KeyDerivationSpec::identity_only(IdentityKeyContext::AccountRoot {
        account_id: b"account-2".to_vec(),
    });
    let key_account2 = derive_encryption_key(root_key.as_bytes(), &spec_account2).unwrap();

    // Use guardian keys as another context type
    let spec_guardian1 = KeyDerivationSpec::identity_only(IdentityKeyContext::GuardianKeys {
        guardian_id: b"guardian-1".to_vec(),
    });
    let key_guardian1 = derive_encryption_key(root_key.as_bytes(), &spec_guardian1).unwrap();

    // Different accounts must produce different keys
    assert_ne!(
        key_account1, key_account2,
        "Different accounts must have isolated keys"
    );

    // Different context types must produce different keys
    assert_ne!(
        key_account1, key_guardian1,
        "Different context types must produce different keys"
    );

    // Verify accounts cannot derive each other's keys
    let hamming_dist: usize = key_account1
        .iter()
        .zip(key_account2.iter())
        .map(|(a, b)| (a ^ b).count_ones() as usize)
        .sum();

    let total_bits = key_account1.len() * 8;
    let distance_ratio = hamming_dist as f64 / total_bits as f64;

    assert!(
        distance_ratio > 0.4 && distance_ratio < 0.6,
        "Account keys should be cryptographically independent (got {:.1}% different bits)",
        distance_ratio * 100.0
    );
}

/// Test that derived keys have proper entropy
#[test]
fn test_derived_key_entropy() {
    let root_key = SigningKey::from_bytes(&[10u8; 32]);

    // Derive multiple keys and check their entropy
    for i in 0..10 {
        let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
            device_id: format!("device-{}", i).into_bytes(),
        });
        let key = derive_encryption_key(root_key.as_bytes(), &spec).unwrap();

        // Count set bits
        let set_bits: usize = key.iter().map(|byte| byte.count_ones() as usize).sum();
        let total_bits = key.len() * 8;
        let set_ratio = set_bits as f64 / total_bits as f64;

        // Should be close to 50% (uniform distribution)
        assert!(
            set_ratio > 0.4 && set_ratio < 0.6,
            "Derived key {} should have uniform bit distribution ({:.1}% set)",
            i,
            set_ratio * 100.0
        );
    }
}

/// Test different output lengths work correctly
#[test]
fn test_variable_output_lengths() {
    let root_key = SigningKey::from_bytes(&[11u8; 32]);
    let spec = KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
        device_id: b"device-1".to_vec(),
    });

    // Test different output lengths
    let key_16 = derive_key_material(root_key.as_bytes(), &spec, 16).unwrap();
    let key_32 = derive_key_material(root_key.as_bytes(), &spec, 32).unwrap();
    let key_64 = derive_key_material(root_key.as_bytes(), &spec, 64).unwrap();

    assert_eq!(key_16.len(), 16, "Should derive 16 bytes");
    assert_eq!(key_32.len(), 32, "Should derive 32 bytes");
    assert_eq!(key_64.len(), 64, "Should derive 64 bytes");

    // Verify first 32 bytes of key_64 match key_32 (HKDF property)
    assert_eq!(
        &key_64[0..32],
        &key_32[..],
        "HKDF should be consistent across lengths"
    );

    // Verify first 16 bytes match
    assert_eq!(
        &key_16[..],
        &key_32[0..16],
        "HKDF should be consistent for shorter lengths"
    );
}

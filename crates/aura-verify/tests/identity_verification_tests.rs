//! Phase 6 Tests: Pure Identity Verification
//!
//! Tests for the clean authentication layer that only verifies WHO signed something.
//! These tests ensure authentication is stateless and contains no authorization logic.

use aura_verify::{verify_identity_proof, IdentityProof, KeyMaterial};
use aura_core::{DeviceId, GuardianId};
use aura_crypto::Ed25519SigningKey;
use signature::Signer;
use uuid::Uuid;

/// Test that device identity verification works correctly
#[tokio::test]
async fn test_pure_device_identity_verification() {
    // Create test device
    let device_id = DeviceId::from_bytes([1u8; 32]);
    let signing_key = Ed25519SigningKey::from_bytes(&[2u8; 32]);
    let verifying_key = signing_key.verifying_key();

    // Create key material
    let mut key_material = KeyMaterial::new();
    key_material.add_device_key(device_id, verifying_key);

    // Create test message
    let message = b"test message for device verification";

    // Create device signature
    let signature = signing_key.sign(message);

    // Create identity proof
    let proof = IdentityProof::Device {
        device_id,
        signature,
    };

    // Verify identity
    let result = verify_identity_proof(&proof, message, &key_material).unwrap();

    match result.proof {
        IdentityProof::Device {
            device_id: verified_device_id,
            ..
        } => {
            assert_eq!(verified_device_id, device_id);
        }
        _ => panic!("Expected device identity verification"),
    }
}

/// Test that guardian identity verification works correctly
#[tokio::test]
async fn test_pure_guardian_identity_verification() {
    // Create test guardian
    let guardian_id = GuardianId::from_uuid(Uuid::from_bytes([3u8; 16]));
    let signing_key = Ed25519SigningKey::from_bytes(&[4u8; 32]);
    let verifying_key = signing_key.verifying_key();

    // Create key material
    let mut key_material = KeyMaterial::new();
    key_material.add_guardian_key(guardian_id, verifying_key);

    // Create test message
    let message = b"test message for guardian verification";

    // Create guardian signature
    let signature = signing_key.sign(message);

    // Create identity proof
    let proof = IdentityProof::Guardian {
        guardian_id,
        signature,
    };

    // Verify identity
    let result = verify_identity_proof(&proof, message, &key_material).unwrap();

    match result.proof {
        IdentityProof::Guardian {
            guardian_id: verified_guardian_id,
            ..
        } => {
            assert_eq!(verified_guardian_id, guardian_id);
        }
        _ => panic!("Expected guardian identity verification"),
    }
}

/// Test that invalid signatures are rejected
#[tokio::test]
async fn test_invalid_signature_rejection() {
    // Create test device
    let device_id = DeviceId::from_bytes([5u8; 32]);
    let signing_key = Ed25519SigningKey::from_bytes(&[6u8; 32]);
    let verifying_key = signing_key.verifying_key();

    // Create key material
    let mut key_material = KeyMaterial::new();
    key_material.add_device_key(device_id, verifying_key);

    // Create test message
    let message = b"test message";

    // Create signature for different message
    let wrong_message = b"different message";
    let signature = signing_key.sign(wrong_message);

    // Create identity proof with wrong signature
    let proof = IdentityProof::Device {
        device_id,
        signature,
    };

    // Verify identity - should fail
    let result = verify_identity_proof(&proof, message, &key_material);
    assert!(
        result.is_err(),
        "Expected authentication failure for invalid signature"
    );
}

/// Test that unknown device keys are rejected
#[tokio::test]
async fn test_unknown_device_rejection() {
    // Create test device
    let device_id = DeviceId::from_bytes([7u8; 32]);
    let signing_key = Ed25519SigningKey::from_bytes(&[8u8; 32]);

    // Create empty key material (no known devices)
    let key_material = KeyMaterial::new();

    // Create test message
    let message = b"test message";

    // Create device signature
    let signature = signing_key.sign(message);

    // Create identity proof
    let proof = IdentityProof::Device {
        device_id,
        signature,
    };

    // Verify identity - should fail
    let result = verify_identity_proof(&proof, message, &key_material);
    assert!(
        result.is_err(),
        "Expected authentication failure for unknown device"
    );
}

/// Test key material management
#[test]
fn test_key_material_management() {
    let mut key_material = KeyMaterial::new();

    // Add device key
    let device_id = DeviceId::from_bytes([9u8; 32]);
    let device_key = Ed25519SigningKey::from_bytes(&[10u8; 32]).verifying_key();
    key_material.add_device_key(device_id, device_key);

    // Add guardian key
    let guardian_id = GuardianId::from_uuid(Uuid::from_bytes([11u8; 16]));
    let guardian_key = Ed25519SigningKey::from_bytes(&[12u8; 32]).verifying_key();
    key_material.add_guardian_key(guardian_id, guardian_key);

    // Verify we can retrieve keys
    assert!(key_material.get_device_public_key(&device_id).is_ok());
    assert!(key_material.get_guardian_public_key(&guardian_id).is_ok());

    // Verify unknown keys fail
    let unknown_device = DeviceId::from_bytes([99u8; 32]);
    let unknown_guardian = GuardianId::from_uuid(Uuid::from_bytes([98u8; 16]));
    assert!(key_material.get_device_public_key(&unknown_device).is_err());
    assert!(key_material
        .get_guardian_public_key(&unknown_guardian)
        .is_err());
}

// Removed test_device_capability_proof as IdentityProof::DeviceCapability doesn't exist

/// Verify authentication is stateless - no state maintained between calls
#[tokio::test]
async fn test_stateless_authentication() {
    // Create test setup
    let device_id = DeviceId::from_bytes([15u8; 32]);
    let signing_key = Ed25519SigningKey::from_bytes(&[16u8; 32]);
    let verifying_key = signing_key.verifying_key();

    let mut key_material = KeyMaterial::new();
    key_material.add_device_key(device_id, verifying_key);

    // First verification
    let message1 = b"first message";
    let signature1 = signing_key.sign(message1);
    let proof1 = IdentityProof::Device {
        device_id,
        signature: signature1,
    };

    let result1 = verify_identity_proof(&proof1, message1, &key_material).unwrap();

    // Second verification with different message
    let message2 = b"second message";
    let signature2 = signing_key.sign(message2);
    let proof2 = IdentityProof::Device {
        device_id,
        signature: signature2,
    };

    let result2 = verify_identity_proof(&proof2, message2, &key_material).unwrap();

    // Both should succeed independently - no state carried over
    match (result1.proof, result2.proof) {
        (
            IdentityProof::Device { device_id: id1, .. },
            IdentityProof::Device { device_id: id2, .. },
        ) => {
            assert_eq!(id1, device_id);
            assert_eq!(id2, device_id);
            assert_eq!(id1, id2);
        }
        _ => panic!("Expected device identity verification for both calls"),
    }
}

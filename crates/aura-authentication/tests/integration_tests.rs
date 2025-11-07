//! Integration tests for authentication and authorization
//!
//! These tests verify that authentication and authorization work together correctly.

#![allow(clippy::disallowed_methods)]

use aura_authentication::{
    AuthenticationContext, EventAuthorization, ThresholdConfig, ThresholdSig,
};
use aura_crypto::Effects;
use aura_types::{AccountId, DeviceId, GuardianId};
use ed25519_dalek::{Signer, SigningKey};

#[test]
fn test_device_authentication_flow() {
    let device_id = DeviceId::new();
    let account_id = AccountId::new();

    // Generate keys using effects system
    let effects = Effects::test();
    let key_bytes: [u8; 32] = effects.random_bytes();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();

    // Create authentication context
    let mut auth_context = AuthenticationContext::new();
    auth_context.add_device_key(device_id, verifying_key.clone());

    // Create event authorization
    let message = b"test event message";
    let signature = signing_key.sign(message);

    let authorization = EventAuthorization::DeviceCertificate {
        device_id,
        signature,
    };

    // Validate event authorization
    let result =
        aura_authentication::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

    assert!(result.is_ok());
}

#[test]
fn test_guardian_authentication_flow() {
    let guardian_id = GuardianId::new();
    let account_id = AccountId::new();

    // Generate guardian keys using effects system
    let effects = Effects::test();
    let key_bytes: [u8; 32] = effects.random_bytes();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();

    // Create authentication context
    let mut auth_context = AuthenticationContext::new();
    auth_context.add_guardian_key(guardian_id, verifying_key.clone());

    // Create event authorization
    let message = b"test guardian event";
    let signature = signing_key.sign(message);

    let authorization = EventAuthorization::GuardianSignature {
        guardian_id,
        signature,
    };

    // Validate event authorization
    let result =
        aura_authentication::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

    assert!(result.is_ok());
}

#[test]
fn test_threshold_authentication_flow() {
    let account_id = AccountId::new();

    // Generate group keys for threshold using effects system
    let effects = Effects::test();
    let key_bytes: [u8; 32] = effects.random_bytes();
    let group_signing_key = SigningKey::from_bytes(&key_bytes);
    let group_verifying_key = group_signing_key.verifying_key();

    // Create authentication context
    let mut auth_context = AuthenticationContext::new();
    auth_context.add_group_key(account_id, group_verifying_key.clone());
    auth_context.add_threshold_config(
        account_id,
        ThresholdConfig {
            threshold: 1, // Use 1 for Ed25519 compatibility
            participants: vec![DeviceId::new(), DeviceId::new(), DeviceId::new()],
        },
    );

    // Create threshold signature
    let message = b"test threshold event";
    let signature = group_signing_key.sign(message);

    let threshold_sig = ThresholdSig {
        signature,
        signers: vec![0], // One signer for Ed25519 compatibility
        signature_shares: vec![vec![1, 2, 3]], // One mock share
    };

    let authorization = EventAuthorization::ThresholdSignature(threshold_sig);

    // Validate event authorization
    let result =
        aura_authentication::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

    assert!(result.is_ok());
}

#[test]
fn test_invalid_authentication_fails() {
    let device_id = DeviceId::new();
    let account_id = AccountId::new();

    // Generate different keys (wrong key for verification) using effects system
    let effects = Effects::test();
    let key_bytes1: [u8; 32] = effects.random_bytes();
    let key_bytes2: [u8; 32] = effects.random_bytes();
    let signing_key = SigningKey::from_bytes(&key_bytes1);
    let wrong_signing_key = SigningKey::from_bytes(&key_bytes2);
    let wrong_verifying_key = wrong_signing_key.verifying_key();

    // Create authentication context with wrong key
    let mut auth_context = AuthenticationContext::new();
    auth_context.add_device_key(device_id, wrong_verifying_key);

    // Create event authorization
    let message = b"test event message";
    let signature = signing_key.sign(message);

    let authorization = EventAuthorization::DeviceCertificate {
        device_id,
        signature,
    };

    // Validate event authorization should fail
    let result =
        aura_authentication::event_validation::EventValidator::validate_event_authorization(
            &authorization,
            message,
            &auth_context,
            &account_id,
        );

    assert!(result.is_err());
}

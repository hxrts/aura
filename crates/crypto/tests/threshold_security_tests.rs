#![allow(warnings, clippy::all)]
// Threshold Signature Security Tests
//
// Tests FROST threshold signature unforgeability properties:
// - Cannot forge signatures without all required shares
// - Signatures bound to specific messages
// - Cannot substitute participants
// - Signatures verify correctly
// - All participants required (current FROST constraint)
// - Corrupted shares detected

use aura_crypto::{frost_verifying_key_to_dalek, verify_signature, Effects, FrostSigner};
use std::collections::BTreeMap;

// Helper to set up FROST keys for testing
fn setup_frost_keys(
    threshold: u16,
    num_participants: u16,
) -> (
    BTreeMap<frost_ed25519::Identifier, frost_ed25519::keys::KeyPackage>,
    frost_ed25519::keys::PublicKeyPackage,
) {
    let effects = Effects::for_test("setup_frost_keys");
    let mut rng = effects.rng();

    let (shares, pubkey_package) = frost_ed25519::keys::generate_with_dealer(
        threshold,
        num_participants,
        frost_ed25519::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("FROST key generation failed");

    let key_packages = shares
        .into_iter()
        .map(|(id, secret_share)| {
            let key_package = frost_ed25519::keys::KeyPackage::try_from(secret_share)
                .expect("Invalid key package");
            (id, key_package)
        })
        .collect();

    (key_packages, pubkey_package)
}

/// Test that insufficient participants cannot create a signature (unforgeability)
#[test]
fn test_frost_unforgeability_insufficient_shares() {
    let effects = Effects::for_test("test_frost_unforgeability_insufficient_shares");
    let mut rng = effects.rng();

    // Setup: 3-of-3 threshold (FROST current version requires all to sign)
    let (key_packages, pubkey_package) = setup_frost_keys(3, 3);
    let message = b"test message";

    // Try to sign with only 2 participants (insufficient)
    let participants: Vec<_> = key_packages.iter().take(2).collect();
    let participating_packages: BTreeMap<_, _> = participants
        .into_iter()
        .map(|(id, pkg)| (*id, pkg.clone()))
        .collect();

    // Attempt threshold signing with insufficient participants
    let result = FrostSigner::threshold_sign(
        message,
        &participating_packages,
        &pubkey_package,
        3,
        &mut rng,
    );

    // Should fail due to insufficient participants
    assert!(
        result.is_err(),
        "Should not be able to create signature with insufficient shares"
    );
}

/// Test that signatures are bound to specific messages
#[test]
fn test_frost_message_binding() {
    let effects = Effects::for_test("test_frost_message_binding");
    let mut rng = effects.rng();

    // Setup: 2-of-2 threshold
    let (key_packages, pubkey_package) = setup_frost_keys(2, 2);
    let verifying_key = pubkey_package.verifying_key();

    let message1 = b"original message";
    let message2 = b"different message";

    // Sign message1
    let signature1 =
        FrostSigner::threshold_sign(message1, &key_packages, &pubkey_package, 2, &mut rng)
            .expect("Signing should succeed");

    // Sign message2
    let signature2 =
        FrostSigner::threshold_sign(message2, &key_packages, &pubkey_package, 2, &mut rng)
            .expect("Signing should succeed");

    // Verify signatures are different
    assert_ne!(
        signature1.to_bytes(),
        signature2.to_bytes(),
        "Different messages must produce different signatures"
    );

    // Verify each signature only works with its message
    let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();

    assert!(
        verify_signature(message1, &signature1, &dalek_key).is_ok(),
        "Signature1 should verify for message1"
    );
    assert!(
        verify_signature(message2, &signature2, &dalek_key).is_ok(),
        "Signature2 should verify for message2"
    );

    // Cross-verification should fail
    assert!(
        verify_signature(message1, &signature2, &dalek_key).is_err(),
        "Signature2 should not verify for message1"
    );
    assert!(
        verify_signature(message2, &signature1, &dalek_key).is_err(),
        "Signature1 should not verify for message2"
    );
}

/// Test that different participant sets produce valid signatures
#[test]
fn test_frost_participant_independence() {
    let effects = Effects::for_test("test_frost_participant_independence");
    let mut rng = effects.rng();

    // Setup: 2-of-2 threshold
    let (key_packages, pubkey_package) = setup_frost_keys(2, 2);
    let verifying_key = pubkey_package.verifying_key();
    let message = b"test message";

    // All participants sign
    let sig = FrostSigner::threshold_sign(message, &key_packages, &pubkey_package, 2, &mut rng)
        .expect("Signing should succeed");

    // Verify signature
    let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();
    assert!(
        verify_signature(message, &sig, &dalek_key).is_ok(),
        "Signature should verify with all participants"
    );

    // Test that any proper subset cannot sign (ensures threshold property)
    let first_only: BTreeMap<_, _> = key_packages
        .iter()
        .take(1)
        .map(|(id, pkg)| (*id, pkg.clone()))
        .collect();
    let result = FrostSigner::threshold_sign(message, &first_only, &pubkey_package, 2, &mut rng);
    assert!(
        result.is_err(),
        "Should not be able to sign with only one participant"
    );
}

/// Test that FROST signatures verify correctly
#[test]
fn test_frost_signature_verification() {
    let effects = Effects::for_test("test_frost_signature_verification");
    let mut rng = effects.rng();

    // Setup: 3-of-3 threshold
    let (key_packages, pubkey_package) = setup_frost_keys(3, 3);
    let message = b"test message";

    // Create signature
    let sig = FrostSigner::threshold_sign(message, &key_packages, &pubkey_package, 3, &mut rng)
        .expect("Signing should succeed");

    // Verify signature multiple times (should be deterministic verification)
    let verifying_key = pubkey_package.verifying_key();
    let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();

    for _ in 0..5 {
        assert!(
            verify_signature(message, &sig, &dalek_key).is_ok(),
            "Signature should verify consistently"
        );
    }

    // Wrong message should fail
    let wrong_message = b"wrong message";
    assert!(
        verify_signature(wrong_message, &sig, &dalek_key).is_err(),
        "Signature should not verify for wrong message"
    );
}

/// Test with different threshold values
#[test]
fn test_frost_different_thresholds() {
    let effects = Effects::for_test("test_frost_different_thresholds");
    let mut rng = effects.rng();
    let message = b"test message";

    // Test 2-of-2
    let (key_packages_2, pubkey_package_2) = setup_frost_keys(2, 2);
    let sig_2 =
        FrostSigner::threshold_sign(message, &key_packages_2, &pubkey_package_2, 2, &mut rng)
            .expect("2-of-2 signing should succeed");

    let vk_2 = frost_verifying_key_to_dalek(pubkey_package_2.verifying_key()).unwrap();
    assert!(
        verify_signature(message, &sig_2, &vk_2).is_ok(),
        "2-of-2 signature should verify"
    );

    // Test 3-of-3
    let (key_packages_3, pubkey_package_3) = setup_frost_keys(3, 3);
    let sig_3 =
        FrostSigner::threshold_sign(message, &key_packages_3, &pubkey_package_3, 3, &mut rng)
            .expect("3-of-3 signing should succeed");

    let vk_3 = frost_verifying_key_to_dalek(pubkey_package_3.verifying_key()).unwrap();
    assert!(
        verify_signature(message, &sig_3, &vk_3).is_ok(),
        "3-of-3 signature should verify"
    );

    // Test 5-of-5
    let (key_packages_5, pubkey_package_5) = setup_frost_keys(5, 5);
    let sig_5 =
        FrostSigner::threshold_sign(message, &key_packages_5, &pubkey_package_5, 5, &mut rng)
            .expect("5-of-5 signing should succeed");

    let vk_5 = frost_verifying_key_to_dalek(pubkey_package_5.verifying_key()).unwrap();
    assert!(
        verify_signature(message, &sig_5, &vk_5).is_ok(),
        "5-of-5 signature should verify"
    );

    // Verify different key groups produce different signatures
    assert_ne!(
        sig_2.to_bytes(),
        sig_3.to_bytes(),
        "Different key groups produce different signatures"
    );
    assert_ne!(
        sig_2.to_bytes(),
        sig_5.to_bytes(),
        "Different key groups produce different signatures"
    );
}

/// Test that corrupted signature shares are detected during aggregation
#[test]
fn test_frost_corrupted_share_detection() {
    let effects = Effects::for_test("test_frost_corrupted_share_detection");
    let mut rng = effects.rng();

    // Setup: 2-of-2 threshold
    let (key_packages, pubkey_package) = setup_frost_keys(2, 2);
    let message = b"test message";

    // Round 1: Generate nonces and commitments
    let mut nonces_map = BTreeMap::new();
    let mut commitments_map = BTreeMap::new();

    for (id, key_package) in key_packages.iter() {
        let (nonces, commitments) =
            FrostSigner::generate_nonces(key_package.signing_share(), &mut rng);
        nonces_map.insert(*id, nonces);
        commitments_map.insert(*id, commitments);
    }

    // Round 2: Create signature shares
    let signing_package = frost_ed25519::SigningPackage::new(commitments_map.clone(), message);
    let mut signature_shares = BTreeMap::new();

    for (id, key_package) in key_packages.iter() {
        let nonces = &nonces_map[id];
        let signature_share = frost_ed25519::round2::sign(&signing_package, nonces, key_package)
            .expect("Signing should succeed");
        signature_shares.insert(*id, signature_share);
    }

    // Corrupt one signature share by replacing it with a different participant's share
    // This simulates a Byzantine participant trying to substitute shares
    let first_id = *signature_shares.keys().next().unwrap();
    let second_id = *signature_shares.keys().nth(1).unwrap();
    let wrong_share = signature_shares[&second_id]; // Use second participant's share for first
    signature_shares.insert(first_id, wrong_share);

    // Try to aggregate with corrupted share
    let result = FrostSigner::aggregate(
        message,
        &commitments_map,
        &signature_shares,
        &pubkey_package,
    );

    // Should fail due to corrupted share
    assert!(
        result.is_err(),
        "Aggregation should fail with corrupted signature share"
    );
}

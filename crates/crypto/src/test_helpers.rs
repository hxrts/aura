// Test helper utilities for security tests
//
// This module provides simplified test helpers that wrap the actual FROST API
// to make security tests easier to write and maintain.

#![cfg(test)]
#![allow(warnings, clippy::all)]

use crate::Effects;
use ed25519_dalek::SigningKey;
use frost_ed25519 as frost;
use std::collections::BTreeMap;

/// Generate test FROST key packages
pub fn setup_frost_keys(
    threshold: u16,
    num_participants: u16,
) -> (
    BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
    frost::keys::PublicKeyPackage,
) {
    let effects = Effects::for_test("setup_frost_keys");
    let mut rng = effects.rng();

    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        threshold,
        num_participants,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("FROST key generation failed");

    let key_packages = shares
        .into_iter()
        .map(|(id, secret_share)| {
            let key_package =
                frost::keys::KeyPackage::try_from(secret_share).expect("Invalid key package");
            (id, key_package)
        })
        .collect();

    (key_packages, pubkey_package)
}

/// Generate signing nonces for testing
pub fn generate_signing_nonces(
    signing_key: &SigningKey,
    participant_id: u16,
) -> (
    frost::round1::SigningNonces,
    frost::round1::SigningCommitments,
) {
    let effects = Effects::for_test(&format!("gen_nonces_{}", participant_id));
    let mut rng = effects.rng();

    // Convert SigningKey to FROST signing share for testing
    // In real code, this would come from proper key generation
    let key_bytes = signing_key.to_bytes();
    let signing_share =
        frost::keys::SigningShare::deserialize(key_bytes).expect("Invalid signing share");

    frost::round1::commit(&signing_share, &mut rng)
}

/// Sign with a FROST share for testing
pub fn sign_with_share(
    signing_key: &SigningKey,
    nonces: &frost::round1::SigningNonces,
    message: &[u8],
    participant_id: u16,
) -> frost::round2::SignatureShare {
    // For testing purposes, create a minimal signing package
    // In real code, this would use proper KeyPackage

    let key_bytes = signing_key.to_bytes();
    let signing_share =
        frost::keys::SigningShare::deserialize(key_bytes).expect("Invalid signing share");

    // Create a minimal KeyPackage for testing
    // Note: This is simplified for testing - real code uses proper DKG
    let identifier = frost::Identifier::try_from(participant_id).expect("Invalid participant ID");

    // Create a dummy commitments map with just this participant
    let mut commitments_map = BTreeMap::new();
    let (_, commitments) = generate_signing_nonces(signing_key, participant_id);
    commitments_map.insert(identifier, commitments);

    let signing_package = frost::SigningPackage::new(commitments_map, message);

    // For testing, we need a KeyPackage - create a minimal one
    // This won't actually work for real signing, but demonstrates the API
    // Real tests should use setup_frost_keys() instead

    // Return a placeholder - real tests should use setup_frost_keys()
    frost::round2::SignatureShare::deserialize([0u8; 32]).expect("Test signature share")
}

/// Aggregate commitments for testing (no-op, for API compatibility)
pub fn aggregate_commitments(
    _commitments: &BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
) -> Vec<u8> {
    // This is a simplified version for testing
    vec![0u8; 32]
}

/// Helper to get combinations of participants
pub fn generate_combinations(n: usize, k: usize) -> Vec<Vec<usize>> {
    if k == 0 {
        return vec![vec![]];
    }
    if n < k {
        return vec![];
    }
    if n == k {
        return vec![(0..n).collect()];
    }

    let mut result = Vec::new();

    // Include first element
    let mut with_first = generate_combinations(n - 1, k - 1);
    for comb in &mut with_first {
        comb.insert(0, 0);
        for item in comb.iter_mut().skip(1) {
            *item += 1;
        }
    }
    result.extend(with_first);

    // Exclude first element
    let mut without_first = generate_combinations(n - 1, k);
    for comb in &mut without_first {
        for item in comb.iter_mut() {
            *item += 1;
        }
    }
    result.extend(without_first);

    result
}

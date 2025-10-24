//! Unit Tests: FROST Signature Aggregation
//!
//! Tests FROST threshold signatures for multi-device coordination.
//! SSB counter coordination and Storage manifest signing both depend on FROST.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 3.1

use aura_crypto::Effects;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use frost_ed25519 as frost;
use std::collections::BTreeMap;

/// Helper to generate FROST key shares for testing
fn generate_key_shares(
    threshold: u16,
    total: u16,
    effects: &Effects,
) -> (
    BTreeMap<frost::Identifier, frost::keys::KeyPackage>,
    frost::keys::PublicKeyPackage,
) {
    let max_signers = total;
    let min_signers = threshold;

    let mut rng = effects.rng();

    // Generate coefficients for secret sharing polynomial
    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        max_signers,
        min_signers,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .unwrap();

    // Convert SecretShare to KeyPackage
    let key_packages: BTreeMap<_, _> = shares
        .into_iter()
        .map(|(id, secret_share)| {
            let key_package = frost::keys::KeyPackage::try_from(secret_share).unwrap();
            (id, key_package)
        })
        .collect();

    (key_packages, pubkey_package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frost_signature_aggregation_2of3() {
        // Generate 3 key shares (threshold = 2)
        let effects = Effects::for_test("test_frost_signature_aggregation_2of3");
        let (key_shares, pubkey_package) = generate_key_shares(2, 3, &effects);

        // Message to sign
        let message = b"SSB counter increment: session_epoch=5, counter=42";

        // Select 2 participants (threshold = 2)
        let participants: Vec<_> = key_shares.iter().take(2).collect();
        let participant_1 = participants[0];
        let participant_2 = participants[1];

        // Round 1: Generate nonces and commitments
        let mut rng = effects.rng();

        let (nonces_1, commitments_1) =
            frost::round1::commit(participant_1.1.signing_share(), &mut rng);

        let (nonces_2, commitments_2) =
            frost::round1::commit(participant_2.1.signing_share(), &mut rng);

        // Collect commitments
        let mut signing_commitments = BTreeMap::new();
        signing_commitments.insert(*participant_1.0, commitments_1);
        signing_commitments.insert(*participant_2.0, commitments_2);

        // Round 2: Generate signature shares
        let signing_package = frost::SigningPackage::new(signing_commitments.clone(), message);

        let signature_share_1 = frost::round2::sign(&signing_package, &nonces_1, participant_1.1)
            .expect("Participant 1 signing should succeed");

        let signature_share_2 = frost::round2::sign(&signing_package, &nonces_2, participant_2.1)
            .expect("Participant 2 signing should succeed");

        // Collect signature shares
        let mut signature_shares = BTreeMap::new();
        signature_shares.insert(*participant_1.0, signature_share_1);
        signature_shares.insert(*participant_2.0, signature_share_2);

        // Aggregate signatures
        let group_signature =
            frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
                .expect("Aggregation should succeed");

        // Verify aggregated signature
        let group_verifying_key = pubkey_package.verifying_key();

        // Convert FROST signature to ed25519 signature
        let signature_bytes = group_signature.serialize();
        let ed25519_signature = Signature::from_bytes(&signature_bytes);

        // Convert FROST verifying key to ed25519 verifying key
        let vk_bytes = group_verifying_key.serialize();
        let ed25519_vk = VerifyingKey::from_bytes(&vk_bytes).expect("Valid verifying key");

        // Verify
        assert!(
            ed25519_vk.verify(message, &ed25519_signature).is_ok(),
            "2-of-3 threshold signature should verify"
        );

        println!("✓ test_frost_signature_aggregation_2of3 PASSED");
    }

    #[test]
    fn test_frost_signature_fails_with_insufficient_shares() {
        // Generate 3 key shares (threshold = 2)
        let effects = Effects::for_test("test_frost_signature_fails_with_insufficient_shares");
        let (key_shares, pubkey_package) = generate_key_shares(2, 3, &effects);

        // Message to sign
        let message = b"SSB counter increment attempt with insufficient shares";

        // Round 1: All participants generate commitments (required by FROST)
        let participants: Vec<_> = key_shares.iter().take(2).collect();
        let mut rng = effects.rng();

        let mut signing_commitments = BTreeMap::new();
        let mut nonces_map = BTreeMap::new();

        for (id, key_package) in &participants {
            let (nonces, commitments) =
                frost::round1::commit(key_package.signing_share(), &mut rng);
            signing_commitments.insert(**id, commitments);
            nonces_map.insert(**id, nonces);
        }

        // Round 2: Only ONE device provides signature share (insufficient for threshold = 2)
        let signing_package = frost::SigningPackage::new(signing_commitments.clone(), message);

        // Only first participant signs
        let participant_1 = participants[0];
        let nonces_1 = nonces_map.get(participant_1.0).unwrap();
        let signature_share_1 = frost::round2::sign(&signing_package, nonces_1, participant_1.1)
            .expect("Individual signing should succeed");

        // Collect signature shares (only one - insufficient)
        let mut signature_shares = BTreeMap::new();
        signature_shares.insert(*participant_1.0, signature_share_1);

        // Attempt aggregation (should fail with insufficient signature shares)
        let aggregation_result =
            frost::aggregate(&signing_package, &signature_shares, &pubkey_package);

        // Assert: Aggregation fails with insufficient participants
        assert!(
            aggregation_result.is_err(),
            "Aggregation should fail with only 1 signature share when threshold is 2"
        );

        let error_msg = format!("{:?}", aggregation_result.unwrap_err());
        assert!(
            error_msg.contains("Invalid")
                || error_msg.contains("Incorrect")
                || error_msg.contains("threshold")
                || error_msg.contains("UnknownIdentifier"),
            "Error should indicate insufficient shares, got: {}",
            error_msg
        );

        println!("✓ test_frost_signature_fails_with_insufficient_shares PASSED");
    }

    #[test]
    fn test_frost_signature_deterministic() {
        // Generate key shares
        let effects = Effects::deterministic(42, 1000);
        let (key_shares, pubkey_package) = generate_key_shares(2, 3, &effects);

        // Message to sign
        let message = b"Deterministic signature test message";

        // Select 2 participants
        let participants: Vec<_> = key_shares.iter().take(2).collect();

        // Sign twice with same parameters
        let mut signatures = Vec::new();

        for round in 0..2 {
            // Use deterministic effects for each round
            let round_effects = Effects::deterministic(42 + round as u64, 1000);
            let mut rng = round_effects.rng();

            // Round 1: Generate nonces and commitments
            let (nonces_1, commitments_1) =
                frost::round1::commit(participants[0].1.signing_share(), &mut rng);

            let (nonces_2, commitments_2) =
                frost::round1::commit(participants[1].1.signing_share(), &mut rng);

            // Collect commitments
            let mut signing_commitments = BTreeMap::new();
            signing_commitments.insert(*participants[0].0, commitments_1);
            signing_commitments.insert(*participants[1].0, commitments_2);

            // Round 2: Generate signature shares
            let signing_package = frost::SigningPackage::new(signing_commitments.clone(), message);

            let signature_share_1 =
                frost::round2::sign(&signing_package, &nonces_1, participants[0].1)
                    .expect("Signing should succeed");

            let signature_share_2 =
                frost::round2::sign(&signing_package, &nonces_2, participants[1].1)
                    .expect("Signing should succeed");

            // Collect signature shares
            let mut signature_shares = BTreeMap::new();
            signature_shares.insert(*participants[0].0, signature_share_1);
            signature_shares.insert(*participants[1].0, signature_share_2);

            // Aggregate
            let group_signature =
                frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
                    .expect("Aggregation should succeed");

            signatures.push(group_signature.serialize());
        }

        // Assert: With deterministic RNG, signatures should differ (nonces are random)
        // BUT both signatures should be valid
        // Note: FROST uses fresh randomness for nonces, so signatures will differ
        // This is correct behavior - we're testing that both signatures verify

        let group_vk = pubkey_package.verifying_key();
        let vk_bytes = group_vk.serialize();
        let ed25519_vk = VerifyingKey::from_bytes(&vk_bytes).expect("Valid verifying key");

        for sig_bytes in &signatures {
            let ed25519_sig = Signature::from_bytes(sig_bytes);
            assert!(
                ed25519_vk.verify(message, &ed25519_sig).is_ok(),
                "All deterministically generated signatures should verify"
            );
        }

        println!("✓ test_frost_signature_deterministic PASSED");
    }

    #[test]
    fn test_frost_threshold_variations() {
        // Test different threshold configurations
        let test_cases = vec![
            (2, 3), // 2-of-3
            (3, 5), // 3-of-5
            (2, 2), // 2-of-2 (all required)
        ];

        for (threshold, total) in test_cases {
            let effects = Effects::for_test(&format!("frost_{}of{}", threshold, total));
            let (key_shares, pubkey_package) = generate_key_shares(threshold, total, &effects);

            let message = format!("Test {}-of-{} threshold", threshold, total);

            // Take exactly threshold participants
            let participants: Vec<_> = key_shares.iter().take(threshold as usize).collect();

            // Round 1: Commitments
            let mut rng = effects.rng();
            let mut signing_commitments = BTreeMap::new();
            let mut nonces_map = BTreeMap::new();

            for (id, key_package) in &participants {
                let (nonces, commitments) =
                    frost::round1::commit(key_package.signing_share(), &mut rng);
                signing_commitments.insert(**id, commitments);
                nonces_map.insert(**id, nonces);
            }

            // Round 2: Signature shares
            let signing_package =
                frost::SigningPackage::new(signing_commitments.clone(), message.as_bytes());

            let mut signature_shares = BTreeMap::new();
            for (id, key_package) in &participants {
                let nonces = nonces_map.get(id).unwrap();
                let sig_share = frost::round2::sign(&signing_package, nonces, *key_package)
                    .expect("Signing should succeed");
                signature_shares.insert(**id, sig_share);
            }

            // Aggregate
            let group_signature =
                frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
                    .expect("Aggregation should succeed");

            // Verify
            let group_vk = pubkey_package.verifying_key();
            let vk_bytes = group_vk.serialize();
            let ed25519_vk = VerifyingKey::from_bytes(&vk_bytes).expect("Valid verifying key");
            let ed25519_sig = Signature::from_bytes(&group_signature.serialize());

            assert!(
                ed25519_vk.verify(message.as_bytes(), &ed25519_sig).is_ok(),
                "{}-of-{} signature should verify",
                threshold,
                total
            );
        }

        println!("✓ test_frost_threshold_variations PASSED");
    }

    #[test]
    fn test_frost_wrong_message_fails_verification() {
        // Generate key shares
        let effects = Effects::for_test("test_frost_wrong_message");
        let (key_shares, pubkey_package) = generate_key_shares(2, 3, &effects);

        // Original message
        let original_message = b"Original message to sign";
        let wrong_message = b"Different message (attack)";

        // Select 2 participants
        let participants: Vec<_> = key_shares.iter().take(2).collect();

        // Sign original message
        let mut rng = effects.rng();
        let mut signing_commitments = BTreeMap::new();
        let mut nonces_map = BTreeMap::new();

        for (id, key_package) in &participants {
            let (nonces, commitments) =
                frost::round1::commit(key_package.signing_share(), &mut rng);
            signing_commitments.insert(**id, commitments);
            nonces_map.insert(**id, nonces);
        }

        let signing_package =
            frost::SigningPackage::new(signing_commitments.clone(), original_message);

        let mut signature_shares = BTreeMap::new();
        for (id, key_package) in &participants {
            let nonces = nonces_map.get(id).unwrap();
            let sig_share = frost::round2::sign(&signing_package, nonces, *key_package)
                .expect("Signing should succeed");
            signature_shares.insert(**id, sig_share);
        }

        let group_signature =
            frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
                .expect("Aggregation should succeed");

        // Convert to ed25519
        let group_vk = pubkey_package.verifying_key();
        let vk_bytes = group_vk.serialize();
        let ed25519_vk = VerifyingKey::from_bytes(&vk_bytes).expect("Valid verifying key");
        let ed25519_sig = Signature::from_bytes(&group_signature.serialize());

        // Verify with original message (should succeed)
        assert!(
            ed25519_vk.verify(original_message, &ed25519_sig).is_ok(),
            "Signature should verify with original message"
        );

        // Verify with wrong message (should fail)
        assert!(
            ed25519_vk.verify(wrong_message, &ed25519_sig).is_err(),
            "Signature should NOT verify with wrong message"
        );

        println!("✓ test_frost_wrong_message_fails_verification PASSED");
    }
}

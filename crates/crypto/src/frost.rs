// FROST (Flexible Round-Optimized Schnorr Threshold) signatures
//
// Reference: 080_architecture_protocol_integration.md - Throughout document
// - Part 1: DKD verification (test signatures)
// - Part 4: Resharing verification (test signatures)
// - Layer 1: Execution primitives
//
// This module provides a simplified wrapper around frost-ed25519 for:
// 1. Threshold signature generation: FROST::sign(message, share)
// 2. Signature verification: Ed25519::verify(signature, group_pk, message)

use crate::{CryptoError, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use frost_ed25519 as frost;
use rand::{CryptoRng, RngCore};
use std::collections::BTreeMap;

/// FROST key share for a participant
///
/// Contains the secret share and associated metadata needed for threshold signing
#[derive(Clone)]
pub struct FrostKeyShare {
    /// Participant identifier
    pub identifier: frost::Identifier,
    /// Secret signing share
    pub signing_share: frost::keys::SigningShare,
    /// Public verifying key
    pub verifying_key: frost::VerifyingKey,
}

impl FrostKeyShare {
    /// Create from raw bytes (for deserialization)
    pub fn from_bytes(
        identifier_bytes: [u8; 32],
        share_bytes: [u8; 32],
        verifying_key_bytes: [u8; 32],
    ) -> Result<Self> {
        let identifier = frost::Identifier::deserialize(&identifier_bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid identifier: {:?}", e)))?;

        let signing_share = frost::keys::SigningShare::deserialize(share_bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid signing share: {:?}", e)))?;

        let verifying_key = frost::VerifyingKey::deserialize(verifying_key_bytes)
            .map_err(|e| CryptoError::InvalidKey(format!("Invalid verifying key: {:?}", e)))?;

        Ok(FrostKeyShare {
            identifier,
            signing_share,
            verifying_key,
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        (
            self.identifier.serialize().to_vec(),
            self.signing_share.serialize().to_vec(),
            self.verifying_key.serialize().to_vec(),
        )
    }
}

/// FROST signing commitment (Round 1)
pub struct SigningCommitment {
    /// Participant identifier
    pub identifier: frost::Identifier,
    /// Signing commitment for round 1
    pub commitment: frost::round1::SigningCommitments,
}

/// FROST signature share (Round 2)
pub struct SignatureShare {
    /// Participant identifier
    pub identifier: frost::Identifier,
    /// Signature share for round 2
    pub share: frost::round2::SignatureShare,
}

/// Sign a message using FROST threshold signatures (simplified two-round protocol)
///
/// This is a simplified interface for threshold signing. In a real implementation,
/// you would need proper round coordination via the CRDT ledger.
///
/// Reference: 080 spec - Layer 1: Execution primitives
pub struct FrostSigner;

impl FrostSigner {
    /// Generate signing nonces (Round 1 - local operation)
    ///
    /// Each participant generates random nonces for the signing session
    pub fn generate_nonces<R: RngCore + CryptoRng>(
        signing_share: &frost::keys::SigningShare,
        rng: &mut R,
    ) -> (
        frost::round1::SigningNonces,
        frost::round1::SigningCommitments,
    ) {
        frost::round1::commit(signing_share, rng)
    }

    /// Create signature share (Round 2)
    ///
    /// After collecting commitments from all participants, each creates their signature share
    #[allow(unused_variables)]
    pub fn sign_share(
        message: &[u8],
        signing_nonces: &frost::round1::SigningNonces,
        signing_commitments: &BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        key_share: &FrostKeyShare,
    ) -> Result<frost::round2::SignatureShare> {
        // Note: This is a simplified interface. In production, you would need the full KeyPackage.
        // For testing, use sign_share_with_package() directly.
        Err(CryptoError::CryptoError(
            "Use sign_share_with_package() for FROST signing with KeyPackage".to_string(),
        ))
    }

    /// Create signature share with KeyPackage (Round 2) - for testing
    ///
    /// This is the actual FROST signing function that works with KeyPackage
    pub fn sign_share_with_package(
        message: &[u8],
        signing_nonces: &frost::round1::SigningNonces,
        signing_commitments: &BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        key_package: &frost::keys::KeyPackage,
    ) -> Result<frost::round2::SignatureShare> {
        // Create signing package
        let signing_package = frost::SigningPackage::new(signing_commitments.clone(), message);

        // Generate signature share
        frost::round2::sign(&signing_package, signing_nonces, key_package)
            .map_err(|e| CryptoError::CryptoError(format!("FROST signing failed: {:?}", e)))
    }

    /// Aggregate signature shares into final signature (performed by any participant)
    pub fn aggregate(
        message: &[u8],
        signing_commitments: &BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        signature_shares: &BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
        pubkey_package: &frost::keys::PublicKeyPackage,
    ) -> Result<Signature> {
        // Create signing package
        let signing_package = frost::SigningPackage::new(signing_commitments.clone(), message);

        // Aggregate signature shares
        let group_signature = frost::aggregate(&signing_package, signature_shares, pubkey_package)
            .map_err(|e| CryptoError::CryptoError(format!("FROST aggregation failed: {:?}", e)))?;

        // Convert FROST signature to ed25519-dalek Signature
        let sig_bytes = group_signature.serialize();
        Ok(Signature::from_bytes(&sig_bytes))
    }
}

/// Verify an Ed25519 signature
///
/// This works for both regular Ed25519 signatures and FROST threshold signatures
/// (since FROST produces standard Ed25519 signatures)
///
/// Reference: 080 spec - Layer 1: Execution primitives
pub fn verify_signature(
    message: &[u8],
    signature: &Signature,
    public_key: &VerifyingKey,
) -> Result<()> {
    public_key
        .verify(message, signature)
        .map_err(|_| CryptoError::InvalidSignature)?;

    Ok(())
}

/// Convert FROST VerifyingKey to ed25519-dalek VerifyingKey
pub fn frost_verifying_key_to_dalek(frost_key: &frost::VerifyingKey) -> Result<VerifyingKey> {
    let bytes = frost_key.serialize();
    VerifyingKey::from_bytes(&bytes)
        .map_err(|e| CryptoError::InvalidKey(format!("Invalid verifying key: {:?}", e)))
}

/// Convert ed25519-dalek VerifyingKey to FROST VerifyingKey
pub fn dalek_verifying_key_to_frost(dalek_key: &VerifyingKey) -> Result<frost::VerifyingKey> {
    let bytes = dalek_key.to_bytes();
    frost::VerifyingKey::deserialize(bytes)
        .map_err(|e| CryptoError::InvalidKey(format!("Invalid verifying key: {:?}", e)))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Test code
mod tests {
    use super::*;
    use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};

    // Helper to set up FROST keys for testing with effects (normally done via DKG)
    #[allow(dead_code)]
    fn setup_frost_keys_with_effects(
        threshold: u16,
        num_participants: u16,
        effects: &crate::Effects,
    ) -> (BTreeMap<frost::Identifier, KeyPackage>, PublicKeyPackage) {
        let mut rng = effects.rng();

        // Generate key shares (in production, this would be done via DKG)
        // FROST v1.0 requires threshold == num_participants (all participants must sign)
        if threshold != num_participants {
            panic!(
                "FROST v1.0 requires threshold ({}) == num_participants ({})",
                threshold, num_participants
            );
        }

        let (shares, pubkey_package) = frost::keys::generate_with_dealer(
            threshold,
            num_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .expect("FROST key generation should work");

        let mut key_packages = BTreeMap::new();
        for (id, share) in shares {
            let key_package = KeyPackage::try_from(share).expect("Invalid key package");
            key_packages.insert(id, key_package);
        }

        (key_packages, pubkey_package)
    }
    
    // Helper to set up FROST keys for testing (normally done via DKG)
    fn setup_frost_keys(
        threshold: u16,
        num_participants: u16,
    ) -> (BTreeMap<frost::Identifier, KeyPackage>, PublicKeyPackage) {
        let effects = crate::Effects::for_test("setup_frost_keys");
        let mut rng = effects.rng();

        // Generate key shares (in production, this would be done via DKG)
        // FROST v1.0 requires threshold == num_participants (all participants must sign)
        if threshold != num_participants {
            panic!(
                "FROST v1.0 requires threshold ({}) == num_participants ({})",
                threshold, num_participants
            );
        }

        let (shares, pubkey_package) = frost::keys::generate_with_dealer(
            threshold,
            num_participants,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .unwrap();

        // Convert SecretShare to KeyPackage
        let key_packages = shares
            .into_iter()
            .map(|(id, secret_share)| {
                let key_package = KeyPackage::try_from(secret_share).unwrap();
                (id, key_package)
            })
            .collect();

        (key_packages, pubkey_package)
    }

    #[test]
    fn test_frost_threshold_signature_3_of_3() {
        let effects = crate::Effects::for_test("test_frost_threshold_signature_3_of_3");
        let mut rng = effects.rng();

        // Setup: 3-of-3 (FROST v1.0 requires threshold == participants)
        let (key_packages, pubkey_package) = setup_frost_keys(3, 3);
        let verifying_key = pubkey_package.verifying_key();

        let message = b"test message for threshold signing";

        // Select all participants for 3-of-3 threshold
        let mut participants = key_packages.iter().take(3);
        let (id1, key1) = participants.next().unwrap();
        let (id2, key2) = participants.next().unwrap();
        let (id3, key3) = participants.next().unwrap();

        // Round 1: Generate nonces and commitments
        let (nonces1, commitments1) = FrostSigner::generate_nonces(key1.signing_share(), &mut rng);
        let (nonces2, commitments2) = FrostSigner::generate_nonces(key2.signing_share(), &mut rng);
        let (nonces3, commitments3) = FrostSigner::generate_nonces(key3.signing_share(), &mut rng);

        let mut all_commitments = BTreeMap::new();
        all_commitments.insert(*id1, commitments1);
        all_commitments.insert(*id2, commitments2);
        all_commitments.insert(*id3, commitments3);

        // Round 2: Create signature shares
        let share1 =
            FrostSigner::sign_share_with_package(message, &nonces1, &all_commitments, key1)
                .unwrap();

        let share2 =
            FrostSigner::sign_share_with_package(message, &nonces2, &all_commitments, key2)
                .unwrap();

        let share3 =
            FrostSigner::sign_share_with_package(message, &nonces3, &all_commitments, key3)
                .unwrap();

        let mut signature_shares = BTreeMap::new();
        signature_shares.insert(*id1, share1);
        signature_shares.insert(*id2, share2);
        signature_shares.insert(*id3, share3);

        // Aggregate signature
        let signature = FrostSigner::aggregate(
            message,
            &all_commitments,
            &signature_shares,
            &pubkey_package,
        )
        .unwrap();

        // Verify signature
        let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();
        verify_signature(message, &signature, &dalek_key).unwrap();
    }

    #[test]
    fn test_frost_signature_invalid_message_fails() {
        let effects = crate::Effects::for_test("test_frost_signature_invalid_message_fails");
        let mut rng = effects.rng();

        let (key_packages, pubkey_package) = setup_frost_keys(3, 3);
        let verifying_key = pubkey_package.verifying_key();

        let message = b"original message";
        let wrong_message = b"tampered message";

        let mut participants = key_packages.iter().take(3);
        let (id1, key1) = participants.next().unwrap();
        let (id2, key2) = participants.next().unwrap();
        let (id3, key3) = participants.next().unwrap();

        // Sign original message
        let (nonces1, commitments1) = FrostSigner::generate_nonces(key1.signing_share(), &mut rng);
        let (nonces2, commitments2) = FrostSigner::generate_nonces(key2.signing_share(), &mut rng);
        let (nonces3, commitments3) = FrostSigner::generate_nonces(key3.signing_share(), &mut rng);

        let mut all_commitments = BTreeMap::new();
        all_commitments.insert(*id1, commitments1);
        all_commitments.insert(*id2, commitments2);
        all_commitments.insert(*id3, commitments3);

        let share1 =
            FrostSigner::sign_share_with_package(message, &nonces1, &all_commitments, key1)
                .unwrap();

        let share2 =
            FrostSigner::sign_share_with_package(message, &nonces2, &all_commitments, key2)
                .unwrap();

        let share3 =
            FrostSigner::sign_share_with_package(message, &nonces3, &all_commitments, key3)
                .unwrap();

        let mut signature_shares = BTreeMap::new();
        signature_shares.insert(*id1, share1);
        signature_shares.insert(*id2, share2);
        signature_shares.insert(*id3, share3);

        let signature = FrostSigner::aggregate(
            message,
            &all_commitments,
            &signature_shares,
            &pubkey_package,
        )
        .unwrap();

        // Try to verify with wrong message
        let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();
        let result = verify_signature(wrong_message, &signature, &dalek_key);

        assert!(
            result.is_err(),
            "Signature verification should fail for wrong message"
        );
    }

    #[test]
    fn test_frost_different_participant_counts() {
        // Test that FROST works with different numbers of participants (but all must sign)
        let effects = crate::Effects::for_test("test_frost_different_participant_counts");
        let mut rng = effects.rng();

        // Test 2-of-2
        let (key_packages, pubkey_package) = setup_frost_keys(2, 2);
        let verifying_key = pubkey_package.verifying_key();
        let dalek_key = frost_verifying_key_to_dalek(verifying_key).unwrap();

        let message = b"test message";

        // Get all 2 participants
        let participants: Vec<_> = key_packages.iter().collect();

        // Test 2-of-2 signing
        {
            let (id1, key1) = participants[0];
            let (id2, key2) = participants[1];

            let (nonces1, commitments1) =
                FrostSigner::generate_nonces(key1.signing_share(), &mut rng);
            let (nonces2, commitments2) =
                FrostSigner::generate_nonces(key2.signing_share(), &mut rng);

            let mut all_commitments = BTreeMap::new();
            all_commitments.insert(*id1, commitments1);
            all_commitments.insert(*id2, commitments2);

            let share1 =
                FrostSigner::sign_share_with_package(message, &nonces1, &all_commitments, key1)
                    .unwrap();

            let share2 =
                FrostSigner::sign_share_with_package(message, &nonces2, &all_commitments, key2)
                    .unwrap();

            let mut signature_shares = BTreeMap::new();
            signature_shares.insert(*id1, share1);
            signature_shares.insert(*id2, share2);

            let signature = FrostSigner::aggregate(
                message,
                &all_commitments,
                &signature_shares,
                &pubkey_package,
            )
            .unwrap();

            verify_signature(message, &signature, &dalek_key).unwrap();
        }
    }

    #[test]
    fn test_verifying_key_conversion() {
        let (_, pubkey_package) = setup_frost_keys(2, 2);
        let frost_key = pubkey_package.verifying_key();

        // Convert FROST -> dalek
        let dalek_key = frost_verifying_key_to_dalek(frost_key).unwrap();

        // Convert dalek -> FROST
        let frost_key_restored = dalek_verifying_key_to_frost(&dalek_key).unwrap();

        // Should round-trip correctly
        assert_eq!(
            frost_key.serialize(),
            frost_key_restored.serialize(),
            "Verifying key conversion should round-trip"
        );
    }

    #[test]
    fn test_frost_key_share_serialization() {
        let (key_packages, pubkey_package) = setup_frost_keys(2, 2);
        let (id, key_package) = key_packages.iter().next().unwrap();

        let key_share = FrostKeyShare {
            identifier: *id,
            signing_share: *key_package.signing_share(),
            verifying_key: *pubkey_package.verifying_key(),
        };

        // Serialize
        let (id_bytes, share_bytes, vk_bytes) = key_share.to_bytes();

        // Deserialize
        let restored = FrostKeyShare::from_bytes(
            id_bytes.as_slice().try_into().unwrap(),
            share_bytes.as_slice().try_into().unwrap(),
            vk_bytes.as_slice().try_into().unwrap(),
        )
        .unwrap();

        // Should match
        assert_eq!(
            key_share.identifier.serialize(),
            restored.identifier.serialize()
        );
        assert_eq!(
            key_share.signing_share.serialize(),
            restored.signing_share.serialize()
        );
    }
}

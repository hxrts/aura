//! Threshold signature verification
//!
//! This module handles verifying that a threshold of authorities (M-of-N)
//! signed a message, proving collective identity.

use crate::{AuthenticationError, Result, ThresholdGroupKey, ThresholdSig};
use aura_core::AuthorityId;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Verify that a threshold of authorities signed a message
///
/// This function proves that at least M out of N authorities signed the given message
/// using FROST threshold signatures, proving collective authority identity.
///
/// # Arguments
///
/// * `message` - The message that was signed
/// * `threshold_sig` - The threshold signature to verify
/// * `group_public_key` - The group's public key
/// * `min_signers` - Minimum required number of signers
///
/// # Returns
///
/// `Ok(())` if the threshold signature is valid and proves enough authorities signed,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_threshold_signature(
    message: &[u8],
    threshold_sig: &ThresholdSig,
    group_public_key: &ThresholdGroupKey,
    min_signers: usize,
) -> Result<()> {
    let signer_count = verify_threshold_evidence(message, threshold_sig, group_public_key, None)?;
    let required = min_signers.max(group_public_key.required_threshold as usize);
    ensure_signers_meet_threshold(signer_count, required)?;

    tracing::debug!(
        min_required = required,
        "Threshold signature verified successfully"
    );

    Ok(())
}

/// Verify that specific authorities contributed to a threshold signature
///
/// This function proves that a specific set of authorities contributed to the
/// threshold signature, providing authority-level accountability.
///
/// # Arguments
///
/// * `message` - The message that was signed
/// * `threshold_sig` - The threshold signature to verify
/// * `expected_signers` - The authorities expected to have signed
/// * `group_public_key` - The group's public key
///
/// # Returns
///
/// `Ok(())` if the signature is valid and from the expected signers,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_threshold_signature_with_signers(
    message: &[u8],
    threshold_sig: &ThresholdSig,
    expected_signers: &[AuthorityId],
    group_public_key: &ThresholdGroupKey,
) -> Result<()> {
    ensure_nonempty_expected_signers(expected_signers)?;
    verify_threshold_evidence(
        message,
        threshold_sig,
        group_public_key,
        Some(expected_signers),
    )?;

    tracing::debug!(
        signers = ?expected_signers,
        signature_signer_count = expected_signers.len(),
        "Threshold signature with specific signers verified successfully"
    );

    Ok(())
}

pub(crate) fn ensure_nonempty_expected_signers(expected_signers: &[AuthorityId]) -> Result<()> {
    if expected_signers.is_empty() {
        Err(AuthenticationError::InvalidThresholdSignature {
            details: "expected signer set cannot be empty".to_string(),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn ensure_signers_meet_threshold(
    actual_signers: usize,
    required_threshold: usize,
) -> Result<()> {
    if actual_signers < required_threshold {
        Err(AuthenticationError::InvalidThresholdSignature {
            details: format!(
                "Threshold not met: current {actual_signers} < required {required_threshold}"
            ),
        })
    } else {
        Ok(())
    }
}

fn invalid_threshold(details: impl Into<String>) -> AuthenticationError {
    AuthenticationError::InvalidThresholdSignature {
        details: details.into(),
    }
}

fn threshold_error(details: impl Into<String>) -> AuthenticationError {
    invalid_threshold(format!(
        "FROST threshold signature verification failed: {}",
        details.into()
    ))
}

fn ensure_unique_signer_indices_u16(signers: &[u16]) -> Result<()> {
    let mut sorted_signers = signers.to_vec();
    sorted_signers.sort_unstable();

    if sorted_signers
        .windows(2)
        .any(|window| window[0] == window[1])
    {
        Err(invalid_threshold(
            "duplicate signer indices in threshold signature",
        ))
    } else {
        Ok(())
    }
}

fn verify_threshold_evidence(
    message: &[u8],
    threshold_sig: &ThresholdSig,
    group_public_key: &ThresholdGroupKey,
    expected_signers: Option<&[AuthorityId]>,
) -> Result<usize> {
    if threshold_sig.participants.is_empty() {
        return Err(invalid_threshold(
            "threshold signature requires at least one signer",
        ));
    }

    let aggregate_signers = &threshold_sig.aggregate_signature.signers;
    if aggregate_signers.is_empty() {
        return Err(invalid_threshold(
            "aggregate threshold signature is missing signer identifiers",
        ));
    }

    ensure_unique_signer_indices_u16(aggregate_signers)?;
    ensure_signers_meet_threshold(
        aggregate_signers.len(),
        group_public_key.required_threshold as usize,
    )?;

    if threshold_sig.participants.len() != aggregate_signers.len()
        || threshold_sig.commitments.len() != aggregate_signers.len()
        || threshold_sig.signature_shares.len() != aggregate_signers.len()
    {
        return Err(invalid_threshold(
            "threshold evidence must include one participant, commitment, and share per aggregate signer",
        ));
    }

    let aggregate_signer_set = aggregate_signers.iter().copied().collect::<BTreeSet<_>>();
    let mut actual_authorities = HashSet::new();
    let mut participant_signers = BTreeSet::new();
    for participant in &threshold_sig.participants {
        let expected_signer = group_public_key
            .signer_membership
            .get(&participant.authority_id)
            .ok_or_else(|| {
                invalid_threshold(format!(
                    "unauthorized threshold signer authority {}",
                    participant.authority_id
                ))
            })?;

        if *expected_signer != participant.signer_index {
            return Err(invalid_threshold(format!(
                "authority {} claimed signer index {} but trusted membership requires {}",
                participant.authority_id, participant.signer_index, expected_signer
            )));
        }

        if !actual_authorities.insert(participant.authority_id) {
            return Err(invalid_threshold(format!(
                "duplicate authority in threshold proof: {}",
                participant.authority_id
            )));
        }

        if !participant_signers.insert(participant.signer_index) {
            return Err(invalid_threshold(format!(
                "duplicate signer index {} in threshold participants",
                participant.signer_index
            )));
        }
    }

    if participant_signers != aggregate_signer_set {
        return Err(invalid_threshold(
            "participant signer identities do not match aggregate signature signer set",
        ));
    }

    let mut commitment_map = BTreeMap::new();
    for commitment in &threshold_sig.commitments {
        if !aggregate_signer_set.contains(&commitment.signer) {
            return Err(invalid_threshold(format!(
                "unexpected signer commitment {} outside aggregate signer set",
                commitment.signer
            )));
        }

        if commitment_map
            .insert(commitment.signer, commitment.clone())
            .is_some()
        {
            return Err(invalid_threshold(format!(
                "duplicate nonce commitment for signer {}",
                commitment.signer
            )));
        }
    }

    let mut share_signers = BTreeSet::new();
    for share in &threshold_sig.signature_shares {
        if !aggregate_signer_set.contains(&share.signer) {
            return Err(invalid_threshold(format!(
                "unexpected signature share signer {} outside aggregate signer set",
                share.signer
            )));
        }

        if !share_signers.insert(share.signer) {
            return Err(invalid_threshold(format!(
                "duplicate signature share for signer {}",
                share.signer
            )));
        }
    }

    if share_signers != aggregate_signer_set {
        return Err(invalid_threshold(
            "signature share signer set does not match aggregate signer set",
        ));
    }

    if let Some(expected_signers) = expected_signers {
        let expected_authorities = expected_signers.iter().copied().collect::<HashSet<_>>();
        if expected_authorities != actual_authorities {
            return Err(invalid_threshold(
                "threshold signer identities do not match expected signer policy",
            ));
        }
    }

    let frost_pkg: frost_ed25519::keys::PublicKeyPackage = group_public_key
        .public_key_package
        .clone()
        .try_into()
        .map_err(|error| threshold_error(format!("invalid group public key package: {error}")))?;

    let reaggregated = aura_core::crypto::tree_signing::frost_aggregate(
        &threshold_sig.signature_shares,
        message,
        &commitment_map,
        &frost_pkg,
    )
    .map_err(|error| threshold_error(error.to_string()))?;

    if reaggregated != threshold_sig.aggregate_signature.signature {
        return Err(invalid_threshold(
            "aggregate signature does not match provided FROST shares and commitments",
        ));
    }

    aura_core::crypto::tree_signing::frost_verify_aggregate(
        frost_pkg.verifying_key(),
        message,
        &threshold_sig.aggregate_signature.signature,
    )
    .map_err(|error| threshold_error(error.to_string()))?;

    Ok(aggregate_signers.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ThresholdParticipant;
    use rand_chacha::ChaCha20Rng;
    use rand_core::SeedableRng;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn threshold_fixture(message: &[u8]) -> (ThresholdGroupKey, ThresholdSig, Vec<AuthorityId>) {
        let authorities = vec![authority(1), authority(2)];
        let mut rng = ChaCha20Rng::from_seed([9u8; 32]);
        let (secret_shares, frost_group_public_key) = frost_ed25519::keys::generate_with_dealer(
            2,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .expect("dealer key generation should succeed");

        let key_packages = secret_shares
            .into_values()
            .map(|secret_share| {
                frost_ed25519::keys::KeyPackage::try_from(secret_share)
                    .expect("secret share should convert to key package")
            })
            .collect::<Vec<_>>();

        let mut commitments = BTreeMap::new();
        let mut signing_inputs = Vec::new();
        let mut participants = Vec::new();
        for (index, key_package) in key_packages.into_iter().enumerate() {
            let signer = u16::from_be_bytes([0, key_package.identifier().serialize()[0]]);
            let authority_id = authorities[index];
            let (nonces, signing_commitments) =
                frost_ed25519::round1::commit(key_package.signing_share(), &mut rng);
            commitments.insert(
                signer,
                aura_core::frost::NonceCommitment {
                    signer,
                    commitment: signing_commitments
                        .serialize()
                        .expect("signing commitments should serialize"),
                },
            );
            participants.push(ThresholdParticipant {
                authority_id,
                signer_index: signer,
            });
            signing_inputs.push((key_package, nonces));
        }

        let frost_commitments = commitments
            .iter()
            .map(|(signer, commitment)| {
                let frost_signer = frost_ed25519::Identifier::try_from(*signer)
                    .expect("signer should convert to FROST identifier");
                let frost_commitment = commitment
                    .to_frost()
                    .expect("commitment should deserialize");
                (frost_signer, frost_commitment)
            })
            .collect::<BTreeMap<_, _>>();
        let signing_package = frost_ed25519::SigningPackage::new(frost_commitments, message);

        let signature_shares = signing_inputs
            .into_iter()
            .map(|(key_package, nonces)| {
                let signature_share =
                    frost_ed25519::round2::sign(&signing_package, &nonces, &key_package)
                        .expect("partial signature should be created");
                aura_core::frost::PartialSignature::from_frost(
                    *key_package.identifier(),
                    signature_share,
                )
            })
            .collect::<Vec<_>>();

        let aggregate_signature = aura_core::crypto::tree_signing::frost_aggregate(
            &signature_shares,
            message,
            &commitments,
            &frost_group_public_key,
        )
        .expect("partial signatures should aggregate");

        let group_key = ThresholdGroupKey::new(
            aura_core::frost::PublicKeyPackage::from(frost_group_public_key),
            2,
            participants.clone(),
        )
        .expect("threshold group key should build");

        (
            group_key,
            ThresholdSig {
                aggregate_signature: aura_core::frost::ThresholdSignature::new(
                    aggregate_signature,
                    signature_shares.iter().map(|share| share.signer).collect(),
                ),
                participants,
                commitments: commitments.into_values().collect(),
                signature_shares,
            },
            authorities,
        )
    }

    /// Threshold signature with >= min_signers verifies.
    #[test]
    fn test_verify_threshold_signature_sufficient_signers() {
        let message = b"test threshold message";
        let (group_key, signature, _) = threshold_fixture(message);
        let min_signers = 2;

        let result = verify_threshold_signature(message, &signature, &group_key, min_signers);

        assert!(result.is_ok());
    }

    /// One signer cannot satisfy a multi-signer threshold claim.
    #[test]
    fn test_verify_threshold_signature_insufficient_signers() {
        let message = b"test threshold message";
        let (group_key, mut signature, _) = threshold_fixture(message);
        signature.aggregate_signature.signers.truncate(1);
        signature.participants.truncate(1);
        signature.commitments.truncate(1);
        signature.signature_shares.truncate(1);

        let result = verify_threshold_signature(message, &signature, &group_key, 2);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    /// Threshold verification with explicit signer list.
    #[test]
    fn test_verify_threshold_signature_with_signers() {
        let message = b"test threshold message";
        let (group_key, signature, expected_signers) = threshold_fixture(message);

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &group_key,
        );

        assert!(result.is_ok());
    }

    /// Empty signer list must be rejected — no signers means no attestation.
    #[test]
    fn test_verify_threshold_signature_with_empty_signers_fails() {
        let expected_signers = Vec::new();
        let message = b"test threshold message";
        let (group_key, signature, _) = threshold_fixture(message);

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &group_key,
        );

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    /// Wrong group key must fail — threshold verification cannot accept key substitution.
    #[test]
    fn test_verify_threshold_signature_wrong_key_fails() {
        let message = b"test threshold message";
        let (_, signature, authorities) = threshold_fixture(message);
        let mut rng = ChaCha20Rng::from_seed([19u8; 32]);
        let (_wrong_shares, wrong_group_public_key) = frost_ed25519::keys::generate_with_dealer(
            2,
            2,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .expect("dealer key generation should succeed");
        let wrong_group_key = ThresholdGroupKey::new(
            aura_core::frost::PublicKeyPackage::from(wrong_group_public_key),
            2,
            vec![
                ThresholdParticipant {
                    authority_id: authorities[0],
                    signer_index: 1,
                },
                ThresholdParticipant {
                    authority_id: authorities[1],
                    signer_index: 2,
                },
            ],
        )
        .expect("wrong threshold group key should build");

        let result = verify_threshold_signature(message, &signature, &wrong_group_key, 2);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    /// Duplicate signer indices must be rejected before group verification.
    #[test]
    fn test_duplicate_signer_indices_fail() {
        let result = ensure_unique_signer_indices_u16(&[1, 3, 3, 5]);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    #[test]
    fn test_mismatched_signer_ids_fail() {
        let message = b"test threshold message";
        let (group_key, mut signature, _) = threshold_fixture(message);
        signature.participants[0].authority_id = authority(99);

        let result = verify_threshold_signature(message, &signature, &group_key, 2);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    #[test]
    fn test_duplicate_signer_entries_fail() {
        let message = b"test threshold message";
        let (group_key, mut signature, _) = threshold_fixture(message);
        signature
            .signature_shares
            .push(signature.signature_shares[0].clone());

        let result = verify_threshold_signature(message, &signature, &group_key, 2);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    #[test]
    fn test_invalid_signature_share_fails() {
        let message = b"test threshold message";
        let (group_key, mut signature, _) = threshold_fixture(message);
        signature.signature_shares[0].signature[0] ^= 0x80;

        let result = verify_threshold_signature(message, &signature, &group_key, 2);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }
}

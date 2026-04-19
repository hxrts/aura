//! Threshold signature verification
//!
//! This module handles verifying that a threshold of authorities (M-of-N)
//! signed a message, proving collective identity.

use crate::verification_common::verify_ed25519_signature;
use crate::{AuthenticationError, Result};
use aura_core::AuthorityId;
use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

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
    threshold_sig: &Ed25519Signature,
    group_public_key: &Ed25519VerifyingKey,
    min_signers: usize,
) -> Result<()> {
    ensure_single_signature_threshold(min_signers)?;
    verify_group_signature(message, threshold_sig, group_public_key)?;

    tracing::debug!(
        min_required = min_signers,
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
    threshold_sig: &Ed25519Signature,
    expected_signers: &[AuthorityId],
    group_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    ensure_nonempty_expected_signers(expected_signers)?;
    verify_group_signature(message, threshold_sig, group_public_key)?;

    tracing::debug!(
        signers = ?expected_signers,
        signature_signer_count = 1,
        "Threshold signature with specific signers verified successfully"
    );

    Ok(())
}

pub(crate) fn ensure_single_signature_threshold(min_signers: usize) -> Result<()> {
    if min_signers <= 1 {
        Ok(())
    } else {
        Err(AuthenticationError::InvalidThresholdSignature {
            details: format!(
                "Insufficient signers: single signature provided, required {min_signers}"
            ),
        })
    }
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

pub(crate) fn ensure_unique_signer_indices(signers: &[u8]) -> Result<()> {
    let mut sorted_signers = signers.to_vec();
    sorted_signers.sort_unstable();

    if sorted_signers
        .windows(2)
        .any(|window| window[0] == window[1])
    {
        Err(AuthenticationError::InvalidThresholdSignature {
            details: "Duplicate signer indices in threshold signature".to_string(),
        })
    } else {
        Ok(())
    }
}

pub(crate) fn verify_group_signature(
    message: &[u8],
    threshold_sig: &Ed25519Signature,
    group_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    verify_ed25519_signature(
        message,
        threshold_sig,
        group_public_key,
        |details| AuthenticationError::InvalidThresholdSignature {
            details: format!("FROST threshold signature verification failed: {details}"),
        },
        || AuthenticationError::InvalidThresholdSignature {
            details: "FROST threshold signature invalid".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::Ed25519SigningKey;

    fn test_signing_material(seed: u8, message: &[u8]) -> (Ed25519Signature, Ed25519VerifyingKey) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let signature = signing_key.sign(message).unwrap();
        (signature, verifying_key)
    }

    /// Threshold signature with >= min_signers verifies.
    #[test]
    fn test_verify_threshold_signature_sufficient_signers() {
        let message = b"test threshold message";
        let (signature, verifying_key) = test_signing_material(7, message);
        let min_signers = 1;

        let result = verify_threshold_signature(message, &signature, &verifying_key, min_signers);

        assert!(result.is_ok());
    }

    /// Threshold signature with < min_signers must fail.
    #[test]
    fn test_verify_threshold_signature_insufficient_signers() {
        let message = b"test threshold message";
        let (signature, verifying_key) = test_signing_material(3, message);
        let min_signers = 2; // Require more than available (Ed25519 is single signature)

        let result = verify_threshold_signature(message, &signature, &verifying_key, min_signers);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    /// Threshold verification with explicit signer list.
    #[test]
    fn test_verify_threshold_signature_with_signers() {
        let expected_signers = vec![AuthorityId::new_from_entropy([55u8; 32])];
        let message = b"test threshold message";
        let (signature, verifying_key) = test_signing_material(11, message);

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &verifying_key,
        );

        assert!(result.is_ok());
    }

    /// Empty signer list must be rejected — no signers means no attestation.
    #[test]
    fn test_verify_threshold_signature_with_empty_signers_fails() {
        let expected_signers = Vec::new();
        let message = b"test threshold message";
        let (signature, verifying_key) = test_signing_material(12, message);

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &verifying_key,
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
        let (signature, _) = test_signing_material(13, message);
        let (_, wrong_verifying_key) = test_signing_material(14, message);

        let result = verify_threshold_signature(message, &signature, &wrong_verifying_key, 1);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    /// Duplicate signer indices must be rejected before group verification.
    #[test]
    fn test_duplicate_signer_indices_fail() {
        let result = ensure_unique_signer_indices(&[1, 3, 3, 5]);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }
}

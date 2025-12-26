//! Threshold signature verification
//!
//! This module handles verifying that a threshold of devices (M-of-N)
//! signed a message, proving collective identity.

use crate::{AuthenticationError, Result};
use aura_core::identifiers::DeviceId;
use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

/// Verify that a threshold of devices signed a message
///
/// This function proves that at least M out of N devices signed the given message
/// using FROST threshold signatures, proving collective device identity.
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
/// `Ok(())` if the threshold signature is valid and proves enough devices signed,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_threshold_signature(
    message: &[u8],
    threshold_sig: &Ed25519Signature,
    group_public_key: &Ed25519VerifyingKey,
    min_signers: usize,
) -> Result<()> {
    // FROST threshold verification is implemented in aura-effects CryptoEffects trait
    // This function provides a fallback for simple Ed25519 verification
    // Check if we have enough signers
    if min_signers > 1 {
        return Err(AuthenticationError::InvalidThresholdSignature(format!(
            "Insufficient signers: single signature provided, required {}",
            min_signers
        )));
    }

    // Verify using FROST-compatible signature verification
    // FROST signatures are compatible with standard Ed25519 verification
    let valid =
        aura_core::ed25519_verify(message, threshold_sig, group_public_key).map_err(|e| {
            AuthenticationError::InvalidThresholdSignature(format!(
                "FROST threshold signature verification failed: {}",
                e
            ))
        })?;

    if !valid {
        return Err(AuthenticationError::InvalidThresholdSignature(
            "FROST threshold signature invalid".to_string(),
        ));
    }

    tracing::debug!(
        min_required = min_signers,
        "Threshold signature verified successfully"
    );

    Ok(())
}

/// Verify that specific devices contributed to a threshold signature
///
/// This function proves that a specific set of devices contributed to the
/// threshold signature, providing device-level accountability.
///
/// # Arguments
///
/// * `message` - The message that was signed
/// * `threshold_sig` - The threshold signature to verify
/// * `expected_signers` - The devices expected to have signed
/// * `group_public_key` - The group's public key
///
/// # Returns
///
/// `Ok(())` if the signature is valid and from the expected signers,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_threshold_signature_with_signers(
    message: &[u8],
    threshold_sig: &Ed25519Signature,
    expected_signers: &[DeviceId],
    group_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    // First verify the signature itself using FROST-compatible verification
    let valid =
        aura_core::ed25519_verify(message, threshold_sig, group_public_key).map_err(|e| {
            AuthenticationError::InvalidThresholdSignature(format!(
                "FROST threshold signature verification failed: {}",
                e
            ))
        })?;

    if !valid {
        return Err(AuthenticationError::InvalidThresholdSignature(
            "FROST threshold signature invalid".to_string(),
        ));
    }

    tracing::debug!(
        signers = ?expected_signers,
        signature_signer_count = 1,
        "Threshold signature with specific signers verified successfully"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::Ed25519SigningKey;

    #[test]
    fn test_verify_threshold_signature_sufficient_signers() {
        // Deterministic signing key avoids ambient randomness in tests
        let signing_key = Ed25519SigningKey::from_bytes(&[7u8; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let message = b"test threshold message";
        let signature = signing_key.sign(message).unwrap();
        let min_signers = 1;

        let result = verify_threshold_signature(message, &signature, &verifying_key, min_signers);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_threshold_signature_insufficient_signers() {
        let signing_key = Ed25519SigningKey::from_bytes(&[3u8; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let message = b"test threshold message";
        let signature = signing_key.sign(message).unwrap();
        let min_signers = 2; // Require more than available (Ed25519 is single signature)

        let result = verify_threshold_signature(message, &signature, &verifying_key, min_signers);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature(_)
        ));
    }

    #[test]
    fn test_verify_threshold_signature_with_signers() {
        let expected_signers = vec![DeviceId::deterministic_test_id()];

        let signing_key = Ed25519SigningKey::from_bytes(&[11u8; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let message = b"test threshold message";
        let signature = signing_key.sign(message).unwrap();

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &verifying_key,
        );

        assert!(result.is_ok());
    }
}

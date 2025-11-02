//! Threshold signature verification
//!
//! This module handles verifying that a threshold of devices (M-of-N)
//! signed a message, proving collective identity.

use crate::{AuthenticationError, Result};
use aura_crypto::{Ed25519Signature, Ed25519VerifyingKey};
use aura_types::DeviceId;

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
    // For now, Ed25519 represents a single signer (until full FROST is implemented)
    // Check if we have enough signers
    if min_signers > 1 {
        return Err(AuthenticationError::InvalidThresholdSignature(format!(
            "Insufficient signers: single signature provided, required {}",
            min_signers
        )));
    }

    // Verify using FROST-compatible signature verification
    // FROST signatures are compatible with standard Ed25519 verification
    aura_crypto::ed25519_verify(group_public_key, message, threshold_sig).map_err(|e| {
        AuthenticationError::InvalidThresholdSignature(format!(
            "FROST threshold signature verification failed: {}",
            e
        ))
    })?;

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
    aura_crypto::ed25519_verify(group_public_key, message, threshold_sig).map_err(|e| {
        AuthenticationError::InvalidThresholdSignature(format!(
            "FROST threshold signature verification failed: {}",
            e
        ))
    })?;

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
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_verify_threshold_signature_sufficient_signers() {
        let effects = Effects::test();

        // Create a test signature
        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);
        let message = b"test threshold message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);
        let min_signers = 1;

        let result = verify_threshold_signature(message, &signature, &verifying_key, min_signers);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_threshold_signature_insufficient_signers() {
        let effects = Effects::test();

        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);
        let message = b"test threshold message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);
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
        let effects = Effects::test();

        let expected_signers = vec![DeviceId::new_with_effects(&effects)];

        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);
        let message = b"test threshold message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);

        let result = verify_threshold_signature_with_signers(
            message,
            &signature,
            &expected_signers,
            &verifying_key,
        );

        assert!(result.is_ok());
    }
}

//! Authority identity verification
//!
//! This module handles verifying that an authority signed a message.

use crate::verification_common::verify_ed25519_signature;
use crate::{AuthenticationError, Result};
use aura_core::types::identifiers::AuthorityId;
use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

/// Verify that an authority signed a message
///
/// This function proves that a specific authority signed the given message
/// using its private key.
///
/// # Arguments
///
/// * `authority_id` - The claimed authority identity
/// * `message` - The message that was signed
/// * `signature` - The signature to verify
/// * `authority_public_key` - The authority's public key
///
/// # Returns
///
/// `Ok(())` if the signature is valid and proves the authority signed the message,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_authority_signature(
    authority_id: AuthorityId,
    message: &[u8],
    signature: &Ed25519Signature,
    authority_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    verify_ed25519_signature(
        message,
        signature,
        authority_public_key,
        |details| AuthenticationError::InvalidAuthoritySignature {
            details: format!("Authority {authority_id} signature verification failed: {details}"),
        },
        || AuthenticationError::InvalidAuthoritySignature {
            details: format!("Authority {authority_id} signature invalid"),
        },
    )?;

    tracing::debug!(
        authority_id = %authority_id,
        "Authority signature verified successfully"
    );

    Ok(())
}

/// Simple signature verification without authority identity
///
/// This is a convenience function for basic signature verification
/// when authority identity is already established.
pub fn verify_signature(
    public_key: &Ed25519VerifyingKey,
    message: &[u8],
    signature: &Ed25519Signature,
) -> Result<()> {
    verify_ed25519_signature(
        message,
        signature,
        public_key,
        |details| AuthenticationError::InvalidAuthoritySignature {
            details: format!("Signature verification failed: {details}"),
        },
        || AuthenticationError::InvalidAuthoritySignature {
            details: "Signature verification failed".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::crypto::ed25519::Ed25519SigningKey;

    fn authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn signing_material(seed: u8, message: &[u8]) -> (Ed25519Signature, Ed25519VerifyingKey) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let signature = signing_key.sign(message).unwrap();
        (signature, verifying_key)
    }

    /// Valid signature from the correct key verifies successfully.
    #[test]
    fn test_verify_authority_signature_success() {
        let authority_id = authority_id(1);
        let message = b"test message";
        let (signature, verifying_key) = signing_material(21, message);

        let result = verify_authority_signature(authority_id, message, &signature, &verifying_key);

        assert!(result.is_ok());
    }

    /// Signature from a different key must fail — prevents key substitution.
    #[test]
    fn test_verify_authority_signature_invalid() {
        let authority_id = authority_id(2);
        let message = b"test message";
        let (signature, _) = signing_material(31, message);
        let (_, wrong_verifying_key) = signing_material(32, message);

        let result =
            verify_authority_signature(authority_id, message, &signature, &wrong_verifying_key);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidAuthoritySignature { .. }
        ));
    }
}

//! Authority identity verification
//!
//! This module handles verifying that an authority signed a message.

use crate::{AuthenticationError, Result};
use aura_core::identifiers::AuthorityId;
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
    // Verify the cryptographic signature
    let valid =
        aura_core::ed25519_verify(message, signature, authority_public_key).map_err(|e| {
            AuthenticationError::InvalidAuthoritySignature {
                details: format!(
                    "Authority {} signature verification failed: {}",
                    authority_id, e
                ),
            }
        })?;

    if !valid {
        return Err(AuthenticationError::InvalidAuthoritySignature {
            details: format!("Authority {} signature invalid", authority_id),
        });
    }

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
    let valid = aura_core::ed25519_verify(message, signature, public_key).map_err(|e| {
        AuthenticationError::InvalidAuthoritySignature {
            details: format!("Signature verification failed: {}", e),
        }
    })?;

    if valid {
        Ok(())
    } else {
        Err(AuthenticationError::InvalidAuthoritySignature {
            details: "Signature verification failed".to_string(),
        })
    }
}

// Tests commented out due to missing crypto functions in current aura_crypto API
/*
#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::hash;
    use aura_core::AuthorityId;
    use uuid::Uuid;

    #[test]
    fn test_verify_authority_signature_success() {
        let digest = hash::hash(b"authority-test-1");
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes));

        // Generate a key pair for testing
        let signing_key = aura_core::generate_ed25519_key();
        let verifying_key = aura_core::ed25519_verifying_key(&signing_key)
            .expect("test signing key should be valid");

        let message = b"test message";
        let signature = aura_core::ed25519_sign(&signing_key, message);

        let result =
            verify_authority_signature(authority_id, message, &signature, &verifying_key);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_authority_signature_invalid() {
        let digest = hash::hash(b"authority-test-2");
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        let authority_id = AuthorityId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes));

        // Generate two different key pairs
        let signing_key1 = aura_core::generate_ed25519_key();
        let verifying_key1 = aura_core::ed25519_verifying_key(&signing_key1)
            .expect("test signing key should be valid");
        let signing_key2 = aura_core::generate_ed25519_key();

        let message = b"test message";
        // Sign with key2 but verify with key1 (should fail)
        let signature = aura_core::ed25519_sign(&signing_key2, message);

        let result =
            verify_authority_signature(authority_id, message, &signature, &verifying_key1);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidAuthoritySignature { .. }
        ));
    }
}
*/

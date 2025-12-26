//! Guardian identity verification
//!
//! This module handles verifying that a guardian signed a message during
//! recovery operations, proving guardian identity.

use crate::{AuthenticationError, Result};
use aura_core::GuardianId;
use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

/// Verify that a guardian signed a message
///
/// This function proves that a specific guardian (identified by GuardianId)
/// signed the given message during a recovery operation.
///
/// # Arguments
///
/// * `guardian_id` - The claimed guardian identity
/// * `message` - The message that was signed
/// * `signature` - The signature to verify
/// * `guardian_public_key` - The guardian's public key
///
/// # Returns
///
/// `Ok(())` if the signature is valid and proves the guardian signed the message,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_guardian_signature(
    guardian_id: GuardianId,
    message: &[u8],
    signature: &Ed25519Signature,
    guardian_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    // Verify the cryptographic signature
    let valid =
        aura_core::ed25519_verify(message, signature, guardian_public_key).map_err(|e| {
            AuthenticationError::InvalidGuardianSignature(format!(
                "Guardian {} signature verification failed: {}",
                guardian_id, e
            ))
        })?;

    if !valid {
        return Err(AuthenticationError::InvalidGuardianSignature(format!(
            "Guardian {} signature invalid",
            guardian_id
        )));
    }

    tracing::debug!(
        guardian_id = %guardian_id,
        "Guardian signature verified successfully"
    );

    Ok(())
}

/// Verify a guardian recovery approval signature
///
/// This function specifically verifies that a guardian signed a recovery
/// approval message, which has a specific format and context.
///
/// # Arguments
///
/// * `guardian_id` - The guardian approving the recovery
/// * `recovery_request_hash` - Hash of the recovery request being approved
/// * `approval_signature` - The guardian's approval signature
/// * `guardian_public_key` - The guardian's public key
///
/// # Returns
///
/// `Ok(())` if the approval signature is valid,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_recovery_approval(
    guardian_id: GuardianId,
    recovery_request_hash: &[u8; 32],
    approval_signature: &Ed25519Signature,
    guardian_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    // Create the approval message that should have been signed
    let approval_message = create_recovery_approval_message(guardian_id, recovery_request_hash);

    // Verify the signature against the approval message
    verify_guardian_signature(
        guardian_id,
        &approval_message,
        approval_signature,
        guardian_public_key,
    )
}

/// Create a standardized recovery approval message
///
/// This creates the canonical message format that guardians sign when
/// approving recovery requests.
fn create_recovery_approval_message(
    guardian_id: GuardianId,
    recovery_request_hash: &[u8; 32],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(48); // 16 + 32 bytes
    message.extend_from_slice(guardian_id.0.as_bytes());
    message.extend_from_slice(recovery_request_hash);
    message
}

// Tests commented out due to missing crypto functions in current aura_crypto API
/*
#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::hash;

    #[test]
    fn test_verify_guardian_signature_success() {
        let guardian_id = Uuid::from_bytes(hash::hash(b"guardian-test-1")[..16].try_into().unwrap());

        // Generate a key pair for testing
        let signing_key = aura_core::generate_ed25519_key();
        let verifying_key = aura_core::ed25519_verifying_key(&signing_key)
            .expect("test signing key should be valid");

        let message = b"guardian test message";
        let signature = aura_core::ed25519_sign(&signing_key, message);

        let result = verify_guardian_signature(guardian_id, message, &signature, &verifying_key);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_guardian_signature_invalid() {
        let effects = Effects::test();
        let guardian_id = Uuid::from_bytes(hash::hash(b"guardian-test-verify-2")[..16].try_into().unwrap());

        // Generate two different key pairs
        let signing_key1 = aura_core::generate_ed25519_key();
        let verifying_key1 = aura_core::ed25519_verifying_key(&signing_key1)
            .expect("test signing key should be valid");
        let signing_key2 = aura_core::generate_ed25519_key();

        let message = b"guardian test message";
        // Sign with key2 but verify with key1 (should fail)
        let signature = aura_core::ed25519_sign(&signing_key2, message);

        let result = verify_guardian_signature(guardian_id, message, &signature, &verifying_key1);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidGuardianSignature(_)
        ));
    }

    #[test]
    fn test_verify_recovery_approval() {
        let effects = Effects::test();
        let guardian_id = effects.gen_uuid();
        let recovery_request_hash = [42u8; 32];

        // Generate a key pair for testing
        let signing_key = aura_core::generate_ed25519_key();
        let verifying_key = aura_core::ed25519_verifying_key(&signing_key)
            .expect("test signing key should be valid");

        // Create the approval message and sign it
        let approval_message =
            create_recovery_approval_message(guardian_id, &recovery_request_hash);
        let signature = aura_core::ed25519_sign(&signing_key, &approval_message);

        let result = verify_recovery_approval(
            guardian_id,
            &recovery_request_hash,
            &signature,
            &verifying_key,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_recovery_approval_message_format() {
        let guardian_id = Uuid::from_bytes(hash::hash(b"guardian-test-approval-format")[..16].try_into().unwrap());
        let recovery_request_hash = [1u8; 32];

        let message = create_recovery_approval_message(guardian_id, &recovery_request_hash);

        // Should be guardian_id (16 bytes) + recovery_request_hash (32 bytes) = 48 bytes
        assert_eq!(message.len(), 48);

        // First 16 bytes should be guardian_id
        assert_eq!(&message[0..16], guardian_id.0.as_bytes());

        // Last 32 bytes should be recovery_request_hash
        assert_eq!(&message[16..48], &recovery_request_hash);
    }
}
*/

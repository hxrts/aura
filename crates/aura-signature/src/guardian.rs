//! Guardian identity verification
//!
//! This module handles verifying that a guardian signed a message during
//! recovery operations, proving guardian identity.

use crate::verification_common::verify_ed25519_signature;
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
    verify_ed25519_signature(
        message,
        signature,
        guardian_public_key,
        |details| AuthenticationError::InvalidGuardianSignature {
            details: format!("Guardian {guardian_id} signature verification failed: {details}"),
        },
        || AuthenticationError::InvalidGuardianSignature {
            details: format!("Guardian {guardian_id} signature invalid"),
        },
    )?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::crypto::ed25519::Ed25519SigningKey;

    fn guardian_id(seed: u8) -> GuardianId {
        GuardianId::new_from_entropy([seed; 32])
    }

    fn signing_material(seed: u8, message: &[u8]) -> (Ed25519Signature, Ed25519VerifyingKey) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let signature = signing_key.sign(message).unwrap();
        (signature, verifying_key)
    }

    /// Valid guardian signature verifies — happy path for recovery approval.
    #[test]
    fn test_verify_guardian_signature_success() {
        let guardian_id = guardian_id(1);
        let message = b"guardian test message";
        let (signature, verifying_key) = signing_material(41, message);

        let result = verify_guardian_signature(guardian_id, message, &signature, &verifying_key);

        assert!(result.is_ok());
    }

    /// Wrong key must fail — prevents key substitution in recovery.
    #[test]
    fn test_verify_guardian_signature_invalid() {
        let guardian_id = guardian_id(2);
        let message = b"guardian test message";
        let (signature, _) = signing_material(51, message);
        let (_, wrong_verifying_key) = signing_material(52, message);

        let result =
            verify_guardian_signature(guardian_id, message, &signature, &wrong_verifying_key);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidGuardianSignature { .. }
        ));
    }

    /// Recovery approval: valid guardian signature with correct ceremony ID.
    #[test]
    fn test_verify_recovery_approval() {
        let guardian_id = guardian_id(3);
        let recovery_request_hash = [42u8; 32];

        let approval_message =
            create_recovery_approval_message(guardian_id, &recovery_request_hash);
        let (signature, verifying_key) = signing_material(61, &approval_message);

        let result = verify_recovery_approval(
            guardian_id,
            &recovery_request_hash,
            &signature,
            &verifying_key,
        );

        assert!(result.is_ok());
    }

    /// Recovery approval binding message is deterministic for same inputs.
    #[test]
    fn test_recovery_approval_message_format() {
        let guardian_id = guardian_id(4);
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

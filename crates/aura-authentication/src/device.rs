//! Device identity verification
//!
//! This module handles verifying that a device with a specific DeviceId
//! signed a message, proving device identity.

use crate::{AuthenticationError, Result};
use aura_crypto::{Ed25519Signature, Ed25519VerifyingKey};
use aura_types::DeviceId;

/// Verify that a device signed a message
///
/// This function proves that a specific device (identified by DeviceId)
/// signed the given message using their private key.
///
/// # Arguments
///
/// * `device_id` - The claimed device identity
/// * `message` - The message that was signed
/// * `signature` - The signature to verify
/// * `device_public_key` - The device's public key
///
/// # Returns
///
/// `Ok(())` if the signature is valid and proves the device signed the message,
/// `Err(AuthenticationError)` otherwise.
pub fn verify_device_signature(
    device_id: DeviceId,
    message: &[u8],
    signature: &Ed25519Signature,
    device_public_key: &Ed25519VerifyingKey,
) -> Result<()> {
    // Verify the cryptographic signature
    aura_crypto::ed25519_verify(device_public_key, message, signature).map_err(|e| {
        AuthenticationError::InvalidDeviceSignature(format!(
            "Device {} signature verification failed: {}",
            device_id, e
        ))
    })?;

    tracing::debug!(
        device_id = %device_id,
        "Device signature verified successfully"
    );

    Ok(())
}

/// Simple signature verification without device identity
///
/// This is a convenience function for basic signature verification
/// when device identity is already established.
pub fn verify_signature(
    public_key: &Ed25519VerifyingKey,
    message: &[u8],
    signature: &Ed25519Signature,
) -> Result<()> {
    aura_crypto::ed25519_verify(public_key, message, signature).map_err(|e| {
        AuthenticationError::InvalidDeviceSignature(format!("Signature verification failed: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_verify_device_signature_success() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Generate a key pair for testing
        let signing_key = aura_crypto::generate_ed25519_key();
        let verifying_key = aura_crypto::ed25519_verifying_key(&signing_key);

        let message = b"test message";
        let signature = aura_crypto::ed25519_sign(&signing_key, message);

        let result = verify_device_signature(device_id, message, &signature, &verifying_key);

        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_device_signature_invalid() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Generate two different key pairs
        let signing_key1 = aura_crypto::generate_ed25519_key();
        let verifying_key1 = aura_crypto::ed25519_verifying_key(&signing_key1);
        let signing_key2 = aura_crypto::generate_ed25519_key();

        let message = b"test message";
        // Sign with key2 but verify with key1 (should fail)
        let signature = aura_crypto::ed25519_sign(&signing_key2, message);

        let result = verify_device_signature(device_id, message, &signature, &verifying_key1);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidDeviceSignature(_)
        ));
    }
}

// Encryption and decryption for stored objects

use crate::{CryptoError, Effects, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};

/// Encryption context
pub struct EncryptionContext {
    /// Content encryption key
    key: [u8; 32],
}

impl EncryptionContext {
    /// Generate a new random encryption key
    pub fn new(effects: &Effects) -> Self {
        let key: [u8; 32] = effects.random_bytes();
        EncryptionContext { key }
    }

    /// From existing key material
    pub fn from_key(key: [u8; 32]) -> Self {
        EncryptionContext { key }
    }

    /// Encrypt data
    pub fn encrypt(&self, plaintext: &[u8], effects: &Effects) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| CryptoError::CryptoError(format!("Failed to create cipher: {}", e)))?;

        // Generate random nonce using injected effects
        let nonce_bytes: [u8; 12] = effects.random_bytes();
        let nonce = Nonce::from(nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| CryptoError::CryptoError(format!("Encryption failed: {}", e)))?;

        // Prepend nonce to ciphertext
        let mut result = nonce_bytes.to_vec();
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt data
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(CryptoError::CryptoError("Ciphertext too short".to_string()));
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| CryptoError::CryptoError(format!("Failed to create cipher: {}", e)))?;

        // Extract nonce and actual ciphertext
        let nonce_bytes: [u8; 12] = ciphertext[..12]
            .try_into()
            .map_err(|_| CryptoError::CryptoError("Invalid nonce length".to_string()))?;
        let nonce = Nonce::from(nonce_bytes);
        let actual_ciphertext = &ciphertext[12..];

        // Decrypt
        let plaintext = cipher
            .decrypt(&nonce, actual_ciphertext)
            .map_err(|e| CryptoError::CryptoError(format!("Decryption failed: {}", e)))?;

        Ok(plaintext)
    }

    /// Get the encryption key
    pub fn key(&self) -> &[u8; 32] {
        &self.key
    }
}

// Note: Removed Default impl since we need Effects parameter
// Use EncryptionContext::new(effects) instead

/// Recipient for key wrapping
#[derive(Debug, Clone)]
pub enum Recipients {
    /// Broadcast to all devices
    Broadcast,
    /// Specific devices with their device secrets
    Devices(Vec<RecipientDevice>),
}

/// Device recipient information for key wrapping
#[derive(Debug, Clone)]
pub struct RecipientDevice {
    /// Device identifier
    pub device_id: crate::DeviceId,
    /// Device secret for key wrapping
    pub device_secret: [u8; 32],
}

/// Key envelope containing wrapped keys for recipients
///
/// Uses AES-256-GCM sealing from aura-crypto instead of insecure XOR.
/// Each recipient gets their own sealed copy of the content key.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyEnvelope {
    /// Sealed keys, one per recipient
    pub wrapped_keys: Vec<WrappedKey>,
}

/// A single wrapped key for one recipient
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WrappedKey {
    /// Device ID of recipient
    pub device_id: crate::DeviceId,
    /// Sealed content key
    pub sealed_key: crate::SealedData,
}

/// Wrap a content encryption key for recipients using secure AES-256-GCM sealing
///
/// Replaces the insecure XOR placeholder with proper authenticated encryption.
/// Each recipient can unwrap using their device secret.
pub fn wrap_key_for_recipients(
    key: &[u8; 32],
    recipients: &Recipients,
    effects: &Effects,
) -> Result<KeyEnvelope> {
    let wrapped_keys = match recipients {
        Recipients::Broadcast => {
            // For broadcast, we would need to get all active devices from the ledger
            // This function now requires explicit device information instead of implicit broadcast
            // Callers should use Recipients::Devices with all devices from ledger
            return Err(CryptoError::InvalidParameter(
                "Broadcast recipients require explicit device list from ledger".to_string()
            ));
        }
        Recipients::Devices(devices) => devices
            .iter()
            .map(|recipient| {
                let context = format!("aura-content-key-v1:{}", recipient.device_id.0);

                let sealed_key = crate::SealedData::seal_value(
                    key,
                    &recipient.device_secret,
                    &context,
                    None,
                    effects,
                )
                .map_err(|e| CryptoError::CryptoError(format!("Key wrapping failed: {}", e)))?;

                Ok(WrappedKey {
                    device_id: recipient.device_id,
                    sealed_key,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    };

    Ok(KeyEnvelope { wrapped_keys })
}

/// Create recipients from device information retrieved from ledger
///
/// This helper function creates Recipients::Devices from device information
/// that should be retrieved from the AccountLedger by the caller.
pub fn create_recipients_from_devices(device_info: Vec<(crate::DeviceId, [u8; 32])>) -> Recipients {
    let devices = device_info
        .into_iter()
        .map(|(device_id, device_secret)| RecipientDevice {
            device_id,
            device_secret,
        })
        .collect();
    Recipients::Devices(devices)
}

/// Create recipients for all devices using a function to derive device secrets
///
/// The device_secret_fn should derive a device secret from a device ID,
/// typically using deterministic key derivation from the device ID.
pub fn create_recipients_for_devices(
    device_ids: Vec<crate::DeviceId>,
    device_secret_fn: impl Fn(&crate::DeviceId) -> [u8; 32],
) -> Recipients {
    let devices = device_ids
        .into_iter()
        .map(|device_id| {
            let device_secret = device_secret_fn(&device_id);
            RecipientDevice {
                device_id,
                device_secret,
            }
        })
        .collect();
    Recipients::Devices(devices)
}

/// Unwrap a content encryption key using device secret
///
/// Attempts to find and unwrap the key for the specified device.
pub fn unwrap_key(
    envelope: &KeyEnvelope,
    device_id: crate::DeviceId,
    device_secret: &[u8; 32],
) -> Result<[u8; 32]> {
    // Find the wrapped key for this device
    let wrapped = envelope
        .wrapped_keys
        .iter()
        .find(|w| w.device_id == device_id)
        .ok_or_else(|| {
            CryptoError::CryptoError(format!("No key found for device {}", device_id.0))
        })?;

    // Unwrap using the unified crypto utilities
    wrapped
        .sealed_key
        .unseal_value(device_secret)
        .map_err(|e| CryptoError::CryptoError(format!("Key unwrapping failed: {}", e)))
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        let effects = Effects::test();
        let ctx = EncryptionContext::new(&effects);
        let plaintext = b"Hello, world!";

        let ciphertext = ctx.encrypt(plaintext, &effects).unwrap();
        assert_ne!(ciphertext.as_slice(), plaintext);

        let decrypted = ctx.decrypt(&ciphertext).unwrap();
        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_key_wrapping() {
        let effects = Effects::test();
        let key = [42u8; 32];
        let device_id = crate::DeviceId::new_with_effects(&effects);
        let device_secret: [u8; 32] = effects.random_bytes();

        let recipients = Recipients::Devices(vec![RecipientDevice {
            device_id,
            device_secret,
        }]);

        let envelope = wrap_key_for_recipients(&key, &recipients, &effects).unwrap();
        assert_eq!(envelope.wrapped_keys.len(), 1);

        let unwrapped = unwrap_key(&envelope, device_id, &device_secret).unwrap();
        assert_eq!(key, unwrapped);
    }

    #[test]
    fn test_key_wrapping_multiple_devices() {
        let effects = Effects::test();
        let key = [42u8; 32];
        let device1_id = crate::DeviceId::new_with_effects(&effects);
        let device1_secret: [u8; 32] = effects.random_bytes();
        let device2_id = crate::DeviceId::new_with_effects(&effects);
        let device2_secret: [u8; 32] = effects.random_bytes();

        let recipients = Recipients::Devices(vec![
            RecipientDevice {
                device_id: device1_id,
                device_secret: device1_secret,
            },
            RecipientDevice {
                device_id: device2_id,
                device_secret: device2_secret,
            },
        ]);

        let envelope = wrap_key_for_recipients(&key, &recipients, &effects).unwrap();
        assert_eq!(envelope.wrapped_keys.len(), 2);

        // Both devices can unwrap
        let unwrapped1 = unwrap_key(&envelope, device1_id, &device1_secret).unwrap();
        let unwrapped2 = unwrap_key(&envelope, device2_id, &device2_secret).unwrap();
        assert_eq!(key, unwrapped1);
        assert_eq!(key, unwrapped2);
    }

    #[test]
    fn test_key_unwrapping_wrong_device_fails() {
        let effects = Effects::test();
        let key = [42u8; 32];
        let device_id = crate::DeviceId::new_with_effects(&effects);
        let device_secret: [u8; 32] = effects.random_bytes();
        let wrong_device_id = crate::DeviceId::new_with_effects(&effects);

        let recipients = Recipients::Devices(vec![RecipientDevice {
            device_id,
            device_secret,
        }]);

        let envelope = wrap_key_for_recipients(&key, &recipients, &effects).unwrap();

        // Wrong device ID should fail
        let result = unwrap_key(&envelope, wrong_device_id, &device_secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_encryption_deterministic() {
        let effects1 = Effects::deterministic(42, 1000);
        let effects2 = Effects::deterministic(42, 1000);

        let ctx1 = EncryptionContext::new(&effects1);
        let ctx2 = EncryptionContext::new(&effects2);

        // Same effects should produce same key
        assert_eq!(ctx1.key(), ctx2.key());

        let plaintext = b"Hello, deterministic world!";
        let ciphertext1 = ctx1.encrypt(plaintext, &effects1).unwrap();
        let ciphertext2 = ctx2.encrypt(plaintext, &effects2).unwrap();

        // Same effects should produce same ciphertext (same nonce)
        assert_eq!(ciphertext1, ciphertext2);
    }
}

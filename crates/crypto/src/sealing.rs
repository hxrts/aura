//! Secure sealing (encryption) for sensitive data at rest
//!
//! Provides generic AEAD encryption using AES-256-GCM with proper key derivation.
//! Used for protecting key shares and other sensitive material stored locally.

use crate::{CryptoError, Effects, Result};
use aes_gcm::{Aes256Gcm, Key, aead::{Aead, KeyInit}};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Sealed (encrypted) data container
///
/// Generic container for any data that needs to be encrypted at rest.
/// Uses AES-256-GCM with BLAKE3-based key derivation.
///
/// # Security
///
/// - All sensitive fields are zeroized on drop
/// - Nonces are random and never reused
/// - Associated data binds encryption to context
/// - Authenticated encryption prevents tampering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedData {
    /// Encrypted payload - zeroized on drop
    pub ciphertext: Vec<u8>,
    /// Random nonce for GCM (12 bytes)
    pub nonce: [u8; 12],
    /// Context string used for key derivation
    pub context: String,
    /// Optional associated data for verification
    pub associated_data: Option<Vec<u8>>,
}

impl SealedData {
    /// Seal (encrypt) a serializable value with a device-specific secret
    ///
    /// # Arguments
    ///
    /// * `value` - Data to encrypt (must implement Serialize)
    /// * `device_secret` - 32-byte device-specific secret from secure storage
    /// * `context` - Context string for key derivation (e.g., "aura-share-v1:device123")
    /// * `associated_data` - Optional additional data to authenticate (not encrypted)
    /// * `effects` - Injectable effects for deterministic randomness
    ///
    /// # Security
    ///
    /// - Value is serialized with bincode
    /// - Device secret is used for BLAKE3-based key derivation
    /// - Random nonce is generated per encryption
    /// - Context binds encryption to specific use case
    /// - Associated data is authenticated but not encrypted
    pub fn seal_value<T: Serialize>(
        value: &T,
        device_secret: &[u8; 32],
        context: &str,
        associated_data: Option<&[u8]>,
        effects: &Effects,
    ) -> Result<Self> {
        // Serialize the value
        let plaintext = bincode::serialize(value)
            .map_err(|e| CryptoError::SerializationError(format!("Failed to serialize: {}", e)))?;

        // Generate random nonce for AES-GCM
        let nonce: [u8; 12] = effects.random_bytes();
        let gcm_nonce = &nonce.into();

        // Derive encryption key using BLAKE3
        let encryption_key = derive_key(device_secret, context)?;
        let cipher = Aes256Gcm::new(&encryption_key);

        // Encrypt with associated data authentication
        let aad = associated_data.unwrap_or(&[]);
        let ciphertext = cipher
            .encrypt(gcm_nonce, aes_gcm::aead::Payload { msg: &plaintext, aad })
            .map_err(|e| CryptoError::EncryptionFailed(format!("AES-GCM encryption failed: {}", e)))?;

        Ok(SealedData {
            ciphertext,
            nonce,
            context: context.to_string(),
            associated_data: associated_data.map(|d| d.to_vec()),
        })
    }

    /// Unseal (decrypt) data and deserialize back to original type
    ///
    /// # Arguments
    ///
    /// * `device_secret` - 32-byte device-specific secret used for sealing
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Authentication tag verification fails (tampering detected)
    /// - Wrong device secret provided
    /// - Deserialization fails
    pub fn unseal_value<T: serde::de::DeserializeOwned>(
        &self,
        device_secret: &[u8; 32],
    ) -> Result<T> {
        // Derive the same encryption key using BLAKE3
        let encryption_key = derive_key(device_secret, &self.context)?;
        let cipher = Aes256Gcm::new(&encryption_key);

        // Prepare nonce and associated data
        let gcm_nonce = &self.nonce.into();
        let aad = self.associated_data.as_deref().unwrap_or(&[]);

        // Decrypt the ciphertext
        let plaintext = cipher
            .decrypt(gcm_nonce, aes_gcm::aead::Payload { msg: &self.ciphertext, aad })
            .map_err(|e| CryptoError::DecryptionFailed(format!("AES-GCM decryption failed: {}", e)))?;

        // Deserialize the decrypted data
        bincode::deserialize(&plaintext)
            .map_err(|e| CryptoError::DecryptionFailed(format!("Failed to deserialize: {}", e)))
    }
}

/// Derive AES-256 key from device secret and context using BLAKE3
/// 
/// Uses BLAKE3 key derivation function with domain separation.
/// The context string prevents key reuse across different applications.
fn derive_key(device_secret: &[u8; 32], context: &str) -> Result<Key<Aes256Gcm>> {
    let mut hasher = Hasher::new();
    
    // Domain separation prefix
    hasher.update(b"aura-sealing-v1:");
    
    // Include device secret
    hasher.update(device_secret);
    
    // Include context for key separation
    hasher.update(b":");
    hasher.update(context.as_bytes());
    
    // Extract 32 bytes for AES-256 key
    let derived_key: [u8; 32] = hasher.finalize().into();
    
    Ok(derived_key.into())
}

impl Drop for SealedData {
    fn drop(&mut self) {
        // Zeroize sensitive data on drop
        self.ciphertext.zeroize();
        if let Some(ref mut aad) = self.associated_data {
            aad.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seal_unseal_roundtrip() {
        let effects = Effects::test();
        let device_secret: [u8; 32] = effects.random_bytes();
        let original_value = vec![1u8, 2, 3, 4, 5];

        let sealed = SealedData::seal_value(
            &original_value,
            &device_secret,
            "test-context",
            None,
            &effects,
        )
        .unwrap();

        let unsealed: Vec<u8> = sealed.unseal_value(&device_secret).unwrap();

        assert_eq!(original_value, unsealed);
    }

    #[test]
    fn test_seal_deterministic_with_same_effects() {
        let effects1 = Effects::deterministic(42, 1000);
        let effects2 = Effects::deterministic(42, 1000);
        let device_secret: [u8; 32] = [1u8; 32];
        let value = vec![1u8, 2, 3, 4, 5];

        let sealed1 =
            SealedData::seal_value(&value, &device_secret, "test-context", None, &effects1)
                .unwrap();
        let sealed2 =
            SealedData::seal_value(&value, &device_secret, "test-context", None, &effects2)
                .unwrap();

        // Same effects should produce same nonce (deterministic)
        assert_eq!(sealed1.nonce, sealed2.nonce);
        assert_eq!(sealed1.ciphertext, sealed2.ciphertext);
    }

    #[test]
    fn test_wrong_key_fails_decryption() {
        let effects = Effects::test();
        let device_secret: [u8; 32] = effects.random_bytes();
        let wrong_secret: [u8; 32] = effects.random_bytes();
        let original_value = vec![1u8, 2, 3, 4, 5];

        let sealed = SealedData::seal_value(
            &original_value,
            &device_secret,
            "test-context",
            None,
            &effects,
        )
        .unwrap();

        // Wrong key should fail decryption
        let result: Result<Vec<u8>> = sealed.unseal_value(&wrong_secret);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CryptoError::DecryptionFailed(_)));
    }

    #[test]
    fn test_associated_data_authentication() {
        let effects = Effects::test();
        let device_secret: [u8; 32] = effects.random_bytes();
        let original_value = vec![1u8, 2, 3, 4, 5];
        let aad = b"important-metadata";

        let sealed = SealedData::seal_value(
            &original_value,
            &device_secret,
            "test-context",
            Some(aad),
            &effects,
        )
        .unwrap();

        // Correct AAD should work
        let unsealed: Vec<u8> = sealed.unseal_value(&device_secret).unwrap();
        assert_eq!(original_value, unsealed);

        // Tampering with AAD should fail
        let mut tampered_sealed = sealed.clone();
        tampered_sealed.associated_data = Some(b"tampered-metadata".to_vec());
        
        let result: Result<Vec<u8>> = tampered_sealed.unseal_value(&device_secret);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CryptoError::DecryptionFailed(_)));
    }
}

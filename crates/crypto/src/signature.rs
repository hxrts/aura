//! Digital signature abstractions for Ed25519 operations
//!
//! Provides unified interfaces for Ed25519 signing and verification used throughout Aura.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::error::CryptoError;

/// Ed25519 signing key
pub type Ed25519SigningKey = SigningKey;

/// Ed25519 verifying key (public key)
pub type Ed25519VerifyingKey = VerifyingKey;

/// Ed25519 signature with Default implementation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519Signature(pub Signature);

impl Default for Ed25519Signature {
    fn default() -> Self {
        Ed25519Signature(Signature::from_bytes(&[0u8; 64]))
    }
}

impl From<Signature> for Ed25519Signature {
    fn from(sig: Signature) -> Self {
        Ed25519Signature(sig)
    }
}

impl From<Ed25519Signature> for Signature {
    fn from(sig: Ed25519Signature) -> Self {
        sig.0
    }
}

impl std::ops::Deref for Ed25519Signature {
    type Target = Signature;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Ed25519Signature {
    /// Create signature from byte slice
    pub fn from_slice(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != 64 {
            return Err(CryptoError::invalid_signature(format!(
                "Invalid signature length: expected 64 bytes, got {}",
                bytes.len()
            )));
        }

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(bytes);
        Ok(Ed25519Signature(Signature::from_bytes(&sig_bytes)))
    }

    /// Create signature from byte array
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        Ed25519Signature(Signature::from_bytes(bytes))
    }

    /// Get signature as bytes
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0.to_bytes()
    }
}

/// Generate a new Ed25519 signing key
///
/// Note: This function uses OsRng directly and should only be used in tests.
/// Production code should use the Effects system for deterministic testing.
#[allow(clippy::disallowed_types)]
pub fn generate_ed25519_key() -> Ed25519SigningKey {
    SigningKey::generate(&mut OsRng)
}

/// Generate an Ed25519 signing key from seed bytes
pub fn ed25519_key_from_bytes(bytes: &[u8; 32]) -> Result<Ed25519SigningKey, CryptoError> {
    Ok(SigningKey::from_bytes(bytes))
}

/// Get the verifying key from a signing key
pub fn ed25519_verifying_key(signing_key: &Ed25519SigningKey) -> Ed25519VerifyingKey {
    signing_key.verifying_key()
}

/// Sign data with Ed25519
pub fn ed25519_sign(signing_key: &Ed25519SigningKey, data: &[u8]) -> Ed25519Signature {
    Ed25519Signature(signing_key.sign(data))
}

/// Verify an Ed25519 signature
pub fn ed25519_verify(
    verifying_key: &Ed25519VerifyingKey,
    data: &[u8],
    signature: &Ed25519Signature,
) -> Result<(), CryptoError> {
    verifying_key
        .verify(data, &signature.0)
        .map_err(|e| CryptoError::data_corruption_detected(e.to_string()))
}

/// Convert Ed25519 verifying key to bytes
pub fn ed25519_verifying_key_to_bytes(key: &Ed25519VerifyingKey) -> [u8; 32] {
    key.to_bytes()
}

/// Convert Ed25519 signature to bytes
pub fn ed25519_signature_to_bytes(signature: &Ed25519Signature) -> [u8; 64] {
    signature.0.to_bytes()
}

/// Create Ed25519 verifying key from bytes
pub fn ed25519_verifying_key_from_bytes(
    bytes: &[u8; 32],
) -> Result<Ed25519VerifyingKey, CryptoError> {
    VerifyingKey::from_bytes(bytes)
        .map_err(|e| CryptoError::data_corruption_detected(e.to_string()))
}

/// Create Ed25519 signature from bytes
pub fn ed25519_signature_from_bytes(bytes: &[u8; 64]) -> Result<Ed25519Signature, CryptoError> {
    Ok(Ed25519Signature(Signature::from_bytes(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ed25519_key_generation() {
        let signing_key = generate_ed25519_key();
        let verifying_key = ed25519_verifying_key(&signing_key);

        // Keys should be different each time
        let signing_key2 = generate_ed25519_key();
        assert_ne!(signing_key.to_bytes(), signing_key2.to_bytes());

        // Verifying key should be derivable
        let verifying_key2 = ed25519_verifying_key(&signing_key2);
        assert_ne!(verifying_key.to_bytes(), verifying_key2.to_bytes());
    }

    #[test]
    fn test_ed25519_sign_verify() {
        let signing_key = generate_ed25519_key();
        let verifying_key = ed25519_verifying_key(&signing_key);

        let data = b"hello world";
        let signature = ed25519_sign(&signing_key, data);

        // Verification should succeed
        assert!(ed25519_verify(&verifying_key, data, &signature).is_ok());

        // Verification with wrong data should fail
        let wrong_data = b"wrong data";
        assert!(ed25519_verify(&verifying_key, wrong_data, &signature).is_err());
    }

    #[test]
    fn test_ed25519_serialization() {
        let signing_key = generate_ed25519_key();
        let verifying_key = ed25519_verifying_key(&signing_key);

        let data = b"test data";
        let signature = ed25519_sign(&signing_key, data);

        // Convert to bytes and back
        let key_bytes = ed25519_verifying_key_to_bytes(&verifying_key);
        let sig_bytes = ed25519_signature_to_bytes(&signature);

        let restored_key = ed25519_verifying_key_from_bytes(&key_bytes).unwrap();
        let restored_sig = ed25519_signature_from_bytes(&sig_bytes).unwrap();

        // Verification should still work
        assert!(ed25519_verify(&restored_key, data, &restored_sig).is_ok());
    }
}

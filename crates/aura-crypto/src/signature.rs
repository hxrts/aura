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

// ========== Threshold Signatures ==========

/// Threshold signature produced by M-of-N participants
///
/// This represents a signature created using FROST threshold cryptography,
/// where multiple participants collaborate to create a single signature
/// without any individual participant having access to the full private key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// The aggregated Ed25519 signature
    #[serde(with = "crate::serde::signature_serde")]
    pub signature: Ed25519Signature,
    /// Participant IDs who contributed to this signature
    pub signers: Vec<u16>, // Using u16 to avoid circular dependency with ParticipantId
}

impl ThresholdSignature {
    /// Create a placeholder threshold signature for testing purposes
    ///
    /// **WARNING: This creates a fake signature with no cryptographic security.**
    /// Use only for testing. For production, use FROST aggregation.
    pub fn placeholder() -> Self {
        Self {
            signature: Ed25519Signature::default(),
            signers: vec![1, 2],
        }
    }

    /// Create a real threshold signature from FROST aggregation
    ///
    /// This method should be used in production to create actual cryptographically
    /// secure threshold signatures from FROST protocol outputs.
    ///
    /// # Arguments
    ///
    /// * `signature` - The aggregated ed25519 signature from FROST
    /// * `signers` - The participant IDs who contributed to the signature
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // After running FROST threshold signing protocol:
    /// let aggregated_signature = frost_session.aggregate_signature()?;
    /// let signer_ids = frost_session.get_signers();
    /// let threshold_sig = ThresholdSignature::from_frost(aggregated_signature, signer_ids);
    /// ```
    pub fn from_frost(signature: Ed25519Signature, signers: Vec<u16>) -> Self {
        Self { signature, signers }
    }

    /// Verify the threshold signature against provided data and public key
    ///
    /// This uses standard ed25519 verification on the aggregated signature.
    pub fn verify(&self, data: &[u8], public_key: &Ed25519VerifyingKey) -> bool {
        ed25519_verify(public_key, data, &self.signature).is_ok()
    }

    /// Get the number of signers
    pub fn signer_count(&self) -> usize {
        self.signers.len()
    }

    /// Get the signature bytes
    pub fn signature_bytes(&self) -> [u8; 64] {
        ed25519_signature_to_bytes(&self.signature)
    }

    /// Get the list of signer IDs
    pub fn signer_ids(&self) -> &[u16] {
        &self.signers
    }
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

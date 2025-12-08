//! Single-Signer Ed25519 Key Management
//!
//! For 1-of-1 scenarios, use standard Ed25519 instead of FROST threshold signatures.
//! This module provides key types that are compatible with the ThresholdSignature
//! verification flow while avoiding the min_signers >= 2 constraint of FROST.
//!
//! ## Why Not FROST for 1-of-1?
//!
//! FROST (Flexible Round-Optimized Schnorr Threshold) requires at least 2 signers
//! because threshold signatures mathematically need multiple parties. For single-device
//! accounts, standard Ed25519 is:
//! - Simpler (no DKG, no nonce coordination)
//! - Faster (single sign operation)
//! - Cryptographically equivalent (same curve: Ed25519/ristretto255)
//!
//! ## Usage
//!
//! ```ignore
//! // Generate keys via CryptoEffects
//! let result = crypto.generate_signing_keys(1, 1).await?;
//! assert_eq!(result.mode, SigningMode::SingleSigner);
//!
//! // Sign directly
//! let sig = crypto.sign_with_key(&message, &key_package, SigningMode::SingleSigner).await?;
//!
//! // Verify
//! let valid = crypto.verify_signature(&message, &sig, &pubkey, SigningMode::SingleSigner).await?;
//! ```

use serde::{Deserialize, Serialize};

/// Indicates whether keys are single-signer Ed25519 or FROST threshold.
///
/// This enum is stored alongside key material to ensure the correct
/// signing/verification algorithm is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SigningMode {
    /// Standard Ed25519 for 1-of-1 configurations.
    ///
    /// Uses direct Ed25519 signing without FROST protocol overhead.
    /// Key material is a standard Ed25519 keypair.
    SingleSigner,

    /// FROST threshold signatures for m-of-n where m >= 2.
    ///
    /// Requires the full FROST protocol flow:
    /// 1. Nonce commitment exchange
    /// 2. Partial signature generation
    /// 3. Signature aggregation
    Threshold,
}

impl SigningMode {
    /// Returns true if this is a single-signer (1-of-1) configuration.
    pub fn is_single_signer(&self) -> bool {
        matches!(self, SigningMode::SingleSigner)
    }

    /// Returns true if this is a threshold (m-of-n, m >= 2) configuration.
    pub fn is_threshold(&self) -> bool {
        matches!(self, SigningMode::Threshold)
    }
}

impl std::fmt::Display for SigningMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigningMode::SingleSigner => write!(f, "single-signer"),
            SigningMode::Threshold => write!(f, "threshold"),
        }
    }
}

/// Single-signer key package containing Ed25519 keypair.
///
/// This is the 1-of-1 equivalent of a FROST KeyPackage. It contains both
/// the signing key (private) and verifying key (public) for a single authority.
///
/// ## Security
///
/// The signing key is sensitive material and should be:
/// - Stored via `SecureStorageEffects`
/// - Zeroized when no longer needed
/// - Never logged or transmitted
#[derive(Clone, Serialize, Deserialize)]
pub struct SingleSignerKeyPackage {
    /// 32-byte Ed25519 signing key (private).
    ///
    /// This is the secret scalar used for signing operations.
    signing_key: Vec<u8>,

    /// 32-byte Ed25519 verifying key (public).
    ///
    /// This is the public point derived from the signing key.
    verifying_key: Vec<u8>,
}

impl SingleSignerKeyPackage {
    /// Create a new single-signer key package from raw key bytes.
    ///
    /// # Arguments
    ///
    /// * `signing_key` - 32-byte Ed25519 signing key
    /// * `verifying_key` - 32-byte Ed25519 verifying key
    ///
    /// # Panics
    ///
    /// Panics if key lengths are incorrect. Use `try_new` for fallible construction.
    pub fn new(signing_key: Vec<u8>, verifying_key: Vec<u8>) -> Self {
        assert_eq!(signing_key.len(), 32, "Signing key must be 32 bytes");
        assert_eq!(verifying_key.len(), 32, "Verifying key must be 32 bytes");
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Try to create a new single-signer key package from raw key bytes.
    ///
    /// Returns `None` if key lengths are incorrect.
    pub fn try_new(signing_key: Vec<u8>, verifying_key: Vec<u8>) -> Option<Self> {
        if signing_key.len() == 32 && verifying_key.len() == 32 {
            Some(Self {
                signing_key,
                verifying_key,
            })
        } else {
            None
        }
    }

    /// Get the signing key bytes (private).
    ///
    /// # Security
    ///
    /// Handle with care - this is secret key material.
    pub fn signing_key(&self) -> &[u8] {
        &self.signing_key
    }

    /// Get the verifying key bytes (public).
    pub fn verifying_key(&self) -> &[u8] {
        &self.verifying_key
    }

    /// Extract the public key package from this key package.
    pub fn public_key_package(&self) -> SingleSignerPublicKeyPackage {
        SingleSignerPublicKeyPackage {
            verifying_key: self.verifying_key.clone(),
        }
    }

    /// Serialize to bytes using bincode.
    ///
    /// # Panics
    /// This should never panic as the type contains only fixed-size vectors.
    #[allow(clippy::expect_used)] // Serialization of Vec<u8> fields cannot fail
    pub fn to_bytes(&self) -> Vec<u8> {
        // Use bincode for efficient binary serialization
        bincode::serialize(self).expect("SingleSignerKeyPackage serialization should not fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes)
            .map_err(|e| format!("Failed to deserialize SingleSignerKeyPackage: {}", e))
    }
}

impl std::fmt::Debug for SingleSignerKeyPackage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't expose signing key in debug output
        f.debug_struct("SingleSignerKeyPackage")
            .field("signing_key", &"[REDACTED]")
            .field("verifying_key", &hex::encode(&self.verifying_key))
            .finish()
    }
}

impl Drop for SingleSignerKeyPackage {
    fn drop(&mut self) {
        // Zeroize signing key on drop
        use zeroize::Zeroize;
        self.signing_key.zeroize();
    }
}

/// Single-signer public key package for verification.
///
/// This is the 1-of-1 equivalent of a FROST PublicKeyPackage. It contains
/// only the verifying key needed to verify signatures.
///
/// This type is safe to share publicly and store without encryption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleSignerPublicKeyPackage {
    /// 32-byte Ed25519 verifying key.
    pub verifying_key: Vec<u8>,
}

impl SingleSignerPublicKeyPackage {
    /// Create a new public key package from a verifying key.
    ///
    /// # Panics
    ///
    /// Panics if the verifying key is not 32 bytes.
    pub fn new(verifying_key: Vec<u8>) -> Self {
        assert_eq!(verifying_key.len(), 32, "Verifying key must be 32 bytes");
        Self { verifying_key }
    }

    /// Try to create a new public key package from a verifying key.
    ///
    /// Returns `None` if the verifying key is not 32 bytes.
    pub fn try_new(verifying_key: Vec<u8>) -> Option<Self> {
        if verifying_key.len() == 32 {
            Some(Self { verifying_key })
        } else {
            None
        }
    }

    /// Get the verifying key bytes.
    pub fn verifying_key(&self) -> &[u8] {
        &self.verifying_key
    }

    /// Serialize to bytes using bincode.
    ///
    /// # Panics
    /// This should never panic as the type contains only a Vec<u8> field.
    #[allow(clippy::expect_used)] // Serialization of Vec<u8> cannot fail
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self)
            .expect("SingleSignerPublicKeyPackage serialization should not fail")
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes)
            .map_err(|e| format!("Failed to deserialize SingleSignerPublicKeyPackage: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signing_mode_display() {
        assert_eq!(SigningMode::SingleSigner.to_string(), "single-signer");
        assert_eq!(SigningMode::Threshold.to_string(), "threshold");
    }

    #[test]
    fn test_signing_mode_predicates() {
        assert!(SigningMode::SingleSigner.is_single_signer());
        assert!(!SigningMode::SingleSigner.is_threshold());
        assert!(!SigningMode::Threshold.is_single_signer());
        assert!(SigningMode::Threshold.is_threshold());
    }

    #[test]
    fn test_key_package_creation() {
        let signing_key = vec![1u8; 32];
        let verifying_key = vec![2u8; 32];

        let pkg = SingleSignerKeyPackage::new(signing_key.clone(), verifying_key.clone());
        assert_eq!(pkg.signing_key(), &signing_key[..]);
        assert_eq!(pkg.verifying_key(), &verifying_key[..]);
    }

    #[test]
    fn test_key_package_try_new() {
        // Valid lengths
        assert!(SingleSignerKeyPackage::try_new(vec![0u8; 32], vec![0u8; 32]).is_some());

        // Invalid lengths
        assert!(SingleSignerKeyPackage::try_new(vec![0u8; 31], vec![0u8; 32]).is_none());
        assert!(SingleSignerKeyPackage::try_new(vec![0u8; 32], vec![0u8; 33]).is_none());
    }

    #[test]
    fn test_key_package_serialization_roundtrip() {
        let signing_key = vec![1u8; 32];
        let verifying_key = vec![2u8; 32];
        let pkg = SingleSignerKeyPackage::new(signing_key.clone(), verifying_key.clone());

        let bytes = pkg.to_bytes();
        let restored = SingleSignerKeyPackage::from_bytes(&bytes).unwrap();

        assert_eq!(restored.signing_key(), &signing_key[..]);
        assert_eq!(restored.verifying_key(), &verifying_key[..]);
    }

    #[test]
    fn test_public_key_package_creation() {
        let verifying_key = vec![3u8; 32];
        let pkg = SingleSignerPublicKeyPackage::new(verifying_key.clone());
        assert_eq!(pkg.verifying_key(), &verifying_key[..]);
    }

    #[test]
    fn test_public_key_package_serialization_roundtrip() {
        let verifying_key = vec![4u8; 32];
        let pkg = SingleSignerPublicKeyPackage::new(verifying_key.clone());

        let bytes = pkg.to_bytes();
        let restored = SingleSignerPublicKeyPackage::from_bytes(&bytes).unwrap();

        assert_eq!(restored.verifying_key(), &verifying_key[..]);
    }

    #[test]
    fn test_extract_public_key_package() {
        let signing_key = vec![5u8; 32];
        let verifying_key = vec![6u8; 32];
        let key_pkg = SingleSignerKeyPackage::new(signing_key, verifying_key.clone());

        let pub_pkg = key_pkg.public_key_package();
        assert_eq!(pub_pkg.verifying_key(), &verifying_key[..]);
    }

    #[test]
    fn test_debug_redacts_signing_key() {
        let pkg = SingleSignerKeyPackage::new(vec![7u8; 32], vec![8u8; 32]);
        let debug_str = format!("{:?}", pkg);
        assert!(debug_str.contains("[REDACTED]"));
        assert!(!debug_str.contains("07070707")); // Signing key bytes should not appear
    }
}

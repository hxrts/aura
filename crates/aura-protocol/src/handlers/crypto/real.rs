//! Real cryptographic handler using actual crypto operations
//!
//! Provides secure cryptographic operations for production use.

use crate::effects::{CryptoEffects, CryptoError};
use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use rand::RngCore;

/// Real cryptographic handler for production use
pub struct RealCryptoHandler {
    // Note: For thread safety, we use thread_rng() directly in methods rather than storing RNG
}

impl RealCryptoHandler {
    /// Create a new real crypto handler
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for RealCryptoHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CryptoEffects for RealCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        // Note: This is not thread-safe, but for simplicity we'll use thread_rng
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        use rand::Rng;
        rand::thread_rng().gen_range(range)
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()
    }

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().into()
    }

    async fn ed25519_sign(&self, data: &[u8], key: &SigningKey) -> Result<Signature, CryptoError> {
        use ed25519_dalek::Signer;
        Ok(key.sign(data))
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &Signature,
        public_key: &VerifyingKey,
    ) -> Result<bool, CryptoError> {
        match public_key.verify_strict(data, signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError> {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        Ok((signing_key, verifying_key))
    }

    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey {
        private_key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        if a.len() != b.len() {
            return false;
        }
        a.ct_eq(b).into()
    }

    fn secure_zero(&self, data: &mut [u8]) {
        use zeroize::Zeroize;
        data.zeroize();
    }
}
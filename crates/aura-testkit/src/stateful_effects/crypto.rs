//! Mock cryptographic effect handlers for testing
//!
//! This module contains the stateful MockCryptoHandler that was moved from aura-effects
//! to fix architectural violations. The handler uses Arc<Mutex<>> for deterministic
//! behavior in tests.

use async_trait::async_trait;
// Unused legacy imports - keeping for potential future use
// use aura_core::crypto::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};
use aura_core::crypto::single_signer::{
    SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
};
use aura_core::effects::crypto::{
    FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext, SigningKeyGenResult,
};
use aura_core::effects::{CryptoEffects, CryptoError, RandomEffects};
use std::sync::{Arc, Mutex};

/// Mock crypto handler for deterministic testing
#[derive(Debug, Clone)]
pub struct MockCryptoHandler {
    seed: u64,
    counter: Arc<Mutex<u64>>,
}

impl Default for MockCryptoHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockCryptoHandler {
    /// Create a new mock crypto handler with default seed (42)
    pub fn new() -> Self {
        Self {
            seed: 42,
            counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new mock crypto handler with a specific seed
    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed,
            counter: Arc::new(Mutex::new(0)),
        }
    }
}

// RandomEffects implementation for MockCryptoHandler
#[async_trait]
impl RandomEffects for MockCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        let mut counter = self.counter.lock().unwrap();
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = ((self.seed.wrapping_add(*counter).wrapping_add(i as u64)) % 256) as u8;
            *counter = counter.wrapping_add(1);
        }
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.random_bytes(32).await;
        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes);
        result
    }

    async fn random_u64(&self) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter = counter.wrapping_add(1);
        self.seed.wrapping_add(*counter)
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        if min >= max {
            return min;
        }
        let range = max - min;
        let random = self.random_u64().await;
        min + (random % range)
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let bytes = self.random_bytes(16).await;
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes);
        uuid::Uuid::from_bytes(uuid_bytes)
    }
}

// CryptoEffects implementation for MockCryptoHandler
#[async_trait]
impl CryptoEffects for MockCryptoHandler {
    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - deterministic output based on seed and inputs
        // Incorporates all inputs to ensure different keys produce different outputs
        let mut state = self.seed;
        for byte in ikm {
            state = state.wrapping_mul(31).wrapping_add(*byte as u64);
        }
        for byte in salt {
            state = state.wrapping_mul(37).wrapping_add(*byte as u64);
        }
        for byte in info {
            state = state.wrapping_mul(41).wrapping_add(*byte as u64);
        }

        let mut result = vec![0u8; output_len];
        for byte in result.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (state >> 32) as u8;
        }
        Ok(result)
    }

    async fn derive_key(
        &self,
        _master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - deterministic key based on context
        let key_bytes = format!("{:?}", context).as_bytes().to_vec();
        Ok(key_bytes)
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        // Mock implementation
        let private_key = vec![self.seed as u8; 32];
        let public_key = vec![(self.seed >> 8) as u8; 32];
        Ok((private_key, public_key))
    }

    async fn ed25519_sign(
        &self,
        _message: &[u8],
        _private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation
        Ok(vec![self.seed as u8; 64])
    }

    async fn ed25519_verify(
        &self,
        _message: &[u8],
        signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Mock implementation - accept signatures that match our mock signature
        let expected = vec![self.seed as u8; 64];
        Ok(signature == expected.as_slice())
    }

    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        // Mock implementation
        let mut key_packages = Vec::new();
        for i in 0..max_signers {
            let key = vec![self.seed as u8 + i as u8; 32];
            key_packages.push(key);
        }
        let public_key_package = vec![(self.seed >> 16) as u8; 32];
        Ok(FrostKeyGenResult {
            key_packages,
            public_key_package,
        })
    }

    async fn frost_generate_nonces(&self, _key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![self.seed as u8; 64])
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        _nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        Ok(FrostSigningPackage {
            message: message.to_vec(),
            package: vec![self.seed as u8; 32],
            participants: participants.to_vec(),
            public_key_package: public_key_package.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        _package: &FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![self.seed as u8; 64])
    }

    async fn frost_aggregate_signatures(
        &self,
        _package: &FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![self.seed as u8; 64])
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Mock implementation
        let expected = vec![self.seed as u8; 64];
        Ok(signature == expected.as_slice())
    }

    async fn ed25519_public_key(&self, _private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![(self.seed >> 8) as u8; 32])
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - deterministic XOR incorporating key and nonce
        // This ensures different keys produce different ciphertext
        let mut key_state = self.seed;
        for byte in key {
            key_state = key_state.wrapping_mul(31).wrapping_add(*byte as u64);
        }
        for byte in nonce {
            key_state = key_state.wrapping_mul(37).wrapping_add(*byte as u64);
        }

        let mut result = plaintext.to_vec();
        let mut state = key_state;
        for byte in result.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte ^= (state >> 32) as u8;
        }
        Ok(result)
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // XOR is symmetric, so decrypt = encrypt with same key stream
        self.chacha20_encrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - use same key-sensitive XOR as chacha20
        self.chacha20_encrypt(plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - use same key-sensitive XOR as chacha20
        self.chacha20_decrypt(ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        // Mock implementation - generate new keys
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        if threshold == 1 && max_signers == 1 {
            // Single-signer: use Ed25519
            let (signing_key, verifying_key) = self.ed25519_generate_keypair().await?;
            let key_package = SingleSignerKeyPackage::new(signing_key, verifying_key.clone());
            let public_package = SingleSignerPublicKeyPackage::new(verifying_key);
            Ok(SigningKeyGenResult {
                key_packages: vec![key_package.to_bytes()],
                public_key_package: public_package.to_bytes(),
                mode: SigningMode::SingleSigner,
            })
        } else if threshold >= 2 {
            // Threshold: use FROST
            let frost_result = self.frost_generate_keys(threshold, max_signers).await?;
            Ok(SigningKeyGenResult {
                key_packages: frost_result.key_packages,
                public_key_package: frost_result.public_key_package,
                mode: SigningMode::Threshold,
            })
        } else {
            Err(CryptoError::invalid(format!(
                "Invalid signing configuration: threshold={}, max_signers={}. \
                 Use 1-of-1 for single-signer or threshold>=2 for multi-party.",
                threshold, max_signers
            )))
        }
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        match mode {
            SigningMode::SingleSigner => {
                let package = SingleSignerKeyPackage::from_bytes(key_package).map_err(|e| {
                    CryptoError::invalid(format!("Invalid single-signer key package: {}", e))
                })?;
                self.ed25519_sign(message, package.signing_key()).await
            }
            SigningMode::Threshold => Err(CryptoError::invalid(
                "sign_with_key() does not support Threshold mode. Use the full FROST protocol flow.",
            )),
        }
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        match mode {
            SigningMode::SingleSigner => {
                let package = SingleSignerPublicKeyPackage::from_bytes(public_key_package)
                    .map_err(|e| {
                        CryptoError::invalid(format!(
                            "Invalid single-signer public key package: {}",
                            e
                        ))
                    })?;
                self.ed25519_verify(message, signature, package.verifying_key())
                    .await
            }
            SigningMode::Threshold => {
                // Use FROST verification
                self.frost_verify(message, signature, public_key_package)
                    .await
            }
        }
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "ed25519".to_string(),
            "frost".to_string(),
            "aes-gcm".to_string(),
            "chacha20".to_string(),
            "hkdf".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut result = 0u8;
        for (x, y) in a.iter().zip(b.iter()) {
            result |= x ^ y;
        }
        result == 0
    }

    fn secure_zero(&self, data: &mut [u8]) {
        for byte in data.iter_mut() {
            *byte = 0;
        }
    }
}

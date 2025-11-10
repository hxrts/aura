//! Mock cryptographic handler for testing
//!
//! Provides predictable, deterministic crypto operations for unit tests.

use crate::effects::{CryptoEffects, CryptoError};
use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock cryptographic handler for testing
pub struct MockCryptoHandler {
    /// Deterministic seed for reproducible randomness
    seed: u64,
    /// Counter for generating predictable random values
    counter: Arc<Mutex<u64>>,
    /// Pre-configured responses for specific operations
    responses: Arc<Mutex<MockResponses>>,
}

#[derive(Default)]
struct MockResponses {
    /// Pre-configured hash values for specific inputs
    hashes: HashMap<Vec<u8>, [u8; 32]>,
    /// Pre-configured signatures for specific data
    signatures: HashMap<Vec<u8>, Vec<u8>>,
    /// Pre-configured verification results
    verifications: HashMap<(Vec<u8>, Vec<u8>), bool>, // (data, signature) -> result
}

impl MockCryptoHandler {
    /// Create a new mock crypto handler with deterministic behavior
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            counter: Arc::new(Mutex::new(0)),
            responses: Arc::new(Mutex::new(MockResponses::default())),
        }
    }

    /// Set a pre-configured hash result for specific input
    pub fn set_hash_result(&self, input: Vec<u8>, hash: [u8; 32]) {
        self.responses.lock().unwrap().hashes.insert(input, hash);
    }

    /// Set a pre-configured signature for specific data
    pub fn set_signature_result(&self, data: Vec<u8>, signature: Vec<u8>) {
        self.responses
            .lock()
            .unwrap()
            .signatures
            .insert(data, signature);
    }

    /// Set a pre-configured verification result
    pub fn set_verification_result(&self, data: Vec<u8>, signature: Vec<u8>, result: bool) {
        self.responses
            .lock()
            .unwrap()
            .verifications
            .insert((data, signature), result);
    }

    /// Generate deterministic "random" bytes based on seed and counter
    fn deterministic_bytes(&self, len: usize) -> Vec<u8> {
        let mut counter = self.counter.lock().unwrap();
        let mut bytes = Vec::with_capacity(len);

        for i in 0..len {
            // Simple deterministic pseudo-random generator
            let value = (self
                .seed
                .wrapping_mul(1103515245)
                .wrapping_add(*counter)
                .wrapping_add(i as u64))
                % 256;
            bytes.push(value as u8);
            *counter = counter.wrapping_add(1);
        }

        bytes
    }
}

impl Default for MockCryptoHandler {
    fn default() -> Self {
        Self::new(42) // Default deterministic seed
    }
}

// First implement RandomEffects
#[async_trait]
impl RandomEffects for MockCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.deterministic_bytes(len)
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.deterministic_bytes(32);
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        array
    }

    async fn random_u64(&self) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter = counter.wrapping_add(1);
        self.seed.wrapping_add(*counter)
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter = counter.wrapping_add(1);

        if min >= max {
            return min;
        }

        let range_size = max - min;
        // Deterministic value within range
        let value = (self.seed.wrapping_add(*counter)) % range_size;
        min + value
    }
}

// Then implement CryptoEffects (which inherits from RandomEffects)
#[async_trait]
impl CryptoEffects for MockCryptoHandler {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        // Check for pre-configured response
        if let Some(hash) = self.responses.lock().unwrap().hashes.get(data) {
            return *hash;
        }

        // Generate deterministic hash-like value using SHA256-like mixing
        let mut hash = [0u8; 32];
        for (i, &byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte.wrapping_add(i as u8);
        }

        // Mix with seed for determinism
        for (i, h) in hash.iter_mut().enumerate() {
            *h = h.wrapping_add((self.seed >> (i % 8)) as u8);
        }

        hash
    }

    async fn ed25519_sign(&self, data: &[u8], private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        // Check for pre-configured response
        if let Some(sig_bytes) = self.responses.lock().unwrap().signatures.get(data) {
            return Ok(sig_bytes.clone());
        }

        // Generate deterministic signature from data and key
        let mut combined = Vec::new();
        combined.extend_from_slice(data);
        combined.extend_from_slice(private_key);
        let sig_bytes = self.deterministic_bytes(64);

        Ok(sig_bytes)
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Check for pre-configured response
        if let Some(result) = self
            .responses
            .lock()
            .unwrap()
            .verifications
            .get(&(data.to_vec(), signature.to_vec()))
        {
            return Ok(*result);
        }

        // Default to successful verification for mock
        Ok(true)
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        // Generate deterministic keypair
        let private_key = self.deterministic_bytes(32);
        let public_key = self.deterministic_bytes(32); // TODO fix - Simplified - would derive from private in real impl

        Ok((private_key, public_key))
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        // Generate deterministic public key from private key
        let mut combined = Vec::new();
        combined.extend_from_slice(b"pubkey_derive");
        combined.extend_from_slice(private_key);
        let hash = self.hash(&combined).await;

        Ok(hash.to_vec())
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        // For mock, just use regular comparison (not actually constant-time)
        a == b
    }

    fn secure_zero(&self, data: &mut [u8]) {
        // For mock, just zero the data normally
        data.fill(0);
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock HKDF using deterministic approach
        let combined: Vec<u8> = ikm
            .iter()
            .chain(salt.iter())
            .chain(info.iter())
            .copied()
            .collect();

        let hash = self.hash(&combined).await;
        let mut output = vec![0u8; output_len];

        // Expand the hash to fill the output length
        for (i, byte) in output.iter_mut().enumerate() {
            *byte = hash[i % 32] ^ (i as u8);
        }

        Ok(output)
    }

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        // Mock HMAC using deterministic approach
        let mut combined = Vec::new();
        combined.extend_from_slice(key);
        combined.extend_from_slice(data);
        combined.extend_from_slice(b"HMAC");
        self.hash(&combined).await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock key derivation using deterministic approach
        let context_bytes = bincode::serialize(context).unwrap_or_default();
        self.hkdf_derive(master_key, &[], &context_bytes, 32).await
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Mock FROST key generation
        let mut key_shares = Vec::new();
        for i in 0..max_signers {
            let share = self.deterministic_bytes(64); // Mock key share
            key_shares.push(share);
        }
        Ok(key_shares)
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        // Mock FROST nonce generation
        Ok(self.deterministic_bytes(64))
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, CryptoError> {
        // Mock FROST signing package
        let package_bytes = self.deterministic_bytes(128); // Mock package data
        Ok(aura_core::effects::crypto::FrostSigningPackage {
            message: message.to_vec(),
            package: package_bytes,
            participants: participants.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock FROST partial signature
        let mut combined = Vec::new();
        combined.extend_from_slice(&signing_package.message);
        combined.extend_from_slice(key_share);
        combined.extend_from_slice(nonces);

        Ok(self.deterministic_bytes(64))
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock FROST signature aggregation
        let mut combined = Vec::new();
        combined.extend_from_slice(&signing_package.message);
        for share in signature_shares {
            combined.extend_from_slice(share);
        }

        Ok(self.deterministic_bytes(64))
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Mock FROST verification - always succeed for testing
        Ok(true)
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock ChaCha20 encryption - just XOR with deterministic stream
        let mut ciphertext = Vec::new();
        let mut keystream_seed = Vec::new();
        keystream_seed.extend_from_slice(key);
        keystream_seed.extend_from_slice(nonce);

        for (i, &byte) in plaintext.iter().enumerate() {
            let keystream_byte = (keystream_seed[(i % keystream_seed.len())] ^ (i as u8));
            ciphertext.push(byte ^ keystream_byte);
        }

        Ok(ciphertext)
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock ChaCha20 decryption - symmetric with encryption for XOR cipher
        self.chacha20_encrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock AES-GCM encryption - reuse ChaCha20 logic for simplicity
        self.chacha20_encrypt(plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock AES-GCM decryption - reuse ChaCha20 logic for simplicity
        self.chacha20_decrypt(ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Mock FROST key rotation
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
    }

    fn is_simulated(&self) -> bool {
        true // Mock handler is always simulated
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "mock_ed25519".to_string(),
            "mock_blake3".to_string(),
            "mock_frost".to_string(),
            "mock_chacha20".to_string(),
            "mock_aes_gcm".to_string(),
        ]
    }
}

//! Mock cryptographic handler for testing
//!
//! Provides predictable, deterministic crypto operations for unit tests.

use crate::effects::{CryptoEffects, CryptoError};
use async_trait::async_trait;
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

#[async_trait]
impl CryptoEffects for MockCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.deterministic_bytes(len)
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.deterministic_bytes(32);
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        array
    }

    async fn random_range(&self, range: std::ops::Range<u64>) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter = counter.wrapping_add(1);

        let range_size = range.end - range.start;
        if range_size == 0 {
            return range.start;
        }

        // Deterministic value within range
        let value = (self.seed.wrapping_add(*counter)) % range_size;
        range.start + value
    }

    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        // Check for pre-configured response
        if let Some(hash) = self.responses.lock().unwrap().hashes.get(data) {
            return *hash;
        }

        // Generate deterministic hash-like value
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

    async fn sha256_hash(&self, data: &[u8]) -> [u8; 32] {
        // For mock, just use the same logic as blake3_hash but with different mixing
        let mut hash = self.blake3_hash(data).await;

        // Add some differentiation from blake3
        for h in hash.iter_mut() {
            *h = h.wrapping_add(0x5A);
        }

        hash
    }

    async fn ed25519_sign(&self, data: &[u8], _key: &SigningKey) -> Result<Signature, CryptoError> {
        // Check for pre-configured response
        if let Some(sig_bytes) = self.responses.lock().unwrap().signatures.get(data) {
            if sig_bytes.len() == 64 {
                let mut sig_array = [0u8; 64];
                sig_array.copy_from_slice(sig_bytes);
                return Ok(Signature::from_bytes(&sig_array));
            }
        }

        // Generate deterministic signature
        let sig_bytes = self.deterministic_bytes(64);
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);

        Ok(Signature::from_bytes(&sig_array))
    }

    async fn ed25519_verify(
        &self,
        data: &[u8],
        signature: &Signature,
        _public_key: &VerifyingKey,
    ) -> Result<bool, CryptoError> {
        // Check for pre-configured response
        let sig_bytes = signature.to_bytes().to_vec();
        if let Some(result) = self
            .responses
            .lock()
            .unwrap()
            .verifications
            .get(&(data.to_vec(), sig_bytes))
        {
            return Ok(*result);
        }

        // Default to successful verification for mock
        Ok(true)
    }

    async fn ed25519_generate_keypair(&self) -> Result<(SigningKey, VerifyingKey), CryptoError> {
        // Generate deterministic keypair
        let key_bytes = self.deterministic_bytes(32);
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);

        let signing_key = SigningKey::from_bytes(&key_array);
        let verifying_key = signing_key.verifying_key();

        Ok((signing_key, verifying_key))
    }

    async fn ed25519_public_key(&self, private_key: &SigningKey) -> VerifyingKey {
        private_key.verifying_key()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        // For mock, just use regular comparison (not actually constant-time)
        a == b
    }

    fn secure_zero(&self, data: &mut [u8]) {
        // For mock, just zero the data normally
        data.fill(0);
    }
}

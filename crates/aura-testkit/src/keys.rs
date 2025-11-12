//! Key test helpers and utilities
//!
//! This module provides standardized helpers for creating and managing test keys
//! and cryptographic material across the Aura test suite.

use aura_core::DeviceId;
use aura_core::effects::RandomEffects;
use crate::Effects;
use ed25519_dalek::{SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

/// Key test fixture for consistent test key generation
#[derive(Debug, Clone)]
pub struct KeyTestFixture {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    key_id: String,
}

impl KeyTestFixture {
    /// Create a new key fixture with deterministic generation from a seed
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        let signing_key = SigningKey::from_bytes(seed);
        let verifying_key = signing_key.verifying_key();

        // Generate key ID from the public key
        let mut hasher = Sha256::new();
        hasher.update(verifying_key.as_bytes());
        let key_id = format!("key_{:x}", hasher.finalize());

        Self {
            signing_key,
            verifying_key,
            key_id,
        }
    }

    /// Create a key fixture from a seed string
    pub fn from_seed_string(seed: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(seed.as_bytes());
        let digest = hasher.finalize();

        let seed_bytes: [u8; 32] = digest[..32].try_into().unwrap_or([0u8; 32]);
        Self::from_seed(&seed_bytes)
    }

    /// Create a random key fixture
    pub async fn random() -> Self {
        // Use deterministic key generation from fixed seed
        let effects = Effects::for_test("key_fixture_random");
        let key_bytes = effects.random_bytes_32().await;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();

        let mut hasher = Sha256::new();
        hasher.update(verifying_key.as_bytes());
        let key_id = format!("key_{:x}", hasher.finalize());

        Self {
            signing_key,
            verifying_key,
            key_id,
        }
    }

    /// Get the signing key
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_key
    }

    /// Get the verifying key
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Get the key ID
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// Sign a message with this key
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature {
        use ed25519_dalek::Signer;
        self.signing_key.sign(message)
    }

    /// Verify a signature with this key's verifying key
    pub fn verify(&self, message: &[u8], signature: &ed25519_dalek::Signature) -> bool {
        self.verifying_key.verify_strict(message, signature).is_ok()
    }
}

/// Builder for creating multiple test keys with consistent configuration
#[derive(Debug)]
pub struct KeySetBuilder {
    count: usize,
    base_seed: Option<String>,
    key_type: KeyType,
}

/// Types of keys for different test scenarios
#[derive(Debug, Clone, Copy)]
pub enum KeyType {
    /// Standard Ed25519 keys
    Standard,
    /// Device-specific keys (seeded with device ID)
    DeviceSpecific,
    /// Threshold keys (for FROST-based protocols)
    Threshold,
}

impl KeySetBuilder {
    /// Create a new key set builder
    pub fn new(count: usize) -> Self {
        Self {
            count,
            base_seed: None,
            key_type: KeyType::Standard,
        }
    }

    /// Set a base seed for deterministic key generation
    pub fn with_seed(mut self, seed: String) -> Self {
        self.base_seed = Some(seed);
        self
    }

    /// Set the key type
    pub fn with_key_type(mut self, key_type: KeyType) -> Self {
        self.key_type = key_type;
        self
    }

    /// Build the set of keys
    pub fn build(self) -> Vec<KeyTestFixture> {
        (0..self.count)
            .map(|i| {
                let seed_str = if let Some(ref base) = self.base_seed {
                    format!("{}-{}", base, i)
                } else {
                    format!("key-seed-{}", i)
                };

                match self.key_type {
                    KeyType::Standard => KeyTestFixture::from_seed_string(&seed_str),
                    KeyType::DeviceSpecific => {
                        let enhanced_seed = format!("{}-device-specific", seed_str);
                        KeyTestFixture::from_seed_string(&enhanced_seed)
                    }
                    KeyType::Threshold => {
                        let enhanced_seed = format!("{}-threshold", seed_str);
                        KeyTestFixture::from_seed_string(&enhanced_seed)
                    }
                }
            })
            .collect()
    }
}

/// Common test key creation helpers
pub mod helpers {
    use super::*;

    /// Create a single test key with default configuration
    pub fn test_key() -> KeyTestFixture {
        KeyTestFixture::from_seed_string("test-key-default")
    }

    /// Create N test keys with sequential seeding
    pub fn test_keys(count: usize) -> Vec<KeyTestFixture> {
        KeySetBuilder::new(count).build()
    }

    /// Create test keys with a specific base seed
    pub fn test_keys_seeded(count: usize, base_seed: &str) -> Vec<KeyTestFixture> {
        KeySetBuilder::new(count)
            .with_seed(base_seed.to_string())
            .build()
    }

    /// Create a key pair for two-party tests
    pub fn test_key_pair() -> (KeyTestFixture, KeyTestFixture) {
        (
            KeyTestFixture::from_seed_string("key-pair-1"),
            KeyTestFixture::from_seed_string("key-pair-2"),
        )
    }

    /// Create three keys for three-party tests
    pub fn test_key_trio() -> (KeyTestFixture, KeyTestFixture, KeyTestFixture) {
        (
            KeyTestFixture::from_seed_string("key-trio-1"),
            KeyTestFixture::from_seed_string("key-trio-2"),
            KeyTestFixture::from_seed_string("key-trio-3"),
        )
    }

    /// Create device-specific keys (seeded with device ID)
    pub fn test_keys_for_device(count: usize, device_id: DeviceId) -> Vec<KeyTestFixture> {
        let base_seed = format!("device-{:?}", device_id);
        KeySetBuilder::new(count)
            .with_seed(base_seed)
            .with_key_type(KeyType::DeviceSpecific)
            .build()
    }

    /// Create threshold keys for FROST-based protocols
    pub fn test_threshold_keys(
        count: usize,
        threshold: usize,
        base_seed: &str,
    ) -> Vec<KeyTestFixture> {
        KeySetBuilder::new(count)
            .with_seed(format!("{}-threshold-{}", base_seed, threshold))
            .with_key_type(KeyType::Threshold)
            .build()
    }

    /// Create a test message and return it with a signature from the key
    pub fn test_signed_message(
        key: &KeyTestFixture,
        message: &str,
    ) -> (Vec<u8>, ed25519_dalek::Signature) {
        let msg_bytes = message.as_bytes().to_vec();
        let signature = key.sign(&msg_bytes);
        (msg_bytes, signature)
    }

    /// Verify signature consistency across multiple keys
    pub fn verify_key_set_validity(keys: &[KeyTestFixture]) -> bool {
        keys.iter().all(|key| {
            let message = b"test message";
            let signature = key.sign(message);
            key.verify(message, &signature)
        })
    }

    /// Get deterministic key count for common test scenarios
    pub fn key_count_for_scenario(scenario: &str) -> usize {
        match scenario {
            "pair" => 2,
            "trio" => 3,
            "threshold-2-3" => 3,
            "threshold-3-5" => 5,
            "distributed-4" => 4,
            _ => 1,
        }
    }

    /// Create FROST key shares for threshold signature testing
    ///
    /// Returns key shares and public key package for the given threshold and total.
    /// Uses the provided effects for deterministic key generation.
    pub async fn test_frost_key_shares(
        threshold: u16,
        total: u16,
        effects: &Effects,
    ) -> (
        std::collections::BTreeMap<frost_ed25519::Identifier, frost_ed25519::keys::KeyPackage>,
        frost_ed25519::keys::PublicKeyPackage,
    ) {
        use rand::CryptoRng;
        use std::collections::BTreeMap;

        // Use a deterministic RNG from effects - it implements CryptoRng for testing
        let mut rng = {
            // Create a seed from the effects
            let seed = effects.random_bytes(8).await;
            let seed_array: [u8; 8] = seed.try_into().unwrap();
            let seed_u64 = u64::from_le_bytes(seed_array);

            // Create a deterministic RNG that also implements CryptoRng
            struct TestRng {
                state: u64,
            }

            impl rand::RngCore for TestRng {
                fn next_u32(&mut self) -> u32 {
                    self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
                    (self.state / 65536) as u32
                }

                fn next_u64(&mut self) -> u64 {
                    let high = self.next_u32() as u64;
                    let low = self.next_u32() as u64;
                    (high << 32) | low
                }

                fn fill_bytes(&mut self, dest: &mut [u8]) {
                    for byte in dest.iter_mut() {
                        *byte = (self.next_u32() % 256) as u8;
                    }
                }

                fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
                    self.fill_bytes(dest);
                    Ok(())
                }
            }

            impl CryptoRng for TestRng {}

            TestRng { state: seed_u64 }
        };

        // Generate key shares using FROST DKG
        let (secret_shares, pubkey_package) = frost_ed25519::keys::generate_with_dealer(
            total,
            threshold,
            frost_ed25519::keys::IdentifierList::Default,
            &mut rng,
        )
        .expect("Failed to generate FROST keys");

        // Convert secret shares to key packages
        let key_packages: BTreeMap<_, _> = secret_shares
            .into_iter()
            .map(|(id, secret_share)| {
                let key_package = frost_ed25519::keys::KeyPackage::try_from(secret_share)
                    .expect("Failed to convert secret share to key package");
                (id, key_package)
            })
            .collect();

        (key_packages, pubkey_package)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_fixture_creation() {
        let key = KeyTestFixture::from_seed_string("test-seed");
        assert!(!key.key_id().is_empty());
    }

    #[test]
    fn test_key_signing_and_verification() {
        let key = KeyTestFixture::from_seed_string("test-seed");
        let message = b"test message";
        let signature = key.sign(message);
        assert!(key.verify(message, &signature));
    }

    #[test]
    fn test_key_set_builder() {
        let keys = KeySetBuilder::new(3).build();
        assert_eq!(keys.len(), 3);
        assert!(helpers::verify_key_set_validity(&keys));
    }

    #[test]
    fn test_seeded_key_generation_deterministic() {
        let keys1 = helpers::test_keys_seeded(3, "base-seed");
        let keys2 = helpers::test_keys_seeded(3, "base-seed");

        for (k1, k2) in keys1.iter().zip(keys2.iter()) {
            assert_eq!(k1.verifying_key().as_bytes(), k2.verifying_key().as_bytes());
        }
    }

    #[test]
    fn test_key_helpers() {
        let (k1, k2) = helpers::test_key_pair();
        assert_ne!(k1.verifying_key().as_bytes(), k2.verifying_key().as_bytes());

        let (k1, k2, k3) = helpers::test_key_trio();
        assert_ne!(k1.verifying_key().as_bytes(), k2.verifying_key().as_bytes());
        assert_ne!(k2.verifying_key().as_bytes(), k3.verifying_key().as_bytes());
    }

    #[test]
    fn test_signed_message() {
        let key = KeyTestFixture::from_seed_string("test-key");
        let (msg, sig) = helpers::test_signed_message(&key, "test message");
        assert!(key.verify(&msg, &sig));
    }
}

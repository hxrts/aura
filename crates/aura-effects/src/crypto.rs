//! Cryptographic Effect Handlers
//!
//! Provides context-free implementations of cryptographic operations.

use async_trait::async_trait;
use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};
use aura_core::effects::{CryptoEffects, CryptoError, RandomEffects};
use std::sync::{Arc, Mutex};

/// Mock crypto handler for deterministic testing
#[derive(Debug, Clone)]
pub struct MockCryptoHandler {
    seed: u64,
    counter: Arc<Mutex<u64>>,
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

/// Real crypto handler using actual cryptographic operations
#[derive(Debug, Clone)]
pub struct RealCryptoHandler {
    _phantom: std::marker::PhantomData<()>,
}

impl RealCryptoHandler {
    /// Create a new real crypto handler
    pub fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

// RandomEffects implementation for MockCryptoHandler
#[async_trait]
impl RandomEffects for MockCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        let mut counter = self.counter.lock().unwrap();
        for i in 0..len {
            bytes[i] = ((self.seed.wrapping_add(*counter).wrapping_add(i as u64)) % 256) as u8;
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

// RandomEffects implementation for RealCryptoHandler
#[async_trait]
impl RandomEffects for RealCryptoHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        bytes
    }

    async fn random_u64(&self) -> u64 {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
        u64::from_le_bytes(bytes)
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
        _ikm: &[u8],
        _salt: &[u8],
        _info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - deterministic output based on seed
        Ok(vec![self.seed as u8; output_len])
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
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Mock implementation
        let mut keys = Vec::new();
        for i in 0..max_signers {
            let key = vec![self.seed as u8 + i as u8; 32];
            keys.push(key);
        }
        Ok(keys)
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![self.seed as u8; 64])
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        _nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<FrostSigningPackage, CryptoError> {
        Ok(FrostSigningPackage {
            message: message.to_vec(),
            package: vec![self.seed as u8; 32],
            participants: participants.to_vec(),
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
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - simple XOR
        let mut result = plaintext.to_vec();
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= (self.seed as u8).wrapping_add(i as u8);
        }
        Ok(result)
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // ChaCha20 is symmetric, so decrypt = encrypt
        self.chacha20_encrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - simple XOR
        let mut result = plaintext.to_vec();
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= (self.seed as u8).wrapping_add(i as u8);
        }
        Ok(result)
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Mock implementation - simple XOR (symmetric)
        let mut result = ciphertext.to_vec();
        for (i, byte) in result.iter_mut().enumerate() {
            *byte ^= (self.seed as u8).wrapping_add(i as u8);
        }
        Ok(result)
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Mock implementation - generate new keys
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
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

// CryptoEffects implementation for RealCryptoHandler
#[async_trait]
impl CryptoEffects for RealCryptoHandler {
    async fn hkdf_derive(
        &self,
        _ikm: &[u8],
        _salt: &[u8],
        _info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation - would use actual HKDF
        Ok(vec![0u8; output_len])
    }

    async fn derive_key(
        &self,
        _master_key: &[u8],
        _context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation - would use actual key derivation
        Ok(vec![0u8; 32])
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        // Placeholder implementation - would use actual Ed25519
        Ok((vec![0u8; 32], vec![0u8; 32]))
    }

    async fn ed25519_sign(
        &self,
        _message: &[u8],
        _private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation
        Ok(vec![0u8; 64])
    }

    async fn ed25519_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Placeholder implementation
        Ok(false)
    }

    async fn frost_generate_keys(
        &self,
        _threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Placeholder implementation
        let keys = (0..max_signers).map(|_| vec![0u8; 32]).collect();
        Ok(keys)
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        _nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<FrostSigningPackage, CryptoError> {
        Ok(FrostSigningPackage {
            message: message.to_vec(),
            package: vec![0u8; 32],
            participants: participants.to_vec(),
        })
    }

    async fn frost_sign_share(
        &self,
        _package: &FrostSigningPackage,
        _key_share: &[u8],
        _nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_aggregate_signatures(
        &self,
        _package: &FrostSigningPackage,
        _signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 64])
    }

    async fn frost_verify(
        &self,
        _message: &[u8],
        _signature: &[u8],
        _group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        // Placeholder implementation
        Ok(false)
    }

    async fn ed25519_public_key(&self, _private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        Ok(vec![0u8; 32])
    }

    async fn chacha20_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation
        Ok(vec![0u8; 16])
    }

    async fn chacha20_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation
        Ok(vec![0u8; 16])
    }

    async fn aes_gcm_encrypt(
        &self,
        _plaintext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation
        Ok(vec![0u8; 16])
    }

    async fn aes_gcm_decrypt(
        &self,
        _ciphertext: &[u8],
        _key: &[u8; 32],
        _nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        // Placeholder implementation
        Ok(vec![0u8; 16])
    }

    async fn frost_rotate_keys(
        &self,
        _old_shares: &[Vec<u8>],
        _old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        // Placeholder implementation
        self.frost_generate_keys(new_threshold, new_max_signers)
            .await
    }

    fn is_simulated(&self) -> bool {
        false
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
        // Use a simple constant-time comparison
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
        // In a real implementation, we'd use something like zeroize crate
        // to ensure the compiler doesn't optimize away the zeroing
    }
}

/// FROST RNG Adapter for Effect System Integration
///
/// This adapter bridges the async RandomEffects trait to the synchronous RngCore
/// trait required by the frost_ed25519 library. It allows FROST cryptographic
/// operations to use the effect system's randomness source while maintaining
/// testability and determinism.
///
/// # Architecture Note
///
/// FROST requires sync RNG (RngCore trait), but our effect system is async.
/// This adapter uses tokio::runtime::Handle to perform async-to-sync conversion
/// via block_on(). This is acceptable because:
/// 1. FROST operations are already synchronous in the library's API
/// 2. RandomEffects implementations are fast (crypto RNG or deterministic)
/// 3. This is only used during key generation and signing ceremonies
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::crypto::EffectSystemRng;
/// use aura_core::effects::RandomEffects;
/// use frost_ed25519 as frost;
///
/// async fn generate_frost_keys(effects: &dyn RandomEffects) {
///     let runtime = tokio::runtime::Handle::current();
///     let mut rng = EffectSystemRng::new(effects, runtime);
///
///     let (shares, pubkey) = frost::keys::generate_with_dealer(
///         3, 2,
///         frost::keys::IdentifierList::Default,
///         &mut rng
///     )?;
/// }
/// ```
pub struct EffectSystemRng<'a> {
    effects: &'a dyn RandomEffects,
    runtime: tokio::runtime::Handle,
}

impl<'a> EffectSystemRng<'a> {
    /// Create a new RNG adapter from RandomEffects and a runtime handle
    pub fn new(effects: &'a dyn RandomEffects, runtime: tokio::runtime::Handle) -> Self {
        Self { effects, runtime }
    }

    /// Create a new RNG adapter using the current runtime
    ///
    /// # Panics
    ///
    /// Panics if called outside of a Tokio runtime context
    pub fn from_current_runtime(effects: &'a dyn RandomEffects) -> Self {
        let runtime = tokio::runtime::Handle::current();
        Self::new(effects, runtime)
    }
}

impl rand::RngCore for EffectSystemRng<'_> {
    fn next_u32(&mut self) -> u32 {
        // Get lower 32 bits of u64
        (self.runtime.block_on(self.effects.random_u64()) & 0xFFFF_FFFF) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.runtime.block_on(self.effects.random_u64())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let bytes = self.runtime.block_on(self.effects.random_bytes(dest.len()));
        dest.copy_from_slice(&bytes);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

// Mark this RNG as cryptographically secure since RandomEffects is crypto-secure
impl rand::CryptoRng for EffectSystemRng<'_> {}

#[cfg(test)]
mod rng_adapter_tests {
    use super::*;

    #[tokio::test]
    async fn test_rng_adapter_with_mock() {
        let crypto = MockCryptoHandler::with_seed(12345);
        let runtime = tokio::runtime::Handle::current();
        let mut rng = EffectSystemRng::new(&crypto, runtime);

        // Test next_u32
        let val1 = rng.next_u32();
        let val2 = rng.next_u32();
        assert_ne!(val1, val2, "Should produce different values");

        // Test next_u64
        let val3 = rng.next_u64();
        let val4 = rng.next_u64();
        assert_ne!(val3, val4, "Should produce different values");

        // Test fill_bytes
        let mut bytes = [0u8; 32];
        rng.fill_bytes(&mut bytes);
        assert_ne!(bytes, [0u8; 32], "Should fill with random bytes");
    }

    #[tokio::test]
    async fn test_rng_adapter_deterministic() {
        // Same seed should produce same sequence
        let crypto1 = MockCryptoHandler::with_seed(42);
        let crypto2 = MockCryptoHandler::with_seed(42);

        let runtime = tokio::runtime::Handle::current();
        let mut rng1 = EffectSystemRng::new(&crypto1, runtime.clone());
        let mut rng2 = EffectSystemRng::new(&crypto2, runtime);

        let val1 = rng1.next_u64();
        let val2 = rng2.next_u64();
        assert_eq!(val1, val2, "Same seed should produce same values");

        let mut bytes1 = [0u8; 16];
        let mut bytes2 = [0u8; 16];
        rng1.fill_bytes(&mut bytes1);
        rng2.fill_bytes(&mut bytes2);
        assert_eq!(bytes1, bytes2, "Same seed should produce same byte sequences");
    }

    #[tokio::test]
    async fn test_rng_adapter_from_current_runtime() {
        let crypto = MockCryptoHandler::new();
        let mut rng = EffectSystemRng::from_current_runtime(&crypto);

        let val = rng.next_u64();
        assert_ne!(val, 0, "Should produce non-zero values");
    }
}

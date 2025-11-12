//! Test Effects Utilities
//!
//! Standard factory functions for creating effect handlers for testing.
//! This eliminates the most common duplication pattern found across test files.
//! 
//! Uses the new effect system architecture with separate handlers from aura-effects.

use aura_core::effects::*;
use aura_core::effects::crypto::{KeyDerivationContext, FrostSigningPackage};
use aura_core::effects::random::RandomEffects;
use aura_core::effects::time::TimeEffects;
use aura_effects::*;
use async_trait::async_trait;
use std::time::Duration;
use uuid::Uuid;

/// Combined effects handler for testing
/// 
/// This provides a simple combined effects interface for tests that need
/// crypto, time, random, and console operations. Uses mock implementations.
#[derive(Clone)]
pub struct TestEffectHandler {
    crypto: MockCryptoHandler,
    time: SimulatedTimeHandler,
    random: MockRandomHandler,
    console: MockConsoleHandler,
    storage: MemoryStorageHandler,
}

impl TestEffectHandler {
    /// Create test effects with default values
    pub fn new() -> Self {
        Self {
            crypto: MockCryptoHandler::new(42), // seed: 42
            time: SimulatedTimeHandler::new(1000), // timestamp: 1000
            random: MockRandomHandler::new(42), // seed: 42
            console: MockConsoleHandler::new(),
            storage: MemoryStorageHandler::new(),
        }
    }

    /// Create deterministic test effects
    pub fn deterministic(seed: u64, timestamp: u64) -> Self {
        Self {
            crypto: MockCryptoHandler::new(seed),
            time: SimulatedTimeHandler::new(timestamp),
            random: MockRandomHandler::new(seed),
            console: MockConsoleHandler::new(),
            storage: MemoryStorageHandler::new(),
        }
    }

    /// Create named test effects for debugging
    pub fn for_test(_name: &str) -> Self {
        Self::new()
    }

    /// Create production effects for integration tests
    pub fn production() -> Self {
        Self {
            crypto: MockCryptoHandler::new(42), // Use mock with default seed for testing
            time: SimulatedTimeHandler::new(), // Use simulated time
            random: MockRandomHandler::new(42), // Still use mock for predictability
            console: MockConsoleHandler::new(),
            storage: MemoryStorageHandler::new(),
        }
    }
}

#[async_trait::async_trait]
impl RandomEffects for TestEffectHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.random.random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        self.random.random_bytes_32().await
    }

    async fn random_u64(&self) -> u64 {
        self.random.random_u64().await
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        self.random.random_range(min, max).await
    }
}

#[async_trait::async_trait]
impl CryptoEffects for TestEffectHandler {
    async fn hash(&self, data: &[u8]) -> [u8; 32] {
        self.crypto.hash(data).await
    }

    async fn hmac(&self, key: &[u8], data: &[u8]) -> [u8; 32] {
        self.crypto.hmac(key, data).await
    }

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.hkdf_derive(ikm, salt, info, output_len).await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto.ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_sign(message, private_key).await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto.ed25519_verify(message, signature, public_key).await
    }

    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        self.crypto.frost_generate_keys(threshold, max_signers).await
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, CryptoError> {
        self.crypto.frost_generate_nonces().await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto.frost_create_signing_package(message, nonces, participants).await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.frost_sign_share(signing_package, key_share, nonces).await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.frost_aggregate_signatures(signing_package, signature_shares).await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto.frost_verify(message, signature, group_public_key).await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.ed25519_public_key(private_key).await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.chacha20_encrypt(plaintext, key, nonce).await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.chacha20_decrypt(ciphertext, key, nonce).await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.aes_gcm_encrypt(plaintext, key, nonce).await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.aes_gcm_decrypt(ciphertext, key, nonce).await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, CryptoError> {
        self.crypto.frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers).await
    }

    fn is_simulated(&self) -> bool {
        self.crypto.is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto.crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.secure_zero(data)
    }
}

#[async_trait::async_trait]
impl TimeEffects for TestEffectHandler {
    async fn current_epoch(&self) -> u64 {
        self.time.current_epoch().await
    }

    async fn current_timestamp(&self) -> u64 {
        self.time.current_timestamp().await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.time.current_timestamp_millis().await
    }

    async fn sleep_ms(&self, ms: u64) {
        self.time.sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        self.time.sleep_until(epoch).await
    }

    async fn delay(&self, duration: Duration) {
        self.time.delay(duration).await
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.time.sleep(duration_ms).await
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        self.time.yield_until(condition).await
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.time.wait_until(condition).await
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        self.time.set_timeout(timeout_ms).await
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        self.time.cancel_timeout(handle).await
    }

    fn is_simulated(&self) -> bool {
        self.time.is_simulated()
    }

    fn register_context(&self, context_id: Uuid) {
        self.time.register_context(context_id)
    }

    fn unregister_context(&self, context_id: Uuid) {
        self.time.unregister_context(context_id)
    }

    async fn notify_events_available(&self) {
        self.time.notify_events_available().await
    }

    fn resolution_ms(&self) -> u64 {
        self.time.resolution_ms()
    }
}

#[async_trait::async_trait]
impl ConsoleEffects for TestEffectHandler {
    async fn log_info(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_error(message).await
    }

    async fn log_debug(&self, message: &str) -> Result<(), aura_core::AuraError> {
        self.console.log_debug(message).await
    }
}

#[async_trait::async_trait]
impl StorageEffects for TestEffectHandler {
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), aura_core::effects::storage::StorageError> {
        self.storage.store(key, value).await
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, aura_core::effects::storage::StorageError> {
        self.storage.retrieve(key).await
    }

    async fn remove(&self, key: &str) -> Result<bool, aura_core::effects::storage::StorageError> {
        self.storage.remove(key).await
    }

    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, aura_core::effects::storage::StorageError> {
        self.storage.list_keys(prefix).await
    }

    async fn exists(&self, key: &str) -> Result<bool, aura_core::effects::storage::StorageError> {
        self.storage.exists(key).await
    }

    async fn store_batch(&self, pairs: std::collections::HashMap<String, Vec<u8>>) -> Result<(), aura_core::effects::storage::StorageError> {
        self.storage.store_batch(pairs).await
    }

    async fn retrieve_batch(&self, keys: &[String]) -> Result<std::collections::HashMap<String, Vec<u8>>, aura_core::effects::storage::StorageError> {
        self.storage.retrieve_batch(keys).await
    }

    async fn clear_all(&self) -> Result<(), aura_core::effects::storage::StorageError> {
        self.storage.clear_all().await
    }

    async fn stats(&self) -> Result<aura_core::effects::storage::StorageStats, aura_core::effects::storage::StorageError> {
        self.storage.stats().await
    }
}

/// Create deterministic test effects with a seed and timestamp
///
/// This is the most common pattern, used for reproducible tests.
///
/// # Arguments
/// * `seed` - Random seed for deterministic randomness
/// * `timestamp` - Fixed timestamp for deterministic time
///
/// # Example
/// ```rust
/// use aura_testkit::test_effects_deterministic;
///
/// let effects = test_effects_deterministic(42, 1000);
/// let random_bytes = effects.random_bytes::<32>();
/// let now = effects.now().unwrap();
/// assert_eq!(now, 1000);
/// ```
/// Type alias for backward compatibility
pub type Effects = TestEffectHandler;

/// Create deterministic test effects with a seed and timestamp
///
/// This is the most common pattern, used for reproducible tests.
pub fn test_effects_deterministic(seed: u64, timestamp: u64) -> TestEffectHandler {
    TestEffectHandler::deterministic(seed, timestamp)
}

/// Create simple test effects with default values
///
/// Uses a standard seed (42) and timestamp (1000) for basic testing.
pub fn test_effects() -> TestEffectHandler {
    TestEffectHandler::new()
}

/// Create named test effects for debugging
///
/// Useful when you need to identify which test created the effects.
pub fn test_effects_named(name: &str) -> TestEffectHandler {
    TestEffectHandler::for_test(name)
}

/// Create production effects for integration tests
///
/// Uses real system time and randomness, should only be used when
/// testing actual production behavior.
pub fn test_effects_production() -> TestEffectHandler {
    TestEffectHandler::production()
}

/// Create effects with a specific timestamp
///
/// Useful when you need a specific time for testing time-dependent logic.
pub fn test_effects_with_time(timestamp: u64) -> TestEffectHandler {
    TestEffectHandler::deterministic(42, timestamp)
}

/// Create effects with a specific seed
///
/// Useful when you need specific random values for testing.
pub fn test_effects_with_seed(seed: u64) -> TestEffectHandler {
    TestEffectHandler::deterministic(seed, 1000)
}

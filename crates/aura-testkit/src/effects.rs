//! Test Effects Utilities
//!
//! Standard factory functions for creating Effects instances for testing.
//! This eliminates the most common duplication pattern found across 29 test files.

use aura_core::effects::{CryptoEffects, RandomEffects, TimeEffects};
use aura_core::{AuraError, Result as AuraResult};
use std::pin::Pin;
use std::time::Duration;
use uuid::Uuid;

/// Combined effects for testing - replacement for deleted Effects type
///
/// This provides a simple combined effects interface for tests that need
/// both crypto, time, and random operations. Uses basic mock implementations.
pub struct Effects {
    timestamp: u64,
    seed: u64,
}

impl Effects {
    /// Create test effects with default values
    pub fn test() -> Self {
        Self { timestamp: 1000, seed: 42 }
    }

    /// Create deterministic test effects
    pub fn deterministic(seed: u64, timestamp: u64) -> Self {
        Self { timestamp, seed }
    }

    /// Create named test effects for debugging
    pub fn for_test(_name: &str) -> Self {
        Self::test()
    }
}

impl RandomEffects for Effects {
    type RandomBytes<const N: usize> = [u8; N];

    fn random_bytes<const N: usize>(&self) -> Self::RandomBytes<N> {
        // Simple deterministic "random" bytes for testing
        let mut result = [0u8; N];
        for (i, byte) in result.iter_mut().enumerate() {
            *byte = ((self.seed + i as u64) % 256) as u8;
        }
        result
    }

    fn secure_zero(&self, _buffer: &mut [u8]) {
        // Mock secure zero - in tests we don't need real secure zeroing
    }
}

impl CryptoEffects for Effects {
    type RandomBytes<const N: usize> = [u8; N];

    fn random_bytes<const N: usize>(&self) -> Self::RandomBytes<N> {
        RandomEffects::random_bytes(self)
    }

    async fn verify_signature(
        &self,
        _public_key: &[u8],
        _message: &[u8],
        _signature: &[u8],
    ) -> AuraResult<bool> {
        // Simple mock always returns true
        Ok(true)
    }

    async fn sign(&self, _private_key: &[u8], message: &[u8]) -> AuraResult<Vec<u8>> {
        // Simple mock signature (just hash the message)
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(message);
        Ok(hasher.finalize().to_vec())
    }

    async fn hash_sha256(&self, data: &[u8]) -> AuraResult<[u8; 32]> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        Ok(hasher.finalize().into())
    }

    async fn encrypt_aes256_gcm(
        &self,
        _key: &[u8; 32],
        plaintext: &[u8],
    ) -> AuraResult<Vec<u8>> {
        // Simple mock encryption (just return plaintext for testing)
        Ok(plaintext.to_vec())
    }

    async fn decrypt_aes256_gcm(
        &self,
        _key: &[u8; 32],
        ciphertext: &[u8],
    ) -> AuraResult<Vec<u8>> {
        // Simple mock decryption (just return ciphertext for testing)
        Ok(ciphertext.to_vec())
    }

    async fn derive_key(&self, input: &[u8], salt: &[u8]) -> AuraResult<[u8; 32]> {
        // Simple mock key derivation
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(input);
        hasher.update(salt);
        Ok(hasher.finalize().into())
    }
}

// Note: TimeEffects has many methods, so I'll implement a minimal subset for now
#[async_trait::async_trait]
impl TimeEffects for Effects {
    async fn now(&self) -> AuraResult<u64> {
        Ok(self.timestamp)
    }

    async fn sleep(&self, _duration_ms: u64) -> AuraResult<()> {
        // Mock sleep does nothing
        Ok(())
    }

    async fn current_epoch(&self) -> u64 {
        self.timestamp / 1000  // Simple epoch calculation
    }

    async fn current_timestamp(&self) -> u64 {
        self.timestamp
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.timestamp
    }

    async fn sleep_ms(&self, _duration_ms: u64) {
        // Mock sleep does nothing
    }

    async fn sleep_until(&self, _timestamp: u64) {
        // Mock sleep does nothing
    }

    async fn delay(&self, _duration: Duration) {
        // Mock delay does nothing
    }

    async fn yield_until(&self, _condition: aura_core::effects::WakeCondition) -> Result<(), aura_core::effects::TimeError> {
        // Mock yield always succeeds immediately
        Ok(())
    }

    async fn wait_until(&self, _condition: aura_core::effects::WakeCondition) -> AuraResult<()> {
        // Mock wait always succeeds immediately
        Ok(())
    }

    async fn set_timeout(&self, _duration_ms: u64) -> Uuid {
        // Return a dummy timeout ID
        Uuid::nil()
    }

    async fn cancel_timeout(&self, _timeout_id: Uuid) -> Result<(), aura_core::effects::TimeError> {
        // Mock cancel always succeeds
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true  // This is always a simulation for testing
    }

    fn register_context(&self, _context_id: Uuid) {
        // Mock registration does nothing
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Mock unregistration does nothing
    }

    async fn notify_events_available(&self) {
        // Mock notification does nothing
    }

    fn resolution_ms(&self) -> u64 {
        1  // 1ms resolution for testing
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
pub fn test_effects_deterministic(seed: u64, timestamp: u64) -> Effects {
    Effects::deterministic(seed, timestamp)
}

/// Create simple test effects with default values
///
/// Uses a standard seed (42) and timestamp (1000) for basic testing.
pub fn test_effects() -> Effects {
    Effects::test()
}

/// Create named test effects for debugging
///
/// Useful when you need to identify which test created the effects.
///
/// # Example
/// ```rust
/// use aura_testkit::test_effects_named;
///
/// let effects = test_effects_named("test_account_creation");
/// ```
pub fn test_effects_named(name: &str) -> Effects {
    Effects::for_test(name)
}

/// Create production effects for integration tests
///
/// Uses real system time and randomness, should only be used when
/// testing actual production behavior.
pub fn test_effects_production() -> Effects {
    Effects::production()
}

/// Create effects with a specific timestamp
///
/// Useful when you need a specific time for testing time-dependent logic.
pub fn test_effects_with_time(timestamp: u64) -> Effects {
    Effects::deterministic(42, timestamp)
}

/// Create effects with a specific seed
///
/// Useful when you need specific random values for testing.
pub fn test_effects_with_seed(seed: u64) -> Effects {
    Effects::deterministic(seed, 1000)
}

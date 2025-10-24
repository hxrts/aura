//! Injectable effects for deterministic testing
//!
//! This module provides abstractions for side effects (time, randomness) that can be
//! swapped between real implementations and test/simulation implementations.
//!
//! This enables:
//! - Deterministic tests (same inputs → same outputs)
//! - Time travel debugging (step forward/backward through time)
//! - Reproducible simulations (with seed-based randomness)
//! - Fast-forward testing (skip ahead in logical time)

use crate::{CryptoError, Result};
use rand::rngs::StdRng;
use rand::{CryptoRng, RngCore, SeedableRng};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

// ========== Time Source Abstraction ==========

/// Abstract time source - can be real system time or simulated time
///
/// This trait allows injecting different time sources:
/// - Production: Real system time
/// - Testing: Simulated time that can be fast-forwarded
/// - Debugging: Time-travel capable source
pub trait TimeSource: Send + Sync {
    /// Get current Unix timestamp in seconds
    fn current_timestamp(&self) -> Result<u64>;

    /// Advance time by N seconds (no-op for real time, used in simulations)
    fn advance(&self, _seconds: u64) -> Result<()> {
        Ok(()) // Default: no-op for real time sources
    }

    /// Set absolute time (for time-travel debugging)
    fn set_time(&self, _timestamp: u64) -> Result<()> {
        Err(CryptoError::SystemTimeError(
            "Time travel not supported for this time source".to_string(),
        ))
    }

    /// Check if this is a simulated time source
    fn is_simulated(&self) -> bool {
        false // Default: real time sources
    }
}

/// Real system time source (production use)
#[derive(Debug, Clone, Default)]
pub struct SystemTimeSource;

impl SystemTimeSource {
    /// Create a new system time source
    pub fn new() -> Self {
        SystemTimeSource
    }
}

impl TimeSource for SystemTimeSource {
    fn current_timestamp(&self) -> Result<u64> {
        #[allow(clippy::disallowed_methods)]
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|e| {
                CryptoError::SystemTimeError(format!("System time is before UNIX epoch: {}", e))
            })
    }
}

/// Simulated time source (for testing and time-travel debugging)
///
/// Allows manual control of time progression for deterministic tests.
#[derive(Debug, Clone)]
pub struct SimulatedTimeSource {
    current_time: Arc<Mutex<u64>>,
}

impl SimulatedTimeSource {
    /// Create a new simulated time source starting at the given timestamp
    pub fn new(initial_timestamp: u64) -> Self {
        SimulatedTimeSource {
            current_time: Arc::new(Mutex::new(initial_timestamp)),
        }
    }

    /// Create starting at Unix epoch (1970-01-01 00:00:00)
    pub fn from_epoch() -> Self {
        Self::new(0)
    }

    /// Create starting at a recent time (for more realistic tests)
    pub fn from_recent() -> Self {
        // 2025-01-01 00:00:00 UTC
        Self::new(1735689600)
    }
}

impl TimeSource for SimulatedTimeSource {
    fn current_timestamp(&self) -> Result<u64> {
        let time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::SystemTimeError(format!("Lock poisoned: {}", e)))?;
        Ok(*time)
    }

    fn advance(&self, seconds: u64) -> Result<()> {
        let mut time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::SystemTimeError(format!("Lock poisoned: {}", e)))?;
        *time = time.saturating_add(seconds);
        Ok(())
    }

    fn set_time(&self, timestamp: u64) -> Result<()> {
        let mut time = self
            .current_time
            .lock()
            .map_err(|e| CryptoError::SystemTimeError(format!("Lock poisoned: {}", e)))?;
        *time = timestamp;
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }
}

// ========== Random Source Abstraction ==========

/// Abstract randomness source - can be real RNG or seeded/deterministic RNG
///
/// This trait allows injecting different randomness sources:
/// - Production: Cryptographically secure OS randomness
/// - Testing: Seeded deterministic RNG (reproducible)
/// - Debugging: Controllable randomness for specific scenarios
pub trait RandomSource: Send + Sync {
    /// Fill a byte buffer with random data
    fn fill_bytes(&self, dest: &mut [u8]);

    /// Generate a random u64
    fn gen_u64(&self) -> u64;

    /// Generate a UUID (v4 for production, deterministic for testing)
    fn gen_uuid(&self) -> Uuid;
}

/// Real randomness source using OS entropy (production use)
///
/// Uses `rand::thread_rng()` which provides cryptographically secure randomness.
#[derive(Debug, Clone, Default)]
pub struct OsRandomSource;

impl OsRandomSource {
    /// Create a new OS random source
    pub fn new() -> Self {
        OsRandomSource
    }
}

impl RandomSource for OsRandomSource {
    fn fill_bytes(&self, dest: &mut [u8]) {
        use rand::RngCore;
        #[allow(clippy::disallowed_methods)]
        rand::thread_rng().fill_bytes(dest);
    }

    fn gen_u64(&self) -> u64 {
        use rand::RngCore;
        #[allow(clippy::disallowed_methods)]
        rand::thread_rng().next_u64()
    }

    fn gen_uuid(&self) -> Uuid {
        #[allow(clippy::disallowed_methods)]
        Uuid::new_v4()
    }
}

/// Seeded deterministic RNG (for testing and reproducible simulations)
///
/// Uses ChaCha8 PRNG which is:
/// - Fast enough for simulations
/// - Deterministic (same seed → same sequence)
/// - Good statistical properties
#[derive(Debug, Clone)]
pub struct SeededRandomSource {
    // Interior mutability for RNG state
    rng: Arc<Mutex<StdRng>>,
}

impl SeededRandomSource {
    /// Create a new seeded RNG with the given seed
    pub fn new(seed: u64) -> Self {
        SeededRandomSource {
            rng: Arc::new(Mutex::new(StdRng::seed_from_u64(seed))),
        }
    }

    /// Create with seed 0 (default for reproducible tests)
    pub fn default_seed() -> Self {
        Self::new(0)
    }

    /// Create with a specific seed for test isolation
    pub fn from_test_name(test_name: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        test_name.hash(&mut hasher);
        Self::new(hasher.finish())
    }
}

impl RandomSource for SeededRandomSource {
    fn fill_bytes(&self, dest: &mut [u8]) {
        #[allow(clippy::expect_used)] // Mutex poisoning is unrecoverable
        let mut rng = self.rng.lock().expect("RNG lock poisoned");
        rng.fill_bytes(dest);
    }

    fn gen_u64(&self) -> u64 {
        #[allow(clippy::expect_used)] // Mutex poisoning is unrecoverable
        let mut rng = self.rng.lock().expect("RNG lock poisoned");
        rng.next_u64()
    }

    fn gen_uuid(&self) -> Uuid {
        // Generate deterministic UUID using seeded randomness
        let mut bytes = [0u8; 16];
        self.fill_bytes(&mut bytes);

        // Create UUID from random bytes (v4 format)
        #[allow(clippy::disallowed_methods)]
        Uuid::from_bytes(bytes)
    }
}

// ========== Helper Functions ==========

/// Generate random bytes into a fixed-size array from any RandomSource
///
/// This is a helper function since we can't have const generic methods in trait objects.
pub fn gen_random_bytes<const N: usize>(source: &dyn RandomSource) -> [u8; N] {
    let mut bytes = [0u8; N];
    source.fill_bytes(&mut bytes);
    bytes
}

// ========== RNG Adapter ==========

/// Adapter to make RandomSource compatible with rand crate traits
///
/// This struct wraps a RandomSource and implements RngCore + CryptoRng
/// so it can be used with functions that expect standard rand RNG types.
pub struct EffectsRng {
    source: Arc<dyn RandomSource>,
}

impl RngCore for EffectsRng {
    fn next_u32(&mut self) -> u32 {
        (self.source.gen_u64() >> 32) as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.source.gen_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.source.fill_bytes(dest);
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EffectsRng {}

// ========== Effect Bundle ==========

/// Bundle of injectable effects
///
/// This struct holds all side effects that need to be controlled for testing.
/// Pass this to orchestrators and agents to enable deterministic behavior.
#[derive(Clone)]
pub struct Effects {
    /// Time source for timestamps
    pub time: Arc<dyn TimeSource>,
    /// Random number generator for cryptographic randomness
    pub random: Arc<dyn RandomSource>,
}

impl Effects {
    /// Create production effects (real time + OS randomness)
    pub fn production() -> Self {
        Effects {
            time: Arc::new(SystemTimeSource::new()),
            random: Arc::new(OsRandomSource::new()),
        }
    }

    /// Create deterministic test effects (simulated time + seeded RNG)
    pub fn deterministic(seed: u64, initial_time: u64) -> Self {
        Effects {
            time: Arc::new(SimulatedTimeSource::new(initial_time)),
            random: Arc::new(SeededRandomSource::new(seed)),
        }
    }

    /// Create test effects with default seed and recent time
    pub fn test() -> Self {
        Self::deterministic(0, 1735689600) // 2025-01-01
    }

    /// Create test effects isolated by test name
    pub fn for_test(test_name: &str) -> Self {
        Effects {
            time: Arc::new(SimulatedTimeSource::from_recent()),
            random: Arc::new(SeededRandomSource::from_test_name(test_name)),
        }
    }
}

impl Default for Effects {
    fn default() -> Self {
        Self::production()
    }
}

// ========== Convenience Methods ==========

impl Effects {
    /// Get current timestamp
    pub fn now(&self) -> Result<u64> {
        self.time.current_timestamp()
    }

    /// Advance time by N seconds (simulation only)
    pub fn advance_time(&self, seconds: u64) -> Result<()> {
        self.time.advance(seconds)
    }

    /// Jump to specific time (time-travel debugging)
    pub fn set_time(&self, timestamp: u64) -> Result<()> {
        self.time.set_time(timestamp)
    }

    /// Generate random bytes
    pub fn random_bytes<const N: usize>(&self) -> [u8; N] {
        gen_random_bytes(self.random.as_ref())
    }

    /// Fill buffer with random bytes
    pub fn fill_random(&self, dest: &mut [u8]) {
        self.random.fill_bytes(dest);
    }

    /// Generate a UUID (deterministic in tests)
    pub fn gen_uuid(&self) -> Uuid {
        self.random.gen_uuid()
    }

    /// Generate session ID (convenience method)
    pub fn gen_session_id(&self) -> Uuid {
        self.gen_uuid()
    }

    /// Check if running in simulation mode
    pub fn is_simulated(&self) -> bool {
        self.time.is_simulated()
    }

    /// Get an RNG adapter that implements standard rand traits
    /// This is needed for functions that expect `impl Rng` parameters
    pub fn rng(&self) -> EffectsRng {
        EffectsRng {
            source: self.random.clone(),
        }
    }

    /// Async delay - replaces tokio::time::sleep in production
    /// In simulation, this should yield to the scheduler
    pub async fn delay(&self, duration: Duration) {
        if self.is_simulated() {
            // In simulation, advance simulated time instead of real sleep
            let _ = self.time.advance(duration.as_secs());
        } else {
            // In production, use real async sleep
            tokio::time::sleep(duration).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_time_source() {
        let time_source = SystemTimeSource::new();
        let t1 = time_source.current_timestamp().unwrap();

        // Should be after 2020-01-01 (1577836800)
        assert!(t1 > 1577836800);

        // advance() should be no-op for real time
        assert!(time_source.advance(1000).is_ok());

        // Time should still be real time (not advanced)
        let t2 = time_source.current_timestamp().unwrap();
        assert!(t2 >= t1 && t2 < t1 + 100); // Allow some real time passage
    }

    #[test]
    fn test_simulated_time_source() {
        let time_source = SimulatedTimeSource::new(1000);

        assert_eq!(time_source.current_timestamp().unwrap(), 1000);

        // Advance time
        time_source.advance(500).unwrap();
        assert_eq!(time_source.current_timestamp().unwrap(), 1500);

        // Time travel
        time_source.set_time(2000).unwrap();
        assert_eq!(time_source.current_timestamp().unwrap(), 2000);
    }

    #[test]
    fn test_os_random_source() {
        let rng = OsRandomSource::new();

        let bytes1: [u8; 32] = gen_random_bytes(&rng);
        let bytes2: [u8; 32] = gen_random_bytes(&rng);

        // Should be different (with overwhelming probability)
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_seeded_random_source_deterministic() {
        let rng1 = SeededRandomSource::new(42);
        let rng2 = SeededRandomSource::new(42);

        let bytes1: [u8; 32] = gen_random_bytes(&rng1);
        let bytes2: [u8; 32] = gen_random_bytes(&rng2);

        // Same seed → same output
        assert_eq!(bytes1, bytes2);
    }

    #[test]
    fn test_seeded_random_source_different_seeds() {
        let rng1 = SeededRandomSource::new(42);
        let rng2 = SeededRandomSource::new(43);

        let bytes1: [u8; 32] = gen_random_bytes(&rng1);
        let bytes2: [u8; 32] = gen_random_bytes(&rng2);

        // Different seeds → different output
        assert_ne!(bytes1, bytes2);
    }

    #[test]
    fn test_effects_production() {
        let effects = Effects::production();

        let t1 = effects.now().unwrap();
        assert!(t1 > 1577836800); // After 2020

        let bytes: [u8; 16] = effects.random_bytes();
        assert_ne!(bytes, [0u8; 16]); // Should be random
    }

    #[test]
    fn test_effects_deterministic() {
        let effects1 = Effects::deterministic(123, 1000);
        let effects2 = Effects::deterministic(123, 1000);

        // Same seed + time → same behavior
        assert_eq!(effects1.now().unwrap(), effects2.now().unwrap());

        let bytes1: [u8; 32] = effects1.random_bytes();
        let bytes2: [u8; 32] = effects2.random_bytes();
        assert_eq!(bytes1, bytes2);

        // UUIDs should also be deterministic
        let uuid1 = effects1.gen_uuid();
        let uuid2 = effects2.gen_uuid();
        assert_eq!(uuid1, uuid2);
    }

    #[test]
    fn test_effects_time_travel() {
        let effects = Effects::test();

        let t1 = effects.now().unwrap();
        effects.advance_time(3600).unwrap(); // +1 hour
        let t2 = effects.now().unwrap();
        assert_eq!(t2, t1 + 3600);

        effects.set_time(1000).unwrap(); // Jump to specific time
        assert_eq!(effects.now().unwrap(), 1000);
    }

    #[test]
    fn test_from_test_name_isolation() {
        let effects1 = Effects::for_test("test_foo");
        let effects2 = Effects::for_test("test_bar");

        // Different test names → different seeds
        let bytes1: [u8; 32] = effects1.random_bytes();
        let bytes2: [u8; 32] = effects2.random_bytes();
        assert_ne!(bytes1, bytes2);

        // Same test name → same seed
        let effects3 = Effects::for_test("test_foo");
        let bytes3: [u8; 32] = effects3.random_bytes();
        assert_eq!(bytes1, bytes3);

        // UUIDs should also be isolated by test name
        let uuid1 = effects1.gen_uuid();
        let uuid2 = effects2.gen_uuid();
        assert_ne!(uuid1, uuid2);

        let uuid3 = effects3.gen_uuid();
        assert_eq!(uuid1, uuid3);
    }
}

//! Random effects for deterministic randomness

use rand::rngs::StdRng;
use rand::{CryptoRng, RngCore, SeedableRng};
use std::sync::{Arc, Mutex};

/// Combined trait for RNG that is both cryptographically secure and implements RngCore
pub trait CryptoRngCore: RngCore + rand::CryptoRng {}

/// Random effects interface for generating random values
pub trait RandomEffects {
    /// Generate random bytes of specified length
    ///
    /// # Arguments
    /// * `len` - Number of random bytes to generate
    fn random_bytes(&self, len: usize) -> Vec<u8>;

    /// Generate a random u64 value
    fn random_u64(&self) -> u64;

    /// Generate a random number in the specified range (inclusive)
    ///
    /// # Arguments
    /// * `min` - Minimum value (inclusive)
    /// * `max` - Maximum value (inclusive)
    fn random_range(&self, min: u64, max: u64) -> u64;

    /// Get an RNG that implements the rand crate traits
    ///
    /// This is needed for compatibility with libraries that expect `impl RngCore`.
    /// Returns a boxed RNG that implements both RngCore and CryptoRng traits.
    fn rng(&self) -> Box<dyn CryptoRngCore>;
}

/// Production random effects using system randomness
///
/// Uses the system's cryptographically secure random number generator
/// for generating random values in production environments.
pub struct ProductionRandomEffects;

impl ProductionRandomEffects {
    /// Create a new production random effects instance
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProductionRandomEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl RandomEffects for ProductionRandomEffects {
    fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        #[allow(clippy::disallowed_methods)]
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    fn random_u64(&self) -> u64 {
        #[allow(clippy::disallowed_methods)]
        rand::random()
    }

    fn random_range(&self, min: u64, max: u64) -> u64 {
        if max <= min {
            min
        } else {
            use rand::Rng;
            #[allow(clippy::disallowed_methods)]
            rand::thread_rng().gen_range(min..=max)
        }
    }

    fn rng(&self) -> Box<dyn CryptoRngCore> {
        #[allow(clippy::disallowed_methods)]
        Box::new(CryptoThreadRng(rand::thread_rng()))
    }
}

/// Test random effects with deterministic behavior
///
/// Provides repeatable random number generation for testing. Uses a seeded RNG
/// that produces consistent results across test runs, enabling reliable testing
/// of non-deterministic code paths.
pub struct TestRandomEffects {
    /// Seed value for the RNG
    seed: u64,
    /// Counter for fallback behavior when mutex is poisoned
    counter: std::sync::atomic::AtomicU64,
    /// Shared RNG for rand trait compatibility
    rng: Arc<Mutex<StdRng>>,
}

impl TestRandomEffects {
    /// Create a new test random effects instance with the given seed
    ///
    /// # Arguments
    /// * `seed` - The seed value for deterministic behavior
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            counter: std::sync::atomic::AtomicU64::new(0),
            rng: Arc::new(Mutex::new(StdRng::seed_from_u64(seed))),
        }
    }

    /// Create a test random effects instance with a seed derived from test name
    ///
    /// Useful for test isolation - the same test name always produces the same seed.
    ///
    /// # Arguments
    /// * `test_name` - The name of the test (usually from module::function)
    pub fn from_test_name(test_name: &str) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        test_name.hash(&mut hasher);
        Self::new(hasher.finish())
    }
}

impl RandomEffects for TestRandomEffects {
    fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        if let Ok(mut rng) = self.rng.lock() {
            rng.fill_bytes(&mut bytes);
        } else {
            // Fallback if mutex is poisoned
            let count = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let combined_seed = self.seed.wrapping_add(count);
            bytes = (0..len)
                .map(|i| ((combined_seed.wrapping_add(i as u64)) % 256) as u8)
                .collect();
        }
        bytes
    }

    fn random_u64(&self) -> u64 {
        if let Ok(mut rng) = self.rng.lock() {
            rng.next_u64()
        } else {
            // Fallback if mutex is poisoned
            let count = self
                .counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.seed.wrapping_add(count)
        }
    }

    fn random_range(&self, min: u64, max: u64) -> u64 {
        if max <= min {
            min
        } else {
            use rand::Rng;
            if let Ok(mut rng) = self.rng.lock() {
                rng.gen_range(min..=max)
            } else {
                // Fallback if mutex is poisoned
                min + (self.random_u64() % (max - min + 1))
            }
        }
    }

    fn rng(&self) -> Box<dyn CryptoRngCore> {
        // Create a new seeded RNG instance to avoid sharing mutable state
        let current_seed = self.seed.wrapping_add(
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        );
        Box::new(CryptoStdRng(StdRng::seed_from_u64(current_seed)))
    }
}

/// RNG adapter that implements CryptoRng for thread_rng
/// This is needed because rand::ThreadRng doesn't implement CryptoRng in some versions
struct CryptoThreadRng(rand::rngs::ThreadRng);

impl RngCore for CryptoThreadRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl CryptoRng for CryptoThreadRng {}

impl CryptoRngCore for CryptoThreadRng {}

/// RNG adapter for StdRng that implements our combined trait
struct CryptoStdRng(StdRng);

impl RngCore for CryptoStdRng {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.0.fill_bytes(dest)
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

impl CryptoRng for CryptoStdRng {}

impl CryptoRngCore for CryptoStdRng {}

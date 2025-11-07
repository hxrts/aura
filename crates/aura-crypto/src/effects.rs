//! Basic effects traits and concrete implementations for crypto operations
//!
//! This module provides both the effect traits and concrete implementations
//! that aura-crypto needs for testing and production use.

use aura_types::EffectsLike;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Basic crypto effects needed by aura-crypto
pub trait CryptoEffects: Send + Sync {
    /// Hash data using Blake3
    fn blake3_hash(&self, data: &[u8]) -> [u8; 32];

    /// Hash data using Blake3 (async version)
    fn blake3_hash_async<'a>(
        &'a self,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = [u8; 32]> + Send + '_>> {
        Box::pin(async move { self.blake3_hash(data) })
    }

    /// Generate random bytes (single byte version for object safety)
    fn random_byte(&self) -> u8;

    /// Generate a UUID
    fn gen_uuid(&self) -> Uuid;
}

/// Extension methods for CryptoEffects that work with generics
pub trait CryptoEffectsExt {
    /// Generate random bytes array
    fn random_bytes_array<const N: usize>(&self) -> [u8; N];
}

impl<T: ?Sized + CryptoEffects> CryptoEffectsExt for T {
    fn random_bytes_array<const N: usize>(&self) -> [u8; N] {
        let mut bytes = [0u8; N];
        for byte in bytes.iter_mut() {
            *byte = self.random_byte();
        }
        bytes
    }
}

/// Time effects for crypto operations
pub trait TimeEffects: Send + Sync {
    /// Get current timestamp
    fn now(&self) -> crate::Result<u64>;

    /// Advance time by the given number of seconds (for testing)
    fn advance_time(&self, seconds: u64) -> crate::Result<()>;
}

/// Combined effects trait that includes everything aura-crypto needs (object-safe)
pub trait EffectsInterface: CryptoEffects + TimeEffects + Send + Sync {
    /// Clone the implementation (needed for object safety)
    fn clone_box(&self) -> Box<dyn EffectsInterface>;
}

/// Concrete Effects implementation for testing and production use
///
/// This struct provides the familiar interface expected by test utilities
/// while being built on the trait-based system.
#[derive(Clone)]
pub struct Effects {
    inner: Arc<dyn EffectsInterface>,
}

impl Effects {
    /// Create a new Effects instance with the given implementation
    pub fn new(inner: Arc<dyn EffectsInterface>) -> Self {
        Self { inner }
    }

    /// Create deterministic effects for testing with seed and timestamp
    pub fn deterministic(seed: u64, timestamp: u64) -> Self {
        Self::new(Arc::new(DeterministicEffects::new(seed, timestamp)))
    }

    /// Create simple test effects with default values
    pub fn test() -> Self {
        Self::deterministic(42, 1000)
    }

    /// Create named test effects for debugging
    pub fn for_test(name: &str) -> Self {
        let seed = blake3::hash(name.as_bytes()).as_bytes()[0] as u64;
        Self::deterministic(seed, 1000)
    }

    /// Create production effects (placeholder for now)
    ///
    /// TODO: This is a placeholder that uses system RNG and time directly.
    /// In production, these should come from proper effect handlers.
    #[allow(clippy::disallowed_methods)]
    pub fn production() -> Self {
        use rand::RngCore;
        let seed = rand::thread_rng().next_u64();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self::deterministic(seed, timestamp)
    }

    /// Generate random bytes array
    pub fn random_bytes<const N: usize>(&self) -> [u8; N] {
        self.inner.random_bytes_array()
    }

    /// Get current timestamp
    pub fn now(&self) -> crate::Result<u64> {
        self.inner.now()
    }

    /// Advance time by the given number of seconds (for testing)
    pub fn advance_time(&self, seconds: u64) -> crate::Result<()> {
        self.inner.advance_time(seconds)
    }

    /// Hash data using Blake3
    pub fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        self.inner.blake3_hash(data)
    }

    /// Generate a UUID
    pub fn gen_uuid(&self) -> Uuid {
        self.inner.gen_uuid()
    }

    /// Get a random number generator for use with external libraries
    pub fn rng(&self) -> impl rand::RngCore {
        DeterministicRng::new(self.inner.random_byte() as u64)
    }
}

/// Implementation of EffectsLike for the Effects struct
impl EffectsLike for Effects {
    fn gen_uuid(&self) -> Uuid {
        self.inner.gen_uuid()
    }
}

/// Deterministic effects implementation for testing
#[derive(Clone)]
struct DeterministicEffects {
    #[allow(dead_code)] // Stored for debugging purposes
    seed: u64,
    current_time: Arc<Mutex<u64>>,
    rng_state: Arc<Mutex<u64>>,
}

impl DeterministicEffects {
    fn new(seed: u64, timestamp: u64) -> Self {
        Self {
            seed,
            current_time: Arc::new(Mutex::new(timestamp)),
            rng_state: Arc::new(Mutex::new(seed)),
        }
    }
}

impl CryptoEffects for DeterministicEffects {
    fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()
    }

    fn random_byte(&self) -> u8 {
        let mut state = self.rng_state.lock().unwrap();
        *state = state.wrapping_mul(1103515245).wrapping_add(12345);
        (*state / 65536) as u8
    }

    fn gen_uuid(&self) -> Uuid {
        let bytes = self.random_bytes_array::<16>();
        Uuid::from_bytes(bytes)
    }
}

impl TimeEffects for DeterministicEffects {
    fn now(&self) -> crate::Result<u64> {
        Ok(*self.current_time.lock().unwrap())
    }

    fn advance_time(&self, seconds: u64) -> crate::Result<()> {
        let mut time = self.current_time.lock().unwrap();
        *time += seconds;
        Ok(())
    }
}

impl EffectsInterface for DeterministicEffects {
    fn clone_box(&self) -> Box<dyn EffectsInterface> {
        Box::new(self.clone())
    }
}

/// Simple deterministic RNG for FROST operations
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl rand::RngCore for DeterministicRng {
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

/// Implementation of CryptoRng for deterministic testing
/// This is safe for testing but NOT for production use
impl rand::CryptoRng for DeterministicRng {}

/// Legacy test implementation of crypto effects (for backward compatibility)
pub struct TestCryptoEffects {
    seed: u64,
}

impl TestCryptoEffects {
    pub fn new(seed: u64) -> Self {
        Self { seed }
    }
}

impl CryptoEffects for TestCryptoEffects {
    fn blake3_hash(&self, data: &[u8]) -> [u8; 32] {
        blake3::hash(data).into()
    }

    fn random_byte(&self) -> u8 {
        ((self.seed) % 256) as u8
    }

    fn gen_uuid(&self) -> Uuid {
        // Generate deterministic UUID based on seed
        let mut data = [0u8; 16];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = ((self.seed + i as u64) % 256) as u8;
        }
        Uuid::from_bytes(data)
    }
}

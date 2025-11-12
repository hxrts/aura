//! Mock random effect handler for testing

use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use std::sync::{Arc, Mutex};

/// Mock random handler that provides deterministic values for testing
#[derive(Debug, Clone)]
pub struct MockRandomHandler {
    /// Deterministic seed for reproducible randomness
    seed: u64,
    /// Counter for generating unique values
    counter: Arc<Mutex<u64>>,
}

impl MockRandomHandler {
    /// Create a new mock random handler with the given seed
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a mock random handler with seed 0
    pub fn new_deterministic() -> Self {
        Self::new(0)
    }

    /// Reset the counter (for testing)
    pub fn reset(&self) {
        *self.counter.lock().unwrap() = 0;
    }

    /// Get the current counter value (for testing)
    pub fn get_counter(&self) -> u64 {
        *self.counter.lock().unwrap()
    }

    /// Generate deterministic pseudo-random bytes
    fn deterministic_bytes(&self, len: usize) -> Vec<u8> {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let mut bytes = Vec::with_capacity(len);
        for i in 0..len {
            bytes.push(((self.seed.wrapping_add(*counter).wrapping_add(i as u64)) % 256) as u8);
        }
        bytes
    }

    /// Generate deterministic pseudo-random value
    fn deterministic_value(&self) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        self.seed.wrapping_add(*counter)
    }
}

impl Default for MockRandomHandler {
    fn default() -> Self {
        Self::new_deterministic()
    }
}

#[async_trait]
impl RandomEffects for MockRandomHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        self.deterministic_bytes(len)
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.deterministic_bytes(32);
        bytes.try_into().unwrap()
    }

    async fn random_u64(&self) -> u64 {
        self.deterministic_value()
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let value = self.deterministic_value();
        min + (value % (max - min + 1))
    }
}

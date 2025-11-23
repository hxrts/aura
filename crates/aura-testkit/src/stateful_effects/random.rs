//! Mock random effect handlers for testing
//!
//! This module contains the stateful MockRandomHandler that was moved from aura-effects
//! to fix architectural violations. The handler uses Arc<Mutex<>> for deterministic
//! randomness in tests.

use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use std::sync::{Arc, Mutex};

/// Mock random handler for deterministic testing
#[derive(Debug, Clone)]
pub struct MockRandomHandler {
    seed: u64,
    counter: Arc<Mutex<u64>>,
}

impl Default for MockRandomHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRandomHandler {
    /// Create a new mock random handler with default seed (42)
    pub fn new() -> Self {
        Self {
            seed: 42,
            counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Create a new mock random handler with a specific seed
    pub fn with_seed(seed: u64) -> Self {
        Self {
            seed,
            counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Alias for with_seed for compatibility
    pub fn new_with_seed(seed: u64) -> Self {
        Self::with_seed(seed)
    }
}

#[async_trait]
impl RandomEffects for MockRandomHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        let mut bytes = Vec::with_capacity(len);
        for i in 0..len {
            bytes.push(((self.seed.wrapping_add(*counter).wrapping_add(i as u64)) % 256) as u8);
        }
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let bytes = self.random_bytes(32).await;
        bytes.try_into().unwrap()
    }

    async fn random_u64(&self) -> u64 {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
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

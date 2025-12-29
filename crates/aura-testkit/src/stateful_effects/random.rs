//! Mock random effect handlers for testing
//!
//! This module contains the stateful MockRandomHandler that was moved from aura-effects
//! to fix architectural violations. The handler uses Arc<Mutex<>> for deterministic
//! randomness in tests.
//!
//! # Blocking Lock Usage
//!
//! Uses `std::sync::Mutex` because this is Layer 8 test infrastructure where:
//! 1. Tests run in controlled single-threaded contexts
//! 2. Lock contention is not a concern in test scenarios
//! 3. Simpler synchronous API is preferred for test clarity

#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::effects::RandomCoreEffects;
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
impl RandomCoreEffects for MockRandomHandler {
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
}

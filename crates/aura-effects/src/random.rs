//! Random effect handlers
//!
//! This module provides standard implementations of the `RandomEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.
//!
//! Note: This module legitimately uses `rand::thread_rng()` as it implements the
//! RandomEffects trait - this is the effect handler layer where actual system
//! randomness is provided.

// Allow disallowed methods in effect handler implementations
#![allow(clippy::disallowed_methods)]

use aura_core::effects::RandomEffects;
use aura_macros::aura_effect_handlers;
use rand::RngCore;
use std::sync::{Arc, Mutex};

// Generate both mock and real random handlers using the macro
aura_effect_handlers! {
    trait_name: RandomEffects,
    mock: {
        struct_name: MockRandomHandler,
        state: {
            seed: u64,
            counter: Arc<Mutex<u64>>,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            random_bytes(len: usize) -> Vec<u8> => {
                let mut counter = self.counter.lock().unwrap();
                *counter += 1;
                let mut bytes = Vec::with_capacity(len);
                for i in 0..len {
                    bytes.push(((self.seed.wrapping_add(*counter).wrapping_add(i as u64)) % 256) as u8);
                }
                bytes
            },
            random_bytes_32() -> [u8; 32] => {
                let bytes = self.random_bytes(32).await;
                bytes.try_into().unwrap()
            },
            random_u64() -> u64 => {
                let mut counter = self.counter.lock().unwrap();
                *counter += 1;
                self.seed.wrapping_add(*counter)
            },
            random_range(min: u64, max: u64) -> u64 => {
                let value = self.random_u64().await;
                min + (value % (max - min + 1))
            },
            random_uuid() -> uuid::Uuid => {
                let bytes = self.random_bytes(16).await;
                let mut uuid_bytes = [0u8; 16];
                uuid_bytes.copy_from_slice(&bytes);
                uuid::Uuid::from_bytes(uuid_bytes)
            },
        },
    },
    real: {
        struct_name: RealRandomHandler,
        features: {
            async_trait: true,
            disallowed_methods: true,
        },
        methods: {
            random_bytes(len: usize) -> Vec<u8> => {
                let mut bytes = vec![0u8; len];
                rand::thread_rng().fill_bytes(&mut bytes);
                bytes
            },
            random_bytes_32() -> [u8; 32] => {
                let mut bytes = [0u8; 32];
                rand::thread_rng().fill_bytes(&mut bytes);
                bytes
            },
            random_u64() -> u64 => {
                use rand::Rng;
                rand::thread_rng().gen()
            },
            random_range(min: u64, max: u64) -> u64 => {
                use rand::Rng;
                rand::thread_rng().gen_range(min..=max)
            },
            random_uuid() -> uuid::Uuid => {
                let bytes = self.random_bytes(16).await;
                let mut uuid_bytes = [0u8; 16];
                uuid_bytes.copy_from_slice(&bytes);
                uuid::Uuid::from_bytes(uuid_bytes)
            },
        },
    },
}

impl MockRandomHandler {
    /// Create a new mock random handler with the given seed
    pub fn new_with_seed(seed: u64) -> Self {
        Self::with_config(seed, Arc::new(Mutex::new(0)))
    }

    /// Reset the counter (for testing)
    pub fn reset(&self) {
        *self.counter.lock().unwrap() = 0;
    }

    /// Get the current counter value (for testing)
    pub fn get_counter(&self) -> u64 {
        *self.counter.lock().unwrap()
    }
}

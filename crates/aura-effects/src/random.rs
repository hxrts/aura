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

use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use rand::RngCore;

/// Real random handler using actual cryptographically secure randomness
#[derive(Debug, Clone)]
pub struct RealRandomHandler;

impl Default for RealRandomHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RealRandomHandler {
    /// Create a new real random handler
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RandomEffects for RealRandomHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        use rand::Rng;
        rand::thread_rng().gen()
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        use rand::Rng;
        rand::thread_rng().gen_range(min..=max)
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let bytes = self.random_bytes(16).await;
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&bytes);
        uuid::Uuid::from_bytes(uuid_bytes)
    }
}

// MockRandomHandler moved to aura-testkit

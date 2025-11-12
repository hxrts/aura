//! Real random effect handler for production use
//!
//! This module wraps low-level random number generation operations and needs
//! to use disallowed methods like `rand::thread_rng()` for production use.
#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use aura_core::effects::RandomEffects;
use rand::RngCore;

/// Real random handler for production use
#[derive(Debug, Clone, Default)]
pub struct RealRandomHandler;

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
}

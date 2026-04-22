//! Random effect handlers
//!
//! This module provides standard implementations of the `RandomEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.
//!
//! Note: This module legitimately uses `rand::rngs::OsRng` as it implements the
//! RandomEffects trait - this is the effect handler layer where actual system
//! randomness is provided.

// Allow disallowed randomness APIs in this effect handler implementation.
// The architecture ban applies to application code; this Layer 3 handler is
// the sanctioned boundary where OS-backed randomness enters the effect system.
#![allow(clippy::disallowed_methods)]
#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_core::effects::random::RandomCoreEffects;
use rand::rngs::OsRng;
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
impl RandomCoreEffects for RealRandomHandler {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; len];
        OsRng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        bytes
    }

    async fn random_u64(&self) -> u64 {
        OsRng.next_u64()
    }
}

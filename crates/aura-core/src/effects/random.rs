//! Random effects trait definitions
//!
//! This module defines the trait interface for random number generation.
//! Implementations are provided in aura-protocol handlers.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing cryptographically secure randomness
//!
//! This is an infrastructure effect that must be implemented in `aura-effects`
//! with stateless handlers. Provides production (system RNG), testing (seeded),
//! and simulation (controlled) implementations.

use async_trait::async_trait;
use uuid::Uuid;

/// Core random effects for generating random values.
///
/// This trait provides cryptographically secure random number generation
/// for the Aura effects system. Implementations in handlers provide:
/// - Production: System cryptographic RNG
/// - Testing: Deterministic seeded RNG for reproducible tests
/// - Simulation: Controlled randomness for scenario testing
#[async_trait]
pub trait RandomCoreEffects: Send + Sync {
    /// Generate random bytes of specified length
    async fn random_bytes(&self, len: usize) -> Vec<u8>;

    /// Generate 32 random bytes as array
    async fn random_bytes_32(&self) -> [u8; 32];

    /// Generate a random u64 value
    async fn random_u64(&self) -> u64;
}

/// Optional random effects that build on the core RNG.
#[async_trait]
pub trait RandomExtendedEffects: RandomCoreEffects + Send + Sync {
    /// Generate a random number in the specified range
    async fn random_range(&self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        let span = max - min;
        min + (self.random_u64().await % span)
    }

    /// Generate a random UUID v4
    async fn random_uuid(&self) -> Uuid {
        let mut bytes = [0u8; 16];
        let random = self.random_bytes(16).await;
        bytes.copy_from_slice(&random[..16]);
        // Set UUID v4 variant bits without using disallowed Builder APIs.
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Uuid::from_bytes(bytes)
    }
}

#[async_trait]
impl<T: RandomCoreEffects + ?Sized> RandomExtendedEffects for T {}

/// Combined random effects surface (core + extended).
pub trait RandomEffects: RandomCoreEffects + RandomExtendedEffects {}

impl<T: RandomCoreEffects + RandomExtendedEffects + ?Sized> RandomEffects for T {}

/// Blanket implementation for Arc<T> where T: RandomCoreEffects
#[async_trait]
impl<T: RandomCoreEffects + ?Sized> RandomCoreEffects for std::sync::Arc<T> {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        (**self).random_bytes(len).await
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        (**self).random_bytes_32().await
    }

    async fn random_u64(&self) -> u64 {
        (**self).random_u64().await
    }
}

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

/// Random effects interface for generating random values
///
/// This trait provides cryptographically secure random number generation
/// for the Aura effects system. Implementations in handlers provide:
/// - Production: System cryptographic RNG
/// - Testing: Deterministic seeded RNG for reproducible tests
/// - Simulation: Controlled randomness for scenario testing
#[async_trait]
pub trait RandomEffects: Send + Sync {
    /// Generate random bytes of specified length
    async fn random_bytes(&self, len: usize) -> Vec<u8>;

    /// Generate 32 random bytes as array
    async fn random_bytes_32(&self) -> [u8; 32];

    /// Generate a random u64 value
    async fn random_u64(&self) -> u64;

    /// Generate a random number in the specified range
    async fn random_range(&self, min: u64, max: u64) -> u64;

    /// Generate a random UUID v4
    async fn random_uuid(&self) -> Uuid;
}

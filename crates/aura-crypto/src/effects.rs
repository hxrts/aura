//! Basic effects traits for crypto operations
//!
//! This module provides minimal effect traits that aura-crypto needs,
//! avoiding circular dependencies with aura-protocol.

use uuid::Uuid;
use std::future::Future;
use std::pin::Pin;

/// Basic crypto effects needed by aura-crypto
pub trait CryptoEffects: Send + Sync {
    /// Hash data using Blake3
    fn blake3_hash(&self, data: &[u8]) -> [u8; 32];

    /// Hash data using Blake3 (async version)
    fn blake3_hash_async<'a>(&'a self, data: &'a [u8]) -> Pin<Box<dyn Future<Output = [u8; 32]> + Send + '_>> {
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

/// Combined effects trait that includes everything aura-crypto needs
pub trait Effects: CryptoEffects + Send + Sync {}

/// Test implementation of crypto effects
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

impl Effects for TestCryptoEffects {}
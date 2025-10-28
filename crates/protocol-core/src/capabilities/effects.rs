//! Effects provider abstraction.

use aura_errors::Result;
use uuid::Uuid;

/// Provider for injectable side effects used by protocols.
pub trait EffectsProvider: Send + Sync {
    /// Current timestamp (milliseconds since UNIX epoch).
    fn now(&self) -> Result<u64>;
    /// Generate a new UUID.
    fn gen_uuid(&self) -> Uuid;
    /// Generate random bytes into a vector.
    fn random_bytes_vec(&self, len: usize) -> Vec<u8>;
    /// Read monotonic counter.
    fn counter(&self) -> u64;
    /// Increment and return monotonic counter.
    fn next_counter(&self) -> u64;
}

/// Extension helpers for effects.
pub trait EffectsExt: EffectsProvider {
    /// Generate a timestamp-based nonce.
    fn timestamp_nonce(&self) -> Result<u64> {
        self.now()
    }

    /// Generate random bytes of fixed length.
    fn random_bytes<const N: usize>(&self) -> [u8; N] {
        let vec = self.random_bytes_vec(N);
        let mut arr = [0u8; N];
        arr.copy_from_slice(&vec);
        arr
    }

    /// Generate a cryptographically random u64.
    fn random_u64(&self) -> u64 {
        let bytes = self.random_bytes::<8>();
        u64::from_le_bytes(bytes)
    }

    /// Generate a cryptographically random u32.
    fn random_u32(&self) -> u32 {
        let bytes = self.random_bytes::<4>();
        u32::from_le_bytes(bytes)
    }
}

impl<T: EffectsProvider> EffectsExt for T {}

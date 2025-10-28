//! Effects Provider Abstraction
//!
//! Provides an abstract interface for deterministic and controllable side effects
//! such as time, randomness, and UUID generation.

use aura_types::Result;
use uuid::Uuid;

/// Provider for injectable side effects
///
/// This trait enables dependency injection of effects for testing and
/// deterministic execution. Services should depend on this trait instead
/// of concrete `Effects` implementations.
pub trait EffectsProvider: Send + Sync + Clone {
    /// Get current timestamp (milliseconds since UNIX epoch)
    fn now(&self) -> Result<u64>;

    /// Generate a new UUID
    fn gen_uuid(&self) -> Uuid;

    /// Generate random bytes of specified length
    fn random_bytes<const N: usize>(&self) -> [u8; N];

    /// Generate random bytes into a vector
    fn random_bytes_vec(&self, len: usize) -> Vec<u8>;

    /// Get monotonic counter value (for deterministic ordering)
    fn counter(&self) -> u64;

    /// Increment and return counter (for sequence numbers)
    fn next_counter(&self) -> u64;
}

/// Extension trait for Effects-based operations
pub trait EffectsExt: EffectsProvider {
    /// Generate a timestamp-based nonce
    fn timestamp_nonce(&self) -> Result<u64> {
        self.now()
    }

    /// Generate a cryptographically random u64
    fn random_u64(&self) -> u64 {
        let bytes = self.random_bytes::<8>();
        u64::from_le_bytes(bytes)
    }

    /// Generate a cryptographically random u32
    fn random_u32(&self) -> u32 {
        let bytes = self.random_bytes::<4>();
        u32::from_le_bytes(bytes)
    }
}

/// Automatically implement EffectsExt for all EffectsProvider implementations
impl<T: EffectsProvider> EffectsExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    #[derive(Clone)]
    struct MockEffects {
        timestamp: u64,
        counter: u64,
    }

    impl MockEffects {
        fn new() -> Self {
            Self {
                timestamp: 1000,
                counter: 0,
            }
        }
    }

    impl EffectsProvider for MockEffects {
        fn now(&self) -> Result<u64> {
            Ok(self.timestamp)
        }

        fn gen_uuid(&self) -> Uuid {
            Uuid::new_v4()
        }

        fn random_bytes<const N: usize>(&self) -> [u8; N] {
            [0u8; N]
        }

        fn random_bytes_vec(&self, len: usize) -> Vec<u8> {
            vec![0u8; len]
        }

        fn counter(&self) -> u64 {
            self.counter
        }

        fn next_counter(&self) -> u64 {
            self.counter + 1
        }
    }

    #[test]
    fn test_effects_provider() {
        let effects = MockEffects::new();

        // Test basic operations
        assert_eq!(effects.now().unwrap(), 1000);
        assert_eq!(effects.counter(), 0);

        // Test extension trait
        assert_eq!(effects.timestamp_nonce().unwrap(), 1000);
        assert_eq!(effects.random_u64(), 0);
        assert_eq!(effects.random_u32(), 0);
    }
}

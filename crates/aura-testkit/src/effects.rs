//! Test Effects Utilities
//!
//! Standard factory functions for creating Effects instances for testing.
//! This eliminates the most common duplication pattern found across 29 test files.

use aura_crypto::Effects;

/// Create deterministic test effects with a seed and timestamp
///
/// This is the most common pattern, used for reproducible tests.
///
/// # Arguments
/// * `seed` - Random seed for deterministic randomness
/// * `timestamp` - Fixed timestamp for deterministic time
///
/// # Example
/// ```rust
/// use aura_testkit::test_effects_deterministic;
///
/// let effects = test_effects_deterministic(42, 1000);
/// let random_bytes = effects.random_bytes::<32>();
/// let now = effects.now().unwrap();
/// assert_eq!(now, 1000);
/// ```
pub fn test_effects_deterministic(seed: u64, timestamp: u64) -> Effects {
    Effects::deterministic(seed, timestamp)
}

/// Create simple test effects with default values
///
/// Uses a standard seed (42) and timestamp (1000) for basic testing.
pub fn test_effects() -> Effects {
    Effects::test()
}

/// Create named test effects for debugging
///
/// Useful when you need to identify which test created the effects.
///
/// # Example
/// ```rust
/// use aura_testkit::test_effects_named;
///
/// let effects = test_effects_named("test_account_creation");
/// ```
pub fn test_effects_named(name: &str) -> Effects {
    Effects::for_test(name)
}

/// Create production effects for integration tests
///
/// Uses real system time and randomness, should only be used when
/// testing actual production behavior.
pub fn test_effects_production() -> Effects {
    Effects::production()
}

/// Create effects with a specific timestamp
///
/// Useful when you need a specific time for testing time-dependent logic.
pub fn test_effects_with_time(timestamp: u64) -> Effects {
    Effects::deterministic(42, timestamp)
}

/// Create effects with a specific seed
///
/// Useful when you need specific random values for testing.
pub fn test_effects_with_seed(seed: u64) -> Effects {
    Effects::deterministic(seed, 1000)
}

//! Test Account Utilities
//!
//! Factory functions for creating test AccountState instances.
//! Consolidates the account creation pattern found in 18 test files.

use crate::device::test_device_with_effects;
use aura_crypto::Effects;
use aura_journal::AccountState;
use aura_types::{AccountId, AccountIdExt};
use ed25519_dalek::{SigningKey, VerifyingKey};

/// Create a test account with given effects
///
/// This is the standard pattern for creating test accounts, found in many test files.
/// Creates a complete AccountState with a group public key and initial device.
///
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
///
/// # Example
/// ```rust
/// use aura_crypto::Effects;
/// use aura_test_utils::test_account_with_effects;
///
/// let effects = Effects::deterministic(42, 1000);
/// let account = test_account_with_effects(&effects);
/// ```
pub fn test_account_with_effects(effects: &Effects) -> AccountState {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects);

    AccountState::new(
        AccountId::new_with_effects(effects),
        group_public_key,
        device_metadata,
        2, // threshold
        3, // total participants
    )
}

/// Create a test account with seed
///
/// Convenience function that creates effects and account in one call.
///
/// # Arguments
/// * `seed` - Random seed for deterministic generation
///
/// # Example
/// ```rust
/// use aura_test_utils::test_account_with_seed;
///
/// let account = test_account_with_seed(42);
/// ```
pub fn test_account_with_seed(seed: u64) -> AccountState {
    let effects = Effects::deterministic(seed, 1000);
    test_account_with_effects(&effects)
}

/// Create a test account with custom threshold
///
/// For testing different threshold configurations.
///
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
pub fn test_account_with_threshold(effects: &Effects, threshold: u16, total: u16) -> AccountState {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects);

    AccountState::new(
        AccountId::new_with_effects(effects),
        group_public_key,
        device_metadata,
        threshold,
        total,
    )
}

/// Create a test account with specific account ID
///
/// For tests that need a predictable account ID.
///
/// # Arguments
/// * `account_id` - Specific account ID to use
/// * `effects` - Effects instance for other random generation
pub fn test_account_with_id(account_id: AccountId, effects: &Effects) -> AccountState {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects);

    AccountState::new(account_id, group_public_key, device_metadata, 2, 3)
}

/// Create a test account with specific group public key
///
/// For tests that need to control the group key.
///
/// # Arguments
/// * `group_public_key` - Specific group public key to use
/// * `effects` - Effects instance for other random generation
pub fn test_account_with_group_key(
    group_public_key: VerifyingKey,
    effects: &Effects,
) -> AccountState {
    let device_metadata = test_device_with_effects(effects);

    AccountState::new(
        AccountId::new_with_effects(effects),
        group_public_key,
        device_metadata,
        2,
        3,
    )
}

//! Test Account Utilities
//!
//! Factory functions for creating test Journal instances.
//! Consolidates the account creation pattern found in test files.

use aura_core::hash::hash;
use aura_core::AccountId;
use aura_journal::journal_api::Journal;
use ed25519_dalek::VerifyingKey;
use uuid::Uuid;

/// Create a test account with seed
///
/// This is the standard pattern for creating test accounts, found in many test files.
/// Creates a Journal with a group public key.
///
/// # Arguments
/// * `seed` - Seed for deterministic generation
///
/// # Example
/// ```rust
/// use aura_testkit::test_account_with_seed;
///
/// let account = test_account_with_seed(42);
/// ```
pub fn test_account_with_seed_sync(seed: u64) -> Journal {
    let (_, group_public_key) = crate::test_key_pair(seed);

    // Generate deterministic account ID
    let hash_input = format!("account-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    Journal::new_with_group_key(account_id, group_public_key)
}

/// Create a test account with seed (async version for compatibility)
///
/// Convenience function that creates account deterministically.
///
/// # Arguments
/// * `seed` - Random seed for deterministic generation
///
/// # Example
/// ```rust
/// use aura_testkit::test_account_with_seed;
///
/// let account = test_account_with_seed(42).await;
/// ```
pub async fn test_account_with_seed(seed: u64) -> Journal {
    test_account_with_seed_sync(seed)
}

/// Create a test account with custom threshold
///
/// For testing different threshold configurations.
///
/// # Arguments
/// * `seed` - Seed for deterministic generation
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
pub async fn test_account_with_threshold(seed: u64, _threshold: u16, _total: u16) -> Journal {
    let (_, group_public_key) = crate::test_key_pair(seed);

    // Generate deterministic account ID
    let hash_input = format!("threshold-account-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    Journal::new_with_group_key(account_id, group_public_key)
}

/// Create a test account with specific account ID
///
/// For tests that need a predictable account ID.
///
/// # Arguments
/// * `account_id` - Specific account ID to use
/// * `seed` - Seed for other deterministic generation
pub async fn test_account_with_id(account_id: AccountId, seed: u64) -> Journal {
    let (_, group_public_key) = crate::test_key_pair(seed);

    Journal::new_with_group_key(account_id, group_public_key)
}

/// Create a test account with specific group public key
///
/// For tests that need to control the group key.
///
/// # Arguments
/// * `group_public_key` - Specific group public key to use
/// * `seed` - Seed for other deterministic generation
pub async fn test_account_with_group_key(group_public_key: VerifyingKey, seed: u64) -> Journal {
    // Generate deterministic account ID
    let hash_input = format!("custom-group-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    Journal::new_with_group_key(account_id, group_public_key)
}

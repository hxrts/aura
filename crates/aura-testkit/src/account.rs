//! Test Account Utilities
//!
//! Factory functions for creating test AccountState instances.
//! Consolidates the account creation pattern found in 18 test files.

use aura_core::hash::hash;
use aura_core::{AccountId, DeviceId};
use aura_journal::semilattice::ModernAccountState as AccountState;
use aura_journal::{DeviceMetadata, DeviceType};
use ed25519_dalek::{SigningKey, VerifyingKey};
use uuid::Uuid;

/// Helper function to create test device metadata with seed
fn test_device_with_seed(seed: u64) -> DeviceMetadata {
    let (_, device_public_key) = crate::test_key_pair(seed);

    // Generate deterministic UUID from seed
    let hash_input = format!("device-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid = Uuid::from_bytes(hash_bytes[..16].try_into().unwrap());

    DeviceMetadata {
        device_id: DeviceId(uuid),
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: device_public_key,
        added_at: 1000, // Default test timestamp
        last_seen: 1000,
        dkd_commitment_proofs: Default::default(),
        next_nonce: 0,
        key_share_epoch: 0,
        used_nonces: Default::default(),
    }
}

/// Create a test account with seed
///
/// This is the standard pattern for creating test accounts, found in many test files.
/// Creates a complete AccountState with a group public key and initial device.
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
pub fn test_account_with_seed_sync(seed: u64) -> AccountState {
    let (_, group_public_key) = crate::test_key_pair(seed);
    let device_metadata = test_device_with_seed(seed + 1);

    // Generate deterministic account ID
    let hash_input = format!("account-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    let mut state = AccountState::new(account_id, group_public_key);
    state.add_device(device_metadata);
    state
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
pub async fn test_account_with_seed(seed: u64) -> AccountState {
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
pub async fn test_account_with_threshold(seed: u64, threshold: u16, total: u16) -> AccountState {
    let (_, group_public_key) = crate::test_key_pair(seed);

    // Generate deterministic account ID
    let hash_input = format!("threshold-account-{}-{}-{}", seed, threshold, total);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    let mut state = AccountState::new(account_id, group_public_key);

    // Add devices to match total
    for i in 0..total {
        let device_metadata = test_device_with_seed(seed + i as u64);
        state.add_device(device_metadata);
    }

    state
}

/// Create a test account with specific account ID
///
/// For tests that need a predictable account ID.
///
/// # Arguments
/// * `account_id` - Specific account ID to use
/// * `seed` - Seed for other deterministic generation
pub async fn test_account_with_id(account_id: AccountId, seed: u64) -> AccountState {
    let (_, group_public_key) = crate::test_key_pair(seed);
    let device_metadata = test_device_with_seed(seed + 1);

    let mut state = AccountState::new(account_id, group_public_key);
    state.add_device(device_metadata);
    state
}

/// Create a test account with specific group public key
///
/// For tests that need to control the group key.
///
/// # Arguments
/// * `group_public_key` - Specific group public key to use
/// * `seed` - Seed for other deterministic generation
pub async fn test_account_with_group_key(
    group_public_key: VerifyingKey,
    seed: u64,
) -> AccountState {
    let device_metadata = test_device_with_seed(seed);

    // Generate deterministic account ID
    let hash_input = format!("custom-group-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let account_id = AccountId(Uuid::from_bytes(hash_bytes[..16].try_into().unwrap()));

    let mut state = AccountState::new(account_id, group_public_key);
    state.add_device(device_metadata);
    state
}

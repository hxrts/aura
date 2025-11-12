//! Test Account Utilities
//!
//! Factory functions for creating test AccountState instances.
//! Consolidates the account creation pattern found in 18 test files.

use aura_core::{AccountId, DeviceId};
use aura_core::effects::{RandomEffects, TimeEffects};
use crate::Effects;
use aura_journal::semilattice::ModernAccountState as AccountState;
use aura_journal::{DeviceMetadata, DeviceType};
use ed25519_dalek::{SigningKey, VerifyingKey};
use uuid::Uuid;

/// Helper function to create test device metadata with effects
async fn test_device_with_effects(effects: &Effects) -> DeviceMetadata {
    let device_key_bytes = effects.random_bytes_32().await;
    let device_signing_key = SigningKey::from_bytes(&device_key_bytes);
    let device_public_key = device_signing_key.verifying_key();

    // Generate deterministic UUID from random bytes
    let uuid_bytes = effects.random_bytes(16).await;
    let uuid_array: [u8; 16] = uuid_bytes.try_into().unwrap();
    let uuid = Uuid::from_bytes(uuid_array);

    DeviceMetadata {
        device_id: DeviceId(uuid),
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key: device_public_key,
        added_at: effects.current_timestamp_millis().await,
        last_seen: effects.current_timestamp_millis().await,
        dkd_commitment_proofs: Default::default(),
        next_nonce: 0,
        key_share_epoch: 0,
        used_nonces: Default::default(),
    }
}

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
/// use aura_testkit::{Effects, test_account_with_effects};
///
/// let effects = Effects::deterministic(42, 1000);
/// let account = test_account_with_effects(&effects).await;
/// ```
pub async fn test_account_with_effects(effects: &Effects) -> AccountState {
    let key_bytes = effects.random_bytes_32().await;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects).await;

    let mut state = AccountState::new(AccountId(Uuid::new_v4()), group_public_key);

    // Add the initial device
    state.add_device(device_metadata);

    state
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
/// use aura_testkit::test_account_with_seed;
///
/// let account = test_account_with_seed(42).await;
/// ```
pub async fn test_account_with_seed(seed: u64) -> AccountState {
    let effects = Effects::deterministic(seed, 1000);
    test_account_with_effects(&effects).await
}

/// Create a test account with custom threshold
///
/// For testing different threshold configurations.
///
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
pub async fn test_account_with_threshold(effects: &Effects, threshold: u16, total: u16) -> AccountState {
    let key_bytes = effects.random_bytes_32().await;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects).await;

    let mut state = AccountState::new(AccountId(Uuid::new_v4()), group_public_key);

    // Add the initial device
    state.add_device(device_metadata);

    // Add remaining devices to match total
    for _ in 1..total {
        let additional_device = test_device_with_effects(effects).await;
        state.add_device(additional_device);
    }

    state
}

/// Create a test account with specific account ID
///
/// For tests that need a predictable account ID.
///
/// # Arguments
/// * `account_id` - Specific account ID to use
/// * `effects` - Effects instance for other random generation
pub async fn test_account_with_id(account_id: AccountId, effects: &Effects) -> AccountState {
    let key_bytes = effects.random_bytes_32().await;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let group_public_key = signing_key.verifying_key();

    let device_metadata = test_device_with_effects(effects).await;

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
/// * `effects` - Effects instance for other random generation
pub async fn test_account_with_group_key(
    group_public_key: VerifyingKey,
    effects: &Effects,
) -> AccountState {
    let device_metadata = test_device_with_effects(effects).await;

    let mut state = AccountState::new(AccountId(Uuid::new_v4()), group_public_key);
    state.add_device(device_metadata);
    state
}

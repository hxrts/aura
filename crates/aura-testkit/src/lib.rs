//! Aura Testing Infrastructure
//!
//! This module provides common test setup functions to eliminate duplication
//! across test modules. It includes factories for creating test accounts,
//! devices, keys, and other common test fixtures.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
//!
//! # Usage
//!
//! Add this to your crate's `Cargo.toml` dev-dependencies:
//! ```toml
//! [dev-dependencies]
//! aura-testkit = { path = "../aura-testkit" }
//! ```
//!
//! Then in your tests:
//! ```rust,no_run
//! use aura_testkit::*;
//!
//! #[test]
//! fn my_test() {
//!     let effects = test_effects_deterministic(42, 1000);
//!     let account = test_account_with_effects(&effects);
//!     // ... test logic
//! }
//! ```

pub mod account;
pub mod assertions;
pub mod config;
pub mod device;
pub mod effects;
pub mod factories;
pub mod fixtures;
pub mod keys;
pub mod ledger;
pub mod mocks;
pub mod privacy;
pub mod protocol;
pub mod transport;

// Re-export commonly used items
pub use account::*;
pub use assertions::*;
pub use config::*;
pub use device::{DeviceSetBuilder, DeviceTestFixture};
pub use effects::*;
pub use factories::*;
pub use fixtures::*;
pub use keys::KeyTestFixture;
pub use ledger::*;
pub use mocks::*;
pub use protocol::*;
pub use transport::*;

// Re-export commonly used external types for convenience
pub use aura_core::{AccountId, AccountIdExt, DeviceId, DeviceIdExt};
pub use aura_journal::semilattice::ModernAccountState as AccountState;
pub use aura_journal::{DeviceMetadata, DeviceType};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use std::collections::BTreeMap;
pub use uuid::Uuid;

// Re-export FROST key generation helper
pub use keys::helpers::test_frost_key_shares;

/// Quick test account with deterministic seed
///
/// This is the most common pattern - creates a complete test account
/// with deterministic Effects for reproducible tests.
pub fn quick_test_account(seed: u64) -> AccountState {
    let effects = test_effects_deterministic(seed, 1000);
    test_account_with_effects(&effects)
}

/// Quick test device with deterministic ID
///
/// Creates a test device with a predictable ID for easy testing.
pub fn quick_test_device(id: u16) -> DeviceMetadata {
    let effects = test_effects_deterministic(id as u64, 1000);
    test_device_with_id(id, &effects)
}

/// Create a test key pair with effects
///
/// Returns a tuple of (SigningKey, VerifyingKey) for testing.
pub fn test_key_pair(effects: &Effects) -> (SigningKey, VerifyingKey) {
    let key_bytes = effects.random_bytes::<32>();
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// Create test device metadata with a specific ID
///
/// Helper function for creating test devices with predictable IDs.
pub fn test_device_with_id(id: u16, effects: &Effects) -> DeviceMetadata {
    let device_key_bytes = effects.random_bytes::<32>();
    let device_signing_key = SigningKey::from_bytes(&device_key_bytes);
    let device_public_key = device_signing_key.verifying_key();

    // Use deterministic UUID based on the ID
    let uuid_bytes = format!("device-{:04}", id);
    let hash_bytes = blake3::hash(uuid_bytes.as_bytes());
    let uuid = Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap());

    DeviceMetadata {
        device_id: DeviceId(uuid),
        device_name: format!("Test Device {}", id),
        device_type: DeviceType::Native,
        public_key: device_public_key,
        added_at: effects.now().unwrap(),
        last_seen: effects.now().unwrap(),
        dkd_commitment_proofs: Default::default(),
        next_nonce: 0,
        key_share_epoch: 0,
        used_nonces: Default::default(),
    }
}

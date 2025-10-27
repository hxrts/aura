//! Shared Test Utilities for Aura
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
//! aura-test-utils = { path = "../test-utils" }
//! ```
//!
//! Then in your tests:
//! ```rust,no_run
//! use aura_test_utils::*;
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
pub mod device;
pub mod effects;
pub mod fixtures;
pub mod keys;
pub mod ledger;
pub mod protocol;
pub mod transport;

// Re-export commonly used items
pub use account::*;
pub use assertions::*;
pub use device::*;
pub use effects::*;
pub use fixtures::*;
pub use keys::*;
pub use ledger::*;
pub use protocol::*;
pub use transport::*;

// Re-export commonly used external types for convenience
pub use aura_crypto::Effects;
pub use aura_journal::{AccountState, DeviceMetadata, DeviceType};
pub use aura_types::{AccountId, AccountIdExt, DeviceId, DeviceIdExt};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use std::collections::BTreeMap;
pub use uuid::Uuid;

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

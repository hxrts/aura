//! Shared Test Utilities for Aura
//!
//! This module provides common test setup functions to eliminate duplication
//! across test modules. It includes factories for creating test accounts,
//! devices, keys, and other common test fixtures.
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
//! ```rust
//! use aura_test_utils::*;
//! 
//! #[test]
//! fn my_test() {
//!     let effects = test_effects_deterministic(42, 1000);
//!     let account = test_account_with_effects(&effects);
//!     // ... test logic
//! }
//! ```

pub mod effects;
pub mod account;
pub mod device;
pub mod keys;
pub mod ledger;
pub mod transport;
pub mod protocol;

// Re-export commonly used items
pub use effects::*;
pub use account::*;
pub use device::*;
pub use keys::*;
pub use ledger::*;
pub use transport::*;
pub use protocol::*;

// Re-export commonly used external types for convenience
pub use aura_crypto::Effects;
pub use aura_journal::{AccountId, DeviceId, AccountState, DeviceMetadata, DeviceType};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use uuid::Uuid;
pub use std::collections::BTreeMap;

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
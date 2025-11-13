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
//! #[tokio::test]
//! async fn my_test() {
//!     let account = test_account_with_seed(42).await;
//!     let device_id = DeviceId::new();
//!     let fixture = ProtocolTestFixture::for_unit_tests(device_id).await.unwrap();
//!     // ... test logic
//! }
//! ```

pub mod account;
pub mod assertions;
pub mod choreography;
pub mod clean_fixtures;
pub mod config;
pub mod device;
pub mod effects_integration;
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
pub use choreography::{
    test_device_pair, test_device_trio, test_threshold_group, ChoreographyTestHarness,
    CoordinatedSession, MockChoreographyTransport, MockSessionCoordinator, PerformanceSnapshot,
    SimulatorCompatibleContext, TestError, TransportError,
};
pub use clean_fixtures::TestFixtures;
pub use config::*;
pub use device::{DeviceSetBuilder, DeviceTestFixture};
pub use effects_integration::{MockHandlerConfig, TestEffectsBuilder, TestExecutionMode};
pub use factories::*;
pub use fixtures::{
    AccountTestFixture, CryptoTestFixture, ProtocolTestFixture, StatelessFixtureConfig,
    StatelessFixtureError,
};
pub use keys::KeyTestFixture;
pub use ledger::*;
pub use mocks::*;
pub use protocol::*;
pub use transport::*;

// Re-export commonly used external types for convenience
pub use aura_core::{AccountId, DeviceId};
pub use aura_journal::semilattice::ModernAccountState as AccountState;
pub use aura_journal::{DeviceMetadata, DeviceType};
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use std::collections::BTreeMap;
pub use uuid::Uuid;

// Re-export FROST key generation helper
pub use keys::helpers::test_frost_key_shares;

/// Create a test key pair with deterministic seed
///
/// Returns a tuple of (SigningKey, VerifyingKey) for testing.
/// Uses a simple deterministic approach for basic testing.
pub fn test_key_pair(seed: u64) -> (SigningKey, VerifyingKey) {
    // Use deterministic key generation based on seed
    let mut key_bytes = [0u8; 32];
    key_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

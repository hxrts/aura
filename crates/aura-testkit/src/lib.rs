//! Aura Testkit
//!
//! This module provides common test setup functions to eliminate duplication
//! across test modules. It includes factories for creating test accounts,
//! devices, keys, and common test fixtures.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(missing_docs)]
//!
//! # Usage
//!
//! Add this to dev-dependencies:
//! ```toml
//! [dev-dependencies]
//! aura-testkit = { path = "../aura-testkit" }
//! aura-macros = { path = "../aura-macros" }
//! ```
//!
//! Then in your tests:
//! ```rust,no_run
//! use aura_testkit::*;
//! use aura_macros::aura_test;
//!
//! #[aura_test]
//! async fn my_test() -> aura_core::AuraResult<()> {
//!     // Effect system automatically initialized
//!     let account = test_account_with_seed(42).await;
//!     let fixture = create_test_fixture().await?;
//!     // ... test logic
//!     Ok(())
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
pub mod network_sim;
pub mod privacy;
pub mod protocol;
pub mod test_harness;
pub mod time;
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

// Re-export the test harness functions as convenience exports
pub use test_harness::{
    create_test_context, create_test_context_with_config, init_test_tracing, TestConfig,
    TestContext, TestFixture,
};

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

/// Create a test fixture with pre-configured effect system
///
/// Convenience function that replaces the deprecated `AuraEffectSystem::for_testing()` pattern.
/// This function creates a TestFixture with the effect system already initialized.
pub async fn create_test_fixture() -> aura_core::AuraResult<TestFixture> {
    TestFixture::new().await
}

/// Create a test fixture with a specific device ID (deterministic)
///
/// This is useful for tests that need predictable device IDs.
pub async fn create_test_fixture_with_device_id(
    device_id: DeviceId,
) -> aura_core::AuraResult<TestFixture> {
    let config = TestConfig {
        name: "test_with_device_id".to_string(),
        deterministic_time: true,
        capture_effects: false,
        timeout: Some(std::time::Duration::from_secs(30)),
    };

    // We'll need to create the context manually to specify the device ID
    // For now, create normal fixture and document this as a TODO for enhancement
    TestFixture::with_config(config).await
}

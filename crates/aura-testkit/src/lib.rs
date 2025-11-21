//! Aura Testkit
//!
//! This module provides common test setup functions to eliminate duplication
//! across test modules. It includes factories for creating test accounts,
//! devices, keys, and common test fixtures.
//!
//! # Architecture Constraints
//!
//! **IMPORTANT**: This testkit is designed for testing **Layer 4 and higher** crates only:
//! - Layer 4: aura-protocol (orchestration)
//! - Layer 5: aura-frost, aura-invitation, aura-recovery, aura-sync, etc. (protocols)
//! - Layer 6: aura-agent, aura-simulator (runtime)
//! - Layer 7: aura-cli (UI)
//!
//! **DO NOT use aura-testkit in foundation/specification layers** (would create circular dependencies):
//! - ❌ Layer 1: aura-core (foundation)
//! - ❌ Layer 2: aura-journal, aura-wot, aura-verify, aura-store, aura-transport (specification)
//! - ❌ Layer 3: aura-effects (implementation)
//!
//! Foundation layers should create their own internal test utilities (e.g., `aura-core/src/test_utils.rs`)
//! to avoid circular dependencies.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)] // Testkit needs SystemTime::now() for mock implementations
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

// Existing modular structure
pub mod builders;
pub mod configuration;
pub mod effect_api;
pub mod fixtures;
pub mod foundation;
pub mod infrastructure;
pub mod mocks;
pub mod simulation;
pub mod time;
pub mod verification;

// Make privacy module available at top level for backward compatibility
pub mod privacy {
    pub use crate::configuration::privacy::*;
}

// Re-export commonly used items from modular structure
pub use builders::*;
pub use configuration::TestConfig as ConfigTestConfig;
pub use effect_api::*;
pub use fixtures::*;
pub use foundation::*;
pub use infrastructure::*;
pub use mocks::*;
pub use simulation::*;
pub use time::*;
pub use verification::*;

// Re-export commonly used external types for convenience
pub use aura_core::AccountId;
pub use aura_journal::journal_api::{AccountSummary, Journal};

// Re-export Journal as AccountState for backward compatibility in tests
pub type AccountState = Journal;
pub use ed25519_dalek::{SigningKey, VerifyingKey};
pub use std::collections::BTreeMap;
pub use uuid::Uuid;

// Test harness functions available through infrastructure re-exports

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
pub async fn create_test_fixture() -> aura_core::AuraResult<infrastructure::harness::TestFixture> {
    infrastructure::harness::TestFixture::new().await
}

/// Create a test fixture with a specific device ID (deterministic)
///
/// This is useful for tests that need predictable device IDs.
/// Note: DeviceId parameter is ignored in authority-centric model
pub async fn create_test_fixture_with_device_id(
    _device_id: aura_core::DeviceId,
) -> aura_core::AuraResult<infrastructure::harness::TestFixture> {
    // Use the harness TestConfig directly to avoid ambiguity
    let config = infrastructure::harness::TestConfig {
        name: "test_with_device_id".to_string(),
        deterministic_time: true,
        capture_effects: false,
        timeout: Some(std::time::Duration::from_secs(30)),
    };

    // We'll need to create the context manually to specify the device ID
    // For now, create normal fixture and document this as a TODO for enhancement
    infrastructure::harness::TestFixture::with_config(config).await
}

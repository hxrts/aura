//! # Aura Testkit - Layer 8: Testing & Tools
//!
//! This crate provides shared testing infrastructure, fixtures, and utilities for the Aura platform.
//!
//! ## Purpose
//!
//! Layer 8 testing tools crate providing:
//! - Test fixtures for common account, device, and authority scenarios
//! - Effect system test harnesses and effect capture utilities
//! - Mock implementations of effects for deterministic testing
//! - Time control facilities for temporal testing
//! - Privacy analysis and information flow verification tools
//! - Simulation infrastructure for protocol testing
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1-7**: All lower layers (core, domain crates, effects, protocols, features, runtime, UI)
//! - **MUST NOT**: Be imported by Layer 1-3 crates (would create circular dependencies)
//! - **MAY be imported by**: Layer 4-7 crates in dev-dependencies only
//!
//! **IMPORTANT**: This testkit is designed for testing **Layer 4 and higher** crates only:
//! - Layer 4: aura-protocol (orchestration)
//! - Layer 5: aura-frost, aura-invitation, aura-recovery, aura-sync, etc. (protocols)
//! - Layer 6: aura-agent, aura-simulator (runtime)
//! - Layer 7: aura-cli (UI)
//!
//! Foundation layers should create their own internal test utilities (e.g., `aura-core/src/test_utils.rs`)
//! to avoid circular dependencies. These tests should use stateless effect patterns where possible.
//!
//! ## What Belongs Here
//!
//! - Test fixtures and builders for common scenarios
//! - Effect system test harnesses and mocking infrastructure
//! - Mock effect implementations for testing
//! - Time control and deterministic scheduling for tests
//! - Privacy analysis tools and information flow verification
//! - Simulation support infrastructure
//! - Configuration management for test environments
//! - Verification utilities for protocol testing
//!
//! ## What Does NOT Belong Here
//!
//! - Production effect implementations (belong in aura-effects)
//! - Protocol logic (belong in Layer 5 feature crates)
//! - Runtime composition logic (belong in aura-agent)
//! - UI implementations (belong in aura-cli)
//! - Test cases for specific crates (belong in those crates' test modules)
//! - Formal verification logic (Quint belongs in aura-quint-api)
//!
//! ## Design Principles
//!
//! - Reusability: Test fixtures are designed for common testing scenarios
//! - Determinism: All test infrastructure produces deterministic results
//! - Isolation: Tests can run independently without interference
//! - Effect-based: Uses the same effect system as production
//! - Mock transparency: Mock effects behave consistently with real ones
//! - Cleanup: Test fixtures clean up resources on drop
//! - Dependency-aware: Respects architectural layer constraints
//!
//! ## Key Components
//!
//! - **builders**: Builder patterns for constructing test accounts, devices, authorities
//! - **fixtures**: Pre-configured test scenarios and environments
//! - **mocks**: Mock effect implementations for testing
//! - **effect_api**: Effect system test harnesses and capture utilities
//! - **time**: Deterministic time control for temporal testing
//! - **verification**: Protocol verification and assertion utilities
//! - **privacy**: Privacy analysis and leakage tracking
//! - **simulation**: Simulation support for protocol testing

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
//! ```rust,ignore
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
pub mod handlers; // Test and mock handlers moved from aura-protocol
pub mod infrastructure;
pub mod mock_effects;
pub mod mocks;
pub mod simulation;
pub mod stateful_effects;
pub mod time;
pub mod verification;

// Make privacy module available at top level for backward compatibility
pub mod privacy {
    pub use crate::configuration::privacy::*;
}

// Re-export commonly used items from modular structure
#[allow(ambiguous_glob_reexports)]
pub use builders::*;
pub use configuration::TestConfig as ConfigTestConfig;
pub use effect_api::*;
#[allow(ambiguous_glob_reexports)]
pub use fixtures::*;
pub use foundation::*;
pub use infrastructure::*;
pub use mock_effects::MockEffects;
pub use mocks::*;
// Re-export simulation components (excluding ambiguous transport)
pub use simulation::transport as simulation_transport;
pub use simulation::{choreography::*, network::*};

// Re-export stateful effects (all items)
pub use stateful_effects::*;
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

    // We currently ignore the requested device ID; callers that need a specific
    // ID should construct a custom TestFixture via the harness builder directly.
    infrastructure::harness::TestFixture::with_config(config).await
}

//! Test helper utilities for aura-protocol tests - LEGACY
//!
//! DEPRECATED: This module provides legacy test utilities that should be migrated to aura-testkit.
//! New tests should use aura-testkit patterns instead of these custom helpers.
//!
//! Use aura-testkit instead:
//! - DeviceTestFixture::new(index) instead of create_test_device_id()
//! - create_test_fixture() instead of manual ID creation
//! - TestEffectsBuilder::for_unit_tests() instead of custom handlers
//! - ChoreographyTestHarness for multi-device scenarios

use aura_core::{
    identifiers::{DeviceId, SessionId},
    AccountId,
};
use aura_protocol::{
    handlers::CompositeHandler,
};
use uuid::Uuid;

// === LEGACY FUNCTIONS - Use aura-testkit equivalents instead ===

/// DEPRECATED: Use DeviceTestFixture::new(0).device_id() instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit DeviceTestFixture::new(0).device_id() instead")]
#[allow(dead_code)]
pub fn create_test_device_id() -> DeviceId {
    // Delegate to testkit for consistency
    use aura_testkit::DeviceTestFixture;
    DeviceTestFixture::new(0).device_id()
}

/// DEPRECATED: Use DeviceTestFixture::new(1).device_id() instead  
#[deprecated(since = "0.1.0", note = "Use aura-testkit DeviceTestFixture::new(1).device_id() instead")]
#[allow(dead_code)]
pub fn create_test_device_id_2() -> DeviceId {
    // Delegate to testkit for consistency
    use aura_testkit::DeviceTestFixture;
    DeviceTestFixture::new(1).device_id()
}

/// DEPRECATED: Use create_test_fixture().session_id() instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit create_test_fixture().session_id() instead")]
#[allow(dead_code)]
pub fn create_test_session_id() -> SessionId {
    // Delegate to testkit for consistency
    use aura_testkit::create_test_fixture;
    create_test_fixture().session_id()
}

/// DEPRECATED: Use create_test_fixture().account_id() instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit create_test_fixture().account_id() instead")]
#[allow(dead_code)]
pub fn create_test_account_id() -> AccountId {
    // Delegate to testkit for consistency
    use aura_testkit::create_test_fixture;
    create_test_fixture().account_id()
}

/// DEPRECATED: Use DeviceTestFixture::new(i).device_id() in a loop instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit DeviceTestFixture::new(i).device_id() in a loop instead")]
#[allow(dead_code)]
pub fn create_test_participants(count: usize) -> Vec<DeviceId> {
    // Delegate to testkit for consistency
    use aura_testkit::DeviceTestFixture;
    (0..count)
        .map(|i| DeviceTestFixture::new(i).device_id())
        .collect()
}

/// DEPRECATED: Use TestEffectsBuilder::for_unit_tests() instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit TestEffectsBuilder::for_unit_tests() instead")]
#[allow(dead_code)]
pub fn create_test_handler() -> CompositeHandler {
    use aura_testkit::DeviceTestFixture;
    CompositeHandler::for_testing(DeviceTestFixture::new(0).device_id().into())
}

/// DEPRECATED: Use TestEffectsBuilder with simulation mode instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit TestEffectsBuilder with simulation mode instead")]
#[allow(dead_code)]
pub fn create_simulation_handler() -> CompositeHandler {
    use aura_testkit::DeviceTestFixture;
    CompositeHandler::for_simulation(DeviceTestFixture::new(0).device_id().into())
}

/// DEPRECATED: Use testkit's deterministic builders instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit's deterministic builders instead")]
#[allow(dead_code)]
pub fn create_deterministic_uuid(seed: u128) -> Uuid {
    Uuid::from_u128(seed + 0x1234567890abcdef)
}

/// DEPRECATED: Use testkit's data builders instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit's data builders instead")]
#[allow(dead_code)]
pub fn create_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

/// DEPRECATED: Use testkit's KeyTestFixture instead
#[deprecated(since = "0.1.0", note = "Use aura-testkit's KeyTestFixture instead")]
#[allow(dead_code)]
pub fn create_test_keypair() -> ([u8; 32], [u8; 32]) {
    // Delegate to testkit for proper key generation
    use aura_testkit::builders::keys::KeyTestFixture;
    let fixture = KeyTestFixture::from_seed_string("test_keypair");
    let private_key = fixture.signing_key().to_bytes();
    let public_key = fixture.verifying_key().to_bytes();
    (private_key, public_key)
}

// === DISABLED FUNCTIONS (depend on missing modules) ===
//
// /// Create a test execution context
// pub fn create_test_execution_context() -> ExecutionContext { ... }
//
// /// Create a test execution context for simulation
// pub fn create_test_simulation_context() -> ExecutionContext { ... }
//
// /// Create a middleware config for testing
// pub fn create_test_middleware_config() -> MiddlewareConfig { ... }
//
// /// Create a handler with middleware for testing
// pub fn create_test_handler_with_middleware() -> impl ProtocolEffects { ... }

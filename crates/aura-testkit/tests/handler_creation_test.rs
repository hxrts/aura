//! Handler creation and initialization tests.
//!
//! Validates that testkit handlers can be created in all execution modes
//! and that the effect builder API works correctly.

#![allow(clippy::expect_used)]

use aura_testkit::{create_test_fixture, TestEffectsBuilder, TestEffectHandler};

/// Test basic fixture creation provides a valid device ID.
#[tokio::test]
async fn test_composite_handler_creation() {
    let fixture = create_test_fixture()
        .await
        .expect("Failed to create test fixture");

    let device_id = fixture.device_id();
    assert_ne!(device_id.to_string(), "");
}

/// Test that the effects builder produces handlers with the correct
/// execution mode.
#[tokio::test]
async fn test_effect_support() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);
    let effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_seed(42)
        .build()
        .expect("Failed to build test effects");

    assert_eq!(
        effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

/// Unit test mode and simulation mode produce the expected execution modes.
#[tokio::test]
async fn test_execution_mode() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    let unit_test_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .build()
        .expect("Failed to build unit test effects");
    assert_eq!(
        unit_test_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    let sim_effects = TestEffectsBuilder::for_simulation(device_id)
        .with_seed(42)
        .build()
        .expect("Failed to build simulation effects");
    assert_eq!(
        sim_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 42 }
    );
}

/// All three builder modes (unit, integration, simulation) produce valid handlers.
#[tokio::test]
async fn test_fixture_modes() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    let _ = TestEffectsBuilder::for_unit_tests(device_id)
        .build()
        .expect("Failed to create unit test fixture");

    let _ = TestEffectsBuilder::for_integration_tests(device_id)
        .build()
        .expect("Failed to create integration test fixture");

    let _ = TestEffectsBuilder::for_simulation(device_id)
        .with_seed(42)
        .build()
        .expect("Failed to create simulation fixture");
}

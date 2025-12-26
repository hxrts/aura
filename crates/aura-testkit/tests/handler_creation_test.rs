#![allow(warnings)]
#![cfg(any())]
#![allow(missing_docs)]
#![doc = "Handler creation integration tests"]
//! Test basic handler creation and functionality
//!
//! Uses aura-testkit for deterministic, reproducible tests

use aura_testkit::{create_test_fixture, TestEffectsBuilder, TestExecutionMode};

/// Test basic handler creation using testkit
#[tokio::test]
async fn test_composite_handler_creation() {
    let fixture = create_test_fixture()
        .await
        .expect("Failed to create test fixture");

    // Testkit provides deterministic device IDs and contexts
    let device_id = fixture.device_id();
    assert_ne!(device_id.to_string(), "");
}

/// Test effect support using testkit builder
#[tokio::test]
async fn test_effect_support() {
    let device_id = aura_core::identifiers::DeviceId::new();
    let effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_seed(42) // Deterministic
        .build()
        .expect("Failed to build test effects");

    // Test context provides deterministic execution mode
    assert_eq!(
        effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

/// Test execution mode using testkit
#[tokio::test]
async fn test_execution_mode() {
    let device_id = aura_core::identifiers::DeviceId::new();

    // Test different execution modes
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

/// Test fixture creation modes
#[tokio::test]
async fn test_fixture_modes() {
    let device_id = aura_core::identifiers::DeviceId::new();

    // Unit test mode
    let _ = TestEffectsBuilder::for_unit_tests(device_id)
        .build()
        .expect("Failed to create unit test fixture");

    // Integration test mode
    let _ = TestEffectsBuilder::for_integration_tests(device_id)
        .build()
        .expect("Failed to create integration test fixture");

    // Simulation mode
    let _ = TestEffectsBuilder::for_simulation(device_id)
        .with_seed(42)
        .build()
        .expect("Failed to create simulation fixture");
}

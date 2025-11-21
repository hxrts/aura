#![cfg(feature = "fixture_effects")]

//! Tests for individual effect traits and their implementations
//!
//! Uses aura-testkit for deterministic, reproducible testing

use aura_testkit::{test_key_pair, TestEffectsBuilder};

/// Test crypto effects through testkit
#[tokio::test]
async fn test_crypto_effects() {
    let device_id = aura_core::identifiers::DeviceId::new();
    let effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_seed(42) // Deterministic
        .build()
        .expect("Failed to build test effects");

    // Verify the effects context was created
    assert_eq!(effects.device_id(), device_id);
}

/// Test deterministic key generation
#[tokio::test]
async fn test_deterministic_keys() {
    // Test that testkit provides deterministic key generation
    let (sk1, vk1) = test_key_pair(42);
    let (sk2, vk2) = test_key_pair(42);

    assert_eq!(vk1, vk2);
    assert_eq!(sk1.to_bytes(), sk2.to_bytes());
}

/// Test different execution modes
#[tokio::test]
async fn test_execution_modes() {
    let device_id = aura_core::identifiers::DeviceId::new();

    // Unit test mode (full mocking)
    let unit_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .build()
        .expect("Failed to create unit test effects");
    assert_eq!(
        unit_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    // Simulation mode (deterministic behavior)
    let sim_effects = TestEffectsBuilder::for_simulation(device_id)
        .with_seed(123)
        .build()
        .expect("Failed to create simulation effects");
    assert_eq!(
        sim_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 123 }
    );

    // Integration test mode (selective mocking)
    let int_effects = TestEffectsBuilder::for_integration_tests(device_id)
        .build()
        .expect("Failed to create integration effects");
    assert_eq!(
        int_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

/// Test time acceleration for faster tests
#[tokio::test]
async fn test_time_acceleration() {
    let device_id = aura_core::identifiers::DeviceId::new();

    let effects = TestEffectsBuilder::for_integration_tests(device_id)
        .with_time_acceleration(10.0) // 10x faster
        .build()
        .expect("Failed to create accelerated effects");

    // Verify effects were created with time acceleration config
    assert_eq!(
        effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

/// Test storage configuration
#[tokio::test]
async fn test_storage_config() {
    let device_id = aura_core::identifiers::DeviceId::new();

    // Test with mock storage (default for unit tests)
    let mock_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_mock_storage(true)
        .build()
        .expect("Failed to create mock storage effects");
    assert_eq!(
        mock_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    // Test with custom storage directory
    let dir_effects = TestEffectsBuilder::for_integration_tests(device_id)
        .with_storage_dir(std::path::PathBuf::from("/tmp/aura-test"))
        .build()
        .expect("Failed to create dir storage effects");
    assert_eq!(
        dir_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

/// Test network mocking configuration
#[tokio::test]
async fn test_network_config() {
    let device_id = aura_core::identifiers::DeviceId::new();

    // Test with mock network (default for unit tests)
    let mock_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_mock_network(true)
        .build()
        .expect("Failed to create mock network effects");
    assert_eq!(
        mock_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    // Test with real network (for integration)
    let real_effects = TestEffectsBuilder::for_integration_tests(device_id)
        .with_mock_network(false)
        .build()
        .expect("Failed to create real network effects");
    assert_eq!(
        real_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

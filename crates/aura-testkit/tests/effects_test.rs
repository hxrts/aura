//! Effect handler integration tests.
//!
//! Tests for individual effect traits and their testkit implementations.
//! Verifies execution modes, deterministic key generation, time acceleration,
//! and storage/network mock configuration.

#![allow(clippy::expect_used)]

use aura_testkit::{test_key_pair, TestEffectHandler, TestEffectsBuilder};

/// Crypto effects context has the correct execution mode.
#[tokio::test]
async fn test_crypto_effects() {
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

/// Same seed produces identical key pairs — determinism is required for
/// reproducible tests.
#[tokio::test]
async fn test_deterministic_keys() {
    let (sk1, vk1) = test_key_pair(42);
    let (sk2, vk2) = test_key_pair(42);

    assert_eq!(vk1, vk2);
    assert_eq!(sk1.to_bytes(), sk2.to_bytes());
}

/// All three builder modes produce the expected execution mode.
#[tokio::test]
async fn test_execution_modes() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    let unit_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .build()
        .expect("Failed to create unit test effects");
    assert_eq!(
        unit_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    let sim_effects = TestEffectsBuilder::for_simulation(device_id)
        .with_seed(123)
        .build()
        .expect("Failed to create simulation effects");
    assert_eq!(
        sim_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 123 }
    );

    // Integration mode uses real handlers → Simulation execution mode
    let int_effects = TestEffectsBuilder::for_integration_tests(device_id)
        .build()
        .expect("Failed to create integration effects");
    assert_eq!(
        int_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 42 }
    );
}

/// Time acceleration config produces a valid handler.
#[tokio::test]
async fn test_time_acceleration() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    // Integration mode with time acceleration → Simulation execution mode
    let effects = TestEffectsBuilder::for_integration_tests(device_id)
        .with_time_acceleration(10.0)
        .build()
        .expect("Failed to create accelerated effects");

    assert_eq!(
        effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 42 }
    );
}

/// Mock and directory-backed storage configs produce valid handlers.
#[tokio::test]
async fn test_storage_config() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    let mock_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_mock_storage(true)
        .build()
        .expect("Failed to create mock storage effects");
    assert_eq!(
        mock_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    // Integration mode uses real storage → Simulation execution mode
    let dir_effects = TestEffectsBuilder::for_integration_tests(device_id)
        .with_storage_dir(std::path::PathBuf::from("/tmp/aura-test"))
        .build()
        .expect("Failed to create dir storage effects");
    assert_eq!(
        dir_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Simulation { seed: 42 }
    );
}

/// Mock and real network configs produce valid handlers.
#[tokio::test]
async fn test_network_config() {
    let device_id = aura_core::types::identifiers::DeviceId::new_from_entropy([3u8; 32]);

    let mock_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_mock_network(true)
        .build()
        .expect("Failed to create mock network effects");
    assert_eq!(
        mock_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );

    // Real network + mock storage → Testing (mock_storage is still true)
    let real_effects = TestEffectsBuilder::for_unit_tests(device_id)
        .with_mock_network(false)
        .build()
        .expect("Failed to create real network effects");
    assert_eq!(
        real_effects.execution_mode(),
        aura_core::effects::ExecutionMode::Testing
    );
}

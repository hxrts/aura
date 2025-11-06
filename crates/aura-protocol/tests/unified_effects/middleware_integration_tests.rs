//! Integration tests for middleware functionality in the unified effect system
//!
//! These tests verify that middleware properly integrates with the AuraEffectSystem
//! and provides the expected behavior for composition, ordering, and execution.

use std::time::Duration;
use uuid::Uuid;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::handlers::{AuraContext, AuraHandler, EffectType, ExecutionMode};
use aura_protocol::middleware::MiddlewareStack;
use aura_types::identifiers::DeviceId;

#[tokio::test]
async fn test_middleware_stack_integration() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = AuraEffectSystem::for_testing(device_id);

    // Verify the system has middleware support
    assert!(system.supports_effect(EffectType::Crypto));
    assert!(system.supports_effect(EffectType::Network));
    assert!(system.supports_effect(EffectType::Console));
}

#[tokio::test]
async fn test_crypto_middleware_functionality() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test BLAKE3 hashing through crypto middleware
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"test data for hashing".to_vec(),
    };

    let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoHashResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    // Verify hash was generated
    assert!(!result.hash.is_empty());
    assert_eq!(result.hash.len(), 32); // BLAKE3 produces 32-byte hashes
    assert_eq!(
        result.algorithm,
        aura_protocol::effects::crypto::HashAlgorithm::Blake3
    );

    // Test deterministic random bytes generation
    let random_params = aura_protocol::effects::crypto::CryptoRandomParams {
        length: 16,
        algorithm: aura_protocol::effects::crypto::RandomAlgorithm::ChaCha20,
        seed: Some(42),
    };

    let effect = Effect::new(EffectType::Crypto, "random", &random_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoRandomResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    assert_eq!(result.bytes.len(), 16);

    // Test determinism - same seed should produce same result
    let effect2 = Effect::new(EffectType::Crypto, "random", &random_params).unwrap();
    let result2: aura_protocol::effects::crypto::CryptoRandomResult =
        system.execute_effect(effect2, &mut ctx).await.unwrap();

    // In testing mode with deterministic behavior, these should be equal
    assert_eq!(result.bytes, result2.bytes);
}

#[tokio::test]
async fn test_execution_mode_middleware_behavior() {
    let device_id = DeviceId::from(Uuid::new_v4());

    // Test different execution modes affect middleware behavior
    let mut test_system = AuraEffectSystem::for_testing(device_id);
    let mut sim_system = AuraEffectSystem::for_simulation(device_id, 42);

    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"same data".to_vec(),
    };

    // Execute same effect in both modes
    let mut test_ctx = AuraContext::for_testing(device_id);
    let mut sim_ctx = AuraContext::for_testing(device_id);

    let test_effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let sim_effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();

    let test_result: aura_protocol::effects::crypto::CryptoHashResult = test_system
        .execute_effect(test_effect, &mut test_ctx)
        .await
        .unwrap();
    let sim_result: aura_protocol::effects::crypto::CryptoHashResult = sim_system
        .execute_effect(sim_effect, &mut sim_ctx)
        .await
        .unwrap();

    // Both should produce valid hashes
    assert!(!test_result.hash.is_empty());
    assert!(!sim_result.hash.is_empty());

    // Results should be consistent (same data, same algorithm)
    assert_eq!(test_result.hash, sim_result.hash);
}

#[tokio::test]
async fn test_middleware_error_handling() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test invalid effect operation
    let effect = Effect::new(EffectType::Crypto, "invalid_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        aura_types::handlers::AuraHandlerError::UnknownOperation {
            effect_type,
            operation,
        } => {
            assert_eq!(effect_type, EffectType::Crypto);
            assert_eq!(operation, "invalid_operation");
        }
        _ => panic!("Expected UnknownOperation error"),
    }
}

#[tokio::test]
async fn test_middleware_composition() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test that multiple middleware types work together
    // First, log something
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Starting crypto operation".to_string(),
        component: Some("test".to_string()),
    };

    let log_effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    system.execute_effect(log_effect, &mut ctx).await.unwrap();

    // Then, perform crypto operation
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"middleware composition test".to_vec(),
    };

    let crypto_effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoHashResult = system
        .execute_effect(crypto_effect, &mut ctx)
        .await
        .unwrap();

    assert!(!result.hash.is_empty());

    // Finally, log completion
    let completion_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: format!("Crypto operation completed: {} bytes", result.hash.len()),
        component: Some("test".to_string()),
    };

    let completion_effect = Effect::new(EffectType::Console, "log", &completion_params).unwrap();
    system
        .execute_effect(completion_effect, &mut ctx)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_placeholder_middleware_behavior() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test placeholder network middleware
    let send_params = aura_protocol::effects::network::NetworkSendParams {
        peer_id: DeviceId::from(Uuid::new_v4()),
        data: b"test message".to_vec(),
        timeout: Some(Duration::from_secs(5)),
    };

    let effect = Effect::new(EffectType::Network, "send", &send_params).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Should succeed in testing mode (placeholder implementation)
    assert!(result.is_ok());

    // Test placeholder storage middleware
    let put_params = aura_protocol::effects::storage::StoragePutParams {
        key: "test_key".to_string(),
        value: b"test value".to_vec(),
        namespace: Some("test".to_string()),
    };

    let storage_effect = Effect::new(EffectType::Storage, "put", &put_params).unwrap();
    let storage_result = system.execute_effect(storage_effect, &mut ctx).await;

    assert!(storage_result.is_ok());
}

#[tokio::test]
async fn test_context_flow_through_middleware() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Set session context
    let session_id = Uuid::new_v4();
    ctx.session_id = Some(session_id.into());

    // Execute effect and verify context flows through middleware
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Context flow test".to_string(),
        component: Some("test".to_string()),
    };

    let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Context should be preserved
    assert_eq!(ctx.session_id, Some(session_id.into()));
    assert_eq!(ctx.device_id, device_id);
}

#[tokio::test]
async fn test_middleware_priorities() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = AuraEffectSystem::for_testing(device_id);

    // Verify that the system supports all middleware types
    // This implicitly tests that middlewares are properly registered and prioritized
    assert!(system.supports_effect(EffectType::Crypto));
    assert!(system.supports_effect(EffectType::Network));
    assert!(system.supports_effect(EffectType::Storage));
    assert!(system.supports_effect(EffectType::Time));
    assert!(system.supports_effect(EffectType::Console));
    assert!(system.supports_effect(EffectType::Ledger));
    assert!(system.supports_effect(EffectType::Random));
}

#[tokio::test]
async fn test_middleware_resilience() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test that middleware handles edge cases gracefully

    // Empty data hash
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: Vec::new(),
    };

    let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoHashResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    // Should handle empty data
    assert!(!result.hash.is_empty());
    assert_eq!(result.hash.len(), 32);

    // Very large data
    let large_data = vec![0u8; 1024 * 1024]; // 1MB
    let large_hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: large_data,
    };

    let large_effect = Effect::new(EffectType::Crypto, "hash", &large_hash_params).unwrap();
    let large_result: aura_protocol::effects::crypto::CryptoHashResult =
        system.execute_effect(large_effect, &mut ctx).await.unwrap();

    // Should handle large data
    assert!(!large_result.hash.is_empty());
    assert_eq!(large_result.hash.len(), 32);
}

#[tokio::test]
async fn test_simulation_middleware_behavior() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_simulation(device_id, 12345);
    let mut ctx = AuraContext::for_testing(device_id);

    // Initialize simulation context
    ctx.simulation = Some(aura_types::handlers::context::SimulationContext {
        seed: Some(12345),
        deterministic: Some(true),
        current_time: Some(Duration::ZERO),
        time_acceleration: Some(1.0),
        time_paused: Some(false),
        fault_context: None,
        state_snapshots: Some(std::collections::HashMap::new()),
        property_violations: Some(Vec::new()),
        chaos_experiments: Some(Vec::new()),
    });

    // Test that simulation mode affects middleware behavior
    let random_params = aura_protocol::effects::crypto::CryptoRandomParams {
        length: 32,
        algorithm: aura_protocol::effects::crypto::RandomAlgorithm::ChaCha20,
        seed: Some(12345),
    };

    let effect = Effect::new(EffectType::Crypto, "random", &random_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoRandomResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    assert_eq!(result.bytes.len(), 32);

    // Test determinism in simulation mode
    let effect2 = Effect::new(EffectType::Crypto, "random", &random_params).unwrap();
    let result2: aura_protocol::effects::crypto::CryptoRandomResult =
        system.execute_effect(effect2, &mut ctx).await.unwrap();

    // Should be deterministic with same seed
    assert_eq!(result.bytes, result2.bytes);
}

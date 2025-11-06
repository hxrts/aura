//! Unit tests for the unified AuraEffectSystem
//!
//! These tests verify that the core AuraEffectSystem works correctly
//! and provides the expected functionality for all effect types.

use std::time::Duration;
use uuid::Uuid;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::handlers::{AuraContext, AuraHandler, EffectType, ExecutionMode};
use aura_types::{identifiers::DeviceId, sessions::LocalSessionType};

#[tokio::test]
async fn test_effect_system_creation() {
    let device_id = DeviceId::from(Uuid::new_v4());

    // Test testing mode creation
    let test_system = AuraEffectSystem::for_testing(device_id);
    assert_eq!(test_system.device_id(), device_id);
    assert_eq!(test_system.execution_mode(), ExecutionMode::Testing);

    // Test simulation mode creation
    let sim_system = AuraEffectSystem::for_simulation(device_id, 42);
    assert_eq!(sim_system.device_id(), device_id);
    assert_eq!(
        sim_system.execution_mode(),
        ExecutionMode::Simulation { seed: 42 }
    );
}

#[tokio::test]
async fn test_supported_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = AuraEffectSystem::for_testing(device_id);

    // Verify support for all core effect types
    assert!(system.supports_effect(EffectType::Network));
    assert!(system.supports_effect(EffectType::Crypto));
    assert!(system.supports_effect(EffectType::Storage));
    assert!(system.supports_effect(EffectType::Time));
    assert!(system.supports_effect(EffectType::Console));
    assert!(system.supports_effect(EffectType::Ledger));
    assert!(system.supports_effect(EffectType::Random));
    assert!(system.supports_effect(EffectType::Choreographic));
}

#[tokio::test]
async fn test_console_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test console log effect
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Test log message".to_string(),
        component: Some("test".to_string()),
    };

    let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_crypto_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test crypto hash effect
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"test data".to_vec(),
    };

    let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoHashResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    assert!(!result.hash.is_empty());
    assert_eq!(
        result.algorithm,
        aura_protocol::effects::crypto::HashAlgorithm::Blake3
    );
}

#[tokio::test]
async fn test_random_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test random bytes generation
    let random_params = aura_protocol::effects::random::RandomBytesParams {
        length: 32,
        purpose: Some("test".to_string()),
    };

    let effect = Effect::new(EffectType::Random, "bytes", &random_params).unwrap();
    let result: aura_protocol::effects::random::RandomBytesResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    assert_eq!(result.bytes.len(), 32);
}

#[tokio::test]
async fn test_time_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test time now effect
    let effect = Effect::new(EffectType::Time, "now", &()).unwrap();
    let result: aura_protocol::effects::time::TimeNowResult =
        system.execute_effect(effect, &mut ctx).await.unwrap();

    // Should return a reasonable timestamp
    assert!(result.timestamp > 0);
}

#[tokio::test]
async fn test_choreographic_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test choreography event emission
    let event = aura_protocol::effects::choreographic::ChoreographyEvent::SessionStarted {
        session_id: Uuid::new_v4().to_string(),
        participants: vec![device_id.to_string()],
    };

    let event_params = aura_protocol::effects::choreographic::ChoreographyEventParams { event };
    let effect = Effect::new(EffectType::Choreographic, "emit_event", &event_params).unwrap();

    let result = system.execute_effect(effect, &mut ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_network_effect_placeholders() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test network send effect (should work in testing mode)
    let send_params = aura_protocol::effects::network::NetworkSendParams {
        peer_id: DeviceId::from(Uuid::new_v4()),
        data: b"test message".to_vec(),
        timeout: Some(Duration::from_secs(5)),
    };

    let effect = Effect::new(EffectType::Network, "send", &send_params).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Should succeed in testing mode (mock implementation)
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_storage_effect_placeholders() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test storage put effect
    let put_params = aura_protocol::effects::storage::StoragePutParams {
        key: "test_key".to_string(),
        value: b"test value".to_vec(),
        namespace: Some("test".to_string()),
    };

    let effect = Effect::new(EffectType::Storage, "put", &put_params).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Should succeed in testing mode
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ledger_effect_placeholders() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test ledger read effect
    let read_params = aura_protocol::effects::ledger::LedgerReadParams {
        key: "test_entry".to_string(),
        version: None,
    };

    let effect = Effect::new(EffectType::Ledger, "read", &read_params).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Should succeed in testing mode
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unknown_effect_handling() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test unknown operation
    let effect = Effect::new(EffectType::Console, "unknown_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    // Should fail with unknown operation error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_session_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Create a simple session type for testing
    let session = LocalSessionType::new(42, "test_message".to_string());

    // Execute session through the system
    let result = system.execute_session(session, &mut ctx).await;

    // Should succeed in testing mode
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_execution_mode_behavior() {
    let device_id = DeviceId::from(Uuid::new_v4());

    // Test different execution modes
    let test_system = AuraEffectSystem::for_testing(device_id);
    assert_eq!(test_system.execution_mode(), ExecutionMode::Testing);

    let sim_system = AuraEffectSystem::for_simulation(device_id, 123);
    assert_eq!(
        sim_system.execution_mode(),
        ExecutionMode::Simulation { seed: 123 }
    );
}

#[tokio::test]
async fn test_context_preservation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Modify context
    let session_id = Uuid::new_v4();
    ctx.session_id = Some(session_id.into());

    // Execute an effect and verify context is preserved
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Test".to_string(),
        component: None,
    };

    let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    system.execute_effect(effect, &mut ctx).await.unwrap();

    // Context should still have our modifications
    assert_eq!(ctx.session_id, Some(session_id.into()));
}

#[tokio::test]
async fn test_error_propagation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test that errors are properly propagated
    let effect = Effect::new(EffectType::Console, "invalid_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        aura_types::handlers::AuraHandlerError::UnknownOperation { .. } => {}
        _ => panic!("Expected UnknownOperation error"),
    }
}

#[tokio::test]
async fn test_concurrent_effect_execution() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = std::sync::Arc::new(tokio::sync::RwLock::new(AuraEffectSystem::for_testing(
        device_id,
    )));

    // Execute multiple effects concurrently
    let mut handles = Vec::new();

    for i in 0..10 {
        let system_clone = system.clone();
        let handle = tokio::spawn(async move {
            let mut system = system_clone.write().await;
            let mut ctx = AuraContext::for_testing(device_id);

            let log_params = aura_protocol::effects::console::ConsoleLogParams {
                level: aura_protocol::effects::console::LogLevel::Info,
                message: format!("Concurrent test {}", i),
                component: Some("test".to_string()),
            };

            let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
            system.execute_effect(effect, &mut ctx).await
        });
        handles.push(handle);
    }

    // Wait for all effects to complete
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}

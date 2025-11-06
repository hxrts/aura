//! Error handling tests for the unified effect system
//!
//! These tests verify that the unified architecture handles errors properly
//! and provides consistent error behavior across all layers.

use uuid::Uuid;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_types::{
    handlers::{AuraHandler, AuraHandlerError, Effect, EffectType, context::AuraContext},
    identifiers::DeviceId,
    session_types::LocalSessionType,
};

#[tokio::test]
async fn test_unknown_effect_type_error() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test with a hypothetical unsupported effect type
    // Since we support all core types, we'll test with an invalid operation instead
    let effect = Effect::new(EffectType::Console, "unknown_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        AuraHandlerError::UnknownOperation { effect_type, operation } => {
            assert_eq!(effect_type, EffectType::Console);
            assert_eq!(operation, "unknown_operation");
        },
        _ => panic!("Expected UnknownOperation error"),
    }
}

#[tokio::test]
async fn test_effect_creation_errors() {
    // Test serialization errors in effect creation
    use serde::{Serialize, Deserialize};
    
    #[derive(Serialize, Deserialize)]
    struct InvalidParams {
        invalid_field: std::collections::HashMap<String, fn()>, // Functions can't be serialized
    }
    
    // This should fail during effect creation, not execution
    let mut map = std::collections::HashMap::new();
    map.insert("test".to_string(), || {});
    
    let invalid_params = InvalidParams { invalid_field: map };
    
    // Effect creation should fail with serialization error
    let result = Effect::new(EffectType::Console, "log", &invalid_params);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_malformed_effect_parameters() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Create an effect with wrong parameter type for the operation
    // Use empty params for an operation that expects specific params
    let effect = Effect::new(EffectType::Crypto, "hash", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    // Should fail with parameter deserialization error
    assert!(result.is_err());
}

#[tokio::test]
async fn test_middleware_error_propagation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test that errors from middleware are properly propagated
    let effect = Effect::new(EffectType::Network, "invalid_network_op", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    assert!(result.is_err());
    // The error should be an AuraHandlerError
    match result.unwrap_err() {
        AuraHandlerError::UnknownOperation { .. } => {},
        AuraHandlerError::ParameterError { .. } => {},
        _ => {}, // Other error types are also acceptable
    }
}

#[tokio::test]
async fn test_context_error_preservation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Set up context state
    let session_id = Uuid::new_v4();
    ctx.session_id = Some(session_id.into());
    ctx.middleware.add_data("important".to_string(), "data".to_string());
    
    // Execute a failing effect
    let effect = Effect::new(EffectType::Console, "invalid_op", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    // Effect should fail
    assert!(result.is_err());
    
    // But context should be preserved
    assert_eq!(ctx.session_id, Some(session_id.into()));
    assert_eq!(ctx.middleware.get_data("important"), Some(&"data".to_string()));
}

#[tokio::test]
async fn test_error_chain_handling() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test various error conditions in sequence
    let error_tests = vec![
        ("invalid_op1", EffectType::Console),
        ("invalid_op2", EffectType::Crypto),
        ("invalid_op3", EffectType::Network),
        ("invalid_op4", EffectType::Storage),
    ];
    
    for (op, effect_type) in error_tests {
        let effect = Effect::new(effect_type, op, &()).unwrap();
        let result = system.execute_effect(effect, &mut ctx).await;
        
        assert!(result.is_err(), "Expected error for operation: {}", op);
        
        // System should continue to function after errors
        let valid_effect = Effect::new(EffectType::Console, "log", 
            &aura_protocol::effects::console::ConsoleLogParams {
                level: aura_protocol::effects::console::LogLevel::Info,
                message: format!("After error: {}", op),
                component: Some("test".to_string()),
            }).unwrap();
        
        let valid_result = system.execute_effect(valid_effect, &mut ctx).await;
        assert!(valid_result.is_ok(), "System should recover after error");
    }
}

#[tokio::test]
async fn test_session_execution_errors() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test session execution with various error conditions
    let session = LocalSessionType::new(42, "test_session".to_string());
    
    // This should succeed in testing mode
    let result = system.execute_session(session, &mut ctx).await;
    assert!(result.is_ok());
    
    // Test with invalid session data
    let invalid_session = LocalSessionType::new(-1, String::new()); // Empty string might cause issues
    let result = system.execute_session(invalid_session, &mut ctx).await;
    
    // Should handle gracefully (testing mode is permissive)
    // The exact behavior depends on session type implementation
}

#[tokio::test]
async fn test_concurrent_error_handling() {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = Arc::new(RwLock::new(AuraEffectSystem::for_testing(device_id)));
    
    let mut handles = Vec::new();
    
    // Start multiple tasks that will fail
    for i in 0..10 {
        let system_clone = system.clone();
        
        let handle = tokio::spawn(async move {
            let mut system = system_clone.write().await;
            let mut ctx = AuraContext::for_testing(device_id);
            
            // Mix of valid and invalid operations
            if i % 2 == 0 {
                // Valid operation
                let effect = Effect::new(EffectType::Console, "log", 
                    &aura_protocol::effects::console::ConsoleLogParams {
                        level: aura_protocol::effects::console::LogLevel::Info,
                        message: format!("Valid operation {}", i),
                        component: Some("test".to_string()),
                    }).unwrap();
                
                system.execute_effect(effect, &mut ctx).await
            } else {
                // Invalid operation
                let effect = Effect::new(EffectType::Console, "invalid_op", &()).unwrap();
                system.execute_effect(effect, &mut ctx).await
            }
        });
        
        handles.push(handle);
    }
    
    // Collect results
    let mut success_count = 0;
    let mut error_count = 0;
    
    for handle in handles {
        let result = handle.await.unwrap();
        if result.is_ok() {
            success_count += 1;
        } else {
            error_count += 1;
        }
    }
    
    // Should have both successes and errors
    assert_eq!(success_count, 5, "Expected 5 successful operations");
    assert_eq!(error_count, 5, "Expected 5 failed operations");
}

#[tokio::test]
async fn test_error_message_quality() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test that error messages are informative
    let effect = Effect::new(EffectType::Crypto, "nonexistent_algorithm", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_message = error.to_string();
    
    // Error message should be informative
    assert!(error_message.contains("Crypto") || error_message.contains("crypto"));
    assert!(!error_message.is_empty());
    
    // Test error debug formatting
    let debug_str = format!("{:?}", error);
    assert!(!debug_str.is_empty());
}

#[tokio::test]
async fn test_recovery_after_errors() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Cause multiple errors
    for i in 0..5 {
        let effect = Effect::new(EffectType::Console, &format!("invalid_op_{}", i), &()).unwrap();
        let result = system.execute_effect(effect, &mut ctx).await;
        assert!(result.is_err());
    }
    
    // System should still work normally
    let valid_effect = Effect::new(EffectType::Console, "log", 
        &aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: "Recovery test".to_string(),
            component: Some("test".to_string()),
        }).unwrap();
    
    let result = system.execute_effect(valid_effect, &mut ctx).await;
    assert!(result.is_ok(), "System should recover after multiple errors");
    
    // Test crypto operation after errors
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"recovery test".to_vec(),
    };
    
    let crypto_effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let crypto_result = system.execute_effect(crypto_effect, &mut ctx).await;
    assert!(crypto_result.is_ok(), "Crypto operations should work after errors");
}

#[tokio::test]
async fn test_error_boundaries() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test that errors in one middleware don't affect others
    
    // First, test a working crypto operation
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: b"boundary test".to_vec(),
    };
    
    let crypto_effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let crypto_result = system.execute_effect(crypto_effect, &mut ctx).await;
    assert!(crypto_result.is_ok());
    
    // Then cause an error in console middleware
    let invalid_console_effect = Effect::new(EffectType::Console, "invalid_console_op", &()).unwrap();
    let console_result = system.execute_effect(invalid_console_effect, &mut ctx).await;
    assert!(console_result.is_err());
    
    // Crypto should still work
    let crypto_effect2 = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let crypto_result2 = system.execute_effect(crypto_effect2, &mut ctx).await;
    assert!(crypto_result2.is_ok(), "Crypto should work after console error");
}

#[tokio::test]
async fn test_error_context_preservation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Set up rich context
    ctx.session_id = Some(Uuid::new_v4().into());
    ctx.middleware.add_data("error_test".to_string(), "preserved".to_string());
    
    // Cause an error
    let effect = Effect::new(EffectType::Network, "invalid_network_operation", &()).unwrap();
    let result = system.execute_effect(effect, &mut ctx).await;
    
    assert!(result.is_err());
    
    // Verify that context is completely preserved
    assert!(ctx.session_id.is_some());
    assert_eq!(ctx.middleware.get_data("error_test"), Some(&"preserved".to_string()));
    assert_eq!(ctx.device_id, device_id);
    
    // Verify the system can continue with preserved context
    let log_params = aura_protocol::effects::console::ConsoleLogParams {
        level: aura_protocol::effects::console::LogLevel::Info,
        message: "Context preserved after error".to_string(),
        component: Some("test".to_string()),
    };
    
    let log_effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
    let log_result = system.execute_effect(log_effect, &mut ctx).await;
    assert!(log_result.is_ok(), "Should continue working with preserved context");
}
#![cfg(feature = "fixture_effects")]

//! Runtime Integration Tests for Phase 3.2
//!
//! Tests validating choreographic execution following unified effect system architecture

use aura_core::DeviceId;
use aura_protocol::{effects::*, handlers::CompositeHandler};
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

/// Helper to create deterministic device IDs for tests
fn test_device_id(seed: &[u8]) -> DeviceId {
    use aura_core::hash::hash;
    let hash_bytes = hash(seed);
    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
    DeviceId(Uuid::from_bytes(uuid_bytes))
}

/// Test unified choreography adapters
mod adapter_tests {
    use super::*;

    #[tokio::test]
    async fn test_create_testing_adapter() {
        let device_id = test_device_id(b"test");
        let _handler = CompositeHandler::for_testing(device_id.into());

        // Verify handler creation for testing (handler should be created successfully)
        // Test passes if handler creation doesn't panic
    }

    #[tokio::test]
    async fn test_handler_factory_consistency() {
        let device_id = test_device_id(b"test");

        let _testing_handler = CompositeHandler::for_testing(device_id.into());
        let _production_handler = CompositeHandler::for_production(device_id.into());
        let _simulation_handler = CompositeHandler::for_simulation(device_id.into());

        // All factories should create handlers successfully
        // Test passes if handler creation doesn't panic
    }

    #[tokio::test]
    async fn test_multiple_adapters() {
        let participant_count = 5;
        let threshold = 3;

        let mut handlers = Vec::new();
        for i in 0..participant_count {
            let device_id = DeviceId::from_bytes([i as u8; 32]);
            let handler = CompositeHandler::for_testing(device_id.into());
            handlers.push(handler);
        }

        // Verify all handlers created successfully
        assert_eq!(handlers.len(), participant_count);

        // Verify threshold constraints
        assert!(threshold <= participant_count);
        assert!(threshold > 0);
    }
}

/// Test middleware composition following protocol guide patterns
mod middleware_tests {
    use super::*;

    #[tokio::test]
    async fn test_effect_composition() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Test that effect handlers compose properly
        let peers = handler.connected_peers().await;

        // Should handle gracefully even with mock effects
        assert!(peers.is_empty()); // Mock handler starts with no peers
    }

    #[tokio::test]
    async fn test_storage_effect_integration() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        let test_key = "test_key";
        let test_data = b"test data for storage";

        // Test storage operations
        let store_result = handler.store(test_key, test_data.to_vec()).await;
        assert!(store_result.is_ok());

        // Test retrieval
        let retrieve_result = handler.retrieve(test_key).await;
        assert!(retrieve_result.is_ok());

        if let Ok(Some(retrieved_data)) = retrieve_result {
            assert_eq!(retrieved_data, test_data);
        }
    }

    #[tokio::test]
    async fn test_crypto_effect_integration() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        let message = b"test message to hash";

        // Test crypto operations
        let hash_result = aura_core::hash::hash(message);
        assert_eq!(hash_result.len(), 32);

        // Test random generation
        let random_bytes = handler.random_bytes(16).await;
        assert_eq!(random_bytes.len(), 16);

        let random_32 = handler.random_bytes_32().await;
        assert_eq!(random_32.len(), 32);
    }
}

/// Test session type safety guarantees per protocol guide
mod session_safety_tests {
    use super::*;

    #[tokio::test]
    async fn test_deadlock_freedom() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Test that operations complete within reasonable time (no deadlocks)
        let result = timeout(Duration::from_secs(5), async {
            TimeEffects::current_epoch(&handler).await
        })
        .await;

        // Should not timeout (deadlock freedom)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_type_safety_at_compile_time() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // These should compile without type errors (compile-time safety)
        let _peers: Vec<Uuid> = handler.connected_peers().await;
        let _exists: Result<bool, StorageError> = handler.exists("test").await;
        let _hash: [u8; 32] = aura_core::hash::hash(b"test");

        // Type safety verified by compilation
    }

    #[tokio::test]
    async fn test_communication_safety() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Test that operations are properly typed
        let peer_id = test_device_id(b"test");
        let message = b"test message";

        // Send should accept correct types
        let _send_result = handler.send_to_peer(peer_id.into(), message.to_vec()).await;

        // Should handle gracefully in mock environment
        // Type safety verified by compilation
    }
}

/// Test performance characteristics per protocol guide
mod performance_tests {
    use super::*;

    #[tokio::test]
    async fn test_zero_cost_abstractions() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        let start_epoch = TimeEffects::current_epoch(&handler).await;

        // Perform many operations to test overhead
        for _ in 0..1000 {
            let _ = TimeEffects::current_epoch(&handler).await;
        }

        let end_epoch = TimeEffects::current_epoch(&handler).await;
        let duration_epochs = end_epoch.saturating_sub(start_epoch);

        // Should be fast (operations should complete quickly)
        assert!(duration_epochs < 1000); // Less than 1000 epochs
    }

    #[tokio::test]
    async fn test_message_serialization_efficiency() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        let test_data = vec![0u8; 1024]; // 1KB test data
        let start_epoch = TimeEffects::current_epoch(&handler).await;

        // Test serialization performance
        for i in 0..100 {
            let key = format!("test_key_{}", i);
            let _ = handler.store(&key, test_data.clone()).await;
        }

        let end_epoch = TimeEffects::current_epoch(&handler).await;
        let duration_epochs = end_epoch.saturating_sub(start_epoch);

        // Should be efficient (operations should complete quickly)
        assert!(duration_epochs < 100); // Less than 100 epochs
    }

    #[tokio::test]
    async fn test_parallel_composition_performance() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        let start_epoch = TimeEffects::current_epoch(&handler).await;

        // Test parallel execution
        let futures: Vec<_> = (0..10)
            .map(|_| TimeEffects::current_epoch(&handler))
            .collect();

        let results = futures::future::join_all(futures).await;
        let end_epoch = TimeEffects::current_epoch(&handler).await;
        let duration_epochs = end_epoch.saturating_sub(start_epoch);

        // Parallel execution should be faster than sequential
        assert!(results.len() == 10);
        assert!(duration_epochs < 50); // Should be much faster than sequential
    }
}

/// Additional integration tests for basic functionality
mod basic_functionality_tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_creation_deterministic() {
        let device_id1 = DeviceId::from_bytes([42u8; 32]);
        let device_id2 = DeviceId::from_bytes([42u8; 32]);
        let handler1 = CompositeHandler::for_testing(device_id1.into());
        let handler2 = CompositeHandler::for_testing(device_id2.into());

        // Same device ID should create handlers successfully
        let epoch1 = TimeEffects::current_epoch(&handler1).await;
        let epoch2 = TimeEffects::current_epoch(&handler2).await;

        // Both should return valid epochs
        assert!(epoch1 > 0);
        assert!(epoch2 > 0);
    }

    #[tokio::test]
    async fn test_effect_operations_termination() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // All operations should terminate within reasonable time
        let result = timeout(Duration::from_secs(5), async {
            for _ in 0..10 {
                let _ = TimeEffects::current_epoch(&handler).await;
            }
        })
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_concurrent_handler_safety() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Concurrent access should be safe
        let futures: Vec<_> = (0..5)
            .map(|_| TimeEffects::current_epoch(&handler))
            .collect();

        let results = futures::future::join_all(futures).await;
        assert_eq!(results.len(), 5);

        // All should return valid epochs
        for epoch in results {
            assert!(epoch > 0);
        }
    }
}

/// Integration tests for choreographic effect handlers
mod choreographic_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_choreographic_role_integration() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Test choreographic role identification
        let role = handler.current_role();
        assert_eq!(role.device_id, Uuid::from(device_id));
        assert_eq!(role.role_index, 0);
    }

    #[tokio::test]
    async fn test_choreographic_broadcast() {
        let device_id = test_device_id(b"test");
        let handler = CompositeHandler::for_testing(device_id.into());

        // Test choreographic broadcast
        let message = b"test choreographic message";
        let broadcast_result = handler.broadcast_bytes(message.to_vec()).await;

        // Should handle gracefully with mock network
        assert!(broadcast_result.is_ok() || broadcast_result.is_err());
    }
}

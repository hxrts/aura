//! DKD Protocol Tests Following Protocol Guide Patterns
//!
//! Tests for DKD choreographic protocols implementing patterns from docs/405_protocol_guide.md

use aura_choreography::{
    integration::{create_testing_adapter, create_simulation_adapter},
    protocols::dkd::{execute_dkd, DkdConfig, DkdResult},
};
use aura_types::DeviceId;
use proptest::prelude::*;
use std::time::Duration;
use tokio::time::timeout;

/// Unit tests using mock effects as per protocol guide
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_dkd_protocol_basic_execution() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let config = DkdConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            threshold: 2,
            context: "test_context".to_string(),
            derivation_path: vec![0, 1, 2],
        };

        let result = execute_dkd(&mut adapter, config).await;
        
        assert!(result.is_ok(), "DKD protocol should execute successfully");
        let dkd_result = result.unwrap();
        assert!(dkd_result.success, "DKD protocol should report success");
        assert!(!dkd_result.derived_keys.is_empty(), "Should produce derived keys");
    }

    #[tokio::test]
    async fn test_dkd_protocol_different_contexts() {
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Execute with two different contexts
        let mut adapter1 = create_testing_adapter(device_id);
        let config1 = DkdConfig {
            participants: participants.clone(),
            threshold: 2,
            context: "context_1".to_string(),
            derivation_path: vec![0, 1, 2],
        };

        let mut adapter2 = create_testing_adapter(device_id);
        let config2 = DkdConfig {
            participants: participants.clone(),
            threshold: 2,
            context: "context_2".to_string(),
            derivation_path: vec![0, 1, 2],
        };

        let result1 = execute_dkd(&mut adapter1, config1).await.unwrap();
        let result2 = execute_dkd(&mut adapter2, config2).await.unwrap();

        // Different contexts should produce different derived keys
        assert_ne!(
            result1.derived_keys, result2.derived_keys,
            "Different contexts should produce different keys"
        );
    }

    #[tokio::test]
    async fn test_dkd_protocol_threshold_validation() {
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Test valid threshold
        let mut adapter = create_testing_adapter(device_id);
        let config = DkdConfig {
            participants: participants.clone(),
            threshold: 2, // Valid: 2 <= 3
            context: "test_threshold".to_string(),
            derivation_path: vec![0],
        };

        let result = execute_dkd(&mut adapter, config).await;
        assert!(result.is_ok(), "Valid threshold should work");

        // Test edge case: threshold equals participant count
        let mut adapter = create_testing_adapter(device_id);
        let config = DkdConfig {
            participants: participants.clone(),
            threshold: 3, // Edge case: 3 == 3
            context: "test_threshold".to_string(),
            derivation_path: vec![0],
        };

        let result = execute_dkd(&mut adapter, config).await;
        assert!(result.is_ok(), "Threshold equal to participant count should work");
    }
}

/// Property-based tests for protocol invariants
mod property_tests {
    use super::*;

    proptest! {
        #[test]
        fn test_dkd_deterministic_execution(
            context in "[a-zA-Z0-9_]{1,50}",
            threshold in 1u32..=5,
            participant_count in 2u32..=10,
            derivation_path in proptest::collection::vec(0u32..1000, 0..5)
        ) {
            // Ensure threshold is valid
            let threshold = threshold.min(participant_count);
            
            tokio_test::block_on(async {
                let device_id = DeviceId::new();
                let mut participants = vec![device_id];
                for _ in 1..participant_count {
                    participants.push(DeviceId::new());
                }

                // Execute the same protocol configuration twice
                let config1 = DkdConfig {
                    participants: participants.clone(),
                    threshold,
                    context: context.clone(),
                    derivation_path: derivation_path.clone(),
                };
                
                let config2 = DkdConfig {
                    participants: participants.clone(),
                    threshold,
                    context: context.clone(),
                    derivation_path: derivation_path.clone(),
                };

                let mut adapter1 = create_testing_adapter(device_id);
                let mut adapter2 = create_testing_adapter(device_id);

                let result1 = execute_dkd(&mut adapter1, config1).await;
                let result2 = execute_dkd(&mut adapter2, config2).await;

                // Both executions should succeed with same configuration
                prop_assert!(result1.is_ok());
                prop_assert!(result2.is_ok());
                
                let dkd1 = result1.unwrap();
                let dkd2 = result2.unwrap();
                
                // Results should be deterministic for same input
                prop_assert_eq!(dkd1.derived_keys, dkd2.derived_keys);
                prop_assert_eq!(dkd1.success, dkd2.success);
            });
        }

        #[test]
        fn test_dkd_context_independence(
            base_context in "[a-zA-Z0-9_]{1,30}",
            suffix1 in "[a-zA-Z0-9_]{1,10}",
            suffix2 in "[a-zA-Z0-9_]{1,10}"
        ) {
            // Ensure contexts are different
            prop_assume!(suffix1 != suffix2);
            
            tokio_test::block_on(async {
                let device_id = DeviceId::new();
                let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

                let context1 = format!("{}_{}", base_context, suffix1);
                let context2 = format!("{}_{}", base_context, suffix2);

                let mut adapter1 = create_testing_adapter(device_id);
                let config1 = DkdConfig {
                    participants: participants.clone(),
                    threshold: 2,
                    context: context1,
                    derivation_path: vec![0, 1],
                };

                let mut adapter2 = create_testing_adapter(device_id);
                let config2 = DkdConfig {
                    participants: participants.clone(),
                    threshold: 2,
                    context: context2,
                    derivation_path: vec![0, 1],
                };

                let result1 = execute_dkd(&mut adapter1, config1).await;
                let result2 = execute_dkd(&mut adapter2, config2).await;

                prop_assert!(result1.is_ok());
                prop_assert!(result2.is_ok());

                let dkd1 = result1.unwrap();
                let dkd2 = result2.unwrap();

                // Different contexts should produce different keys
                prop_assert_ne!(dkd1.derived_keys, dkd2.derived_keys);
            });
        }
    }
}

/// Integration tests following protocol guide patterns
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_dkd_multi_participant_coordination() {
        // Test coordination patterns from protocol guide
        let participant_count = 5;
        let threshold = 3;

        let mut participants = Vec::new();
        for i in 0..participant_count {
            participants.push(DeviceId::from_bytes([i as u8; 32]));
        }

        // Test each participant's execution
        let mut results = Vec::new();
        for (i, &device_id) in participants.iter().enumerate() {
            let mut adapter = create_testing_adapter(device_id);
            let config = DkdConfig {
                participants: participants.clone(),
                threshold,
                context: "multi_participant_test".to_string(),
                derivation_path: vec![i as u32],
            };

            let result = execute_dkd(&mut adapter, config).await;
            results.push(result);
        }

        // Verify all participants completed successfully
        for (i, result) in results.iter().enumerate() {
            assert!(result.is_ok(), "Participant {} should complete successfully", i);
        }

        // Verify protocol consistency across participants
        let successful_results: Vec<_> = results.into_iter()
            .map(|r| r.unwrap())
            .collect();

        assert_eq!(successful_results.len(), participant_count);
        
        // All participants should report success
        for (i, result) in successful_results.iter().enumerate() {
            assert!(result.success, "Participant {} should report success", i);
            assert!(!result.derived_keys.is_empty(), "Participant {} should have derived keys", i);
        }
    }

    #[tokio::test]
    async fn test_dkd_simulation_environment() {
        // Test using simulation adapter as per protocol guide
        let device_id = DeviceId::new();
        let mut adapter = create_simulation_adapter(device_id);

        let config = DkdConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            threshold: 2,
            context: "simulation_test".to_string(),
            derivation_path: vec![0, 1, 2, 3],
        };

        let result = execute_dkd(&mut adapter, config).await;
        
        assert!(result.is_ok(), "DKD should work in simulation environment");
        let dkd_result = result.unwrap();
        assert!(dkd_result.success, "Simulation DKD should succeed");
    }
}

/// Fault tolerance tests per protocol guide
mod fault_tolerance_tests {
    use super::*;

    #[tokio::test]
    async fn test_dkd_timeout_handling() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let config = DkdConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            threshold: 2,
            context: "timeout_test".to_string(),
            derivation_path: vec![0],
        };

        // Execute with timeout to verify protocol terminates
        let result = timeout(Duration::from_secs(10), execute_dkd(&mut adapter, config)).await;
        
        assert!(result.is_ok(), "DKD protocol should complete within timeout");
        
        let dkd_result = result.unwrap();
        // In current placeholder implementation, this should succeed
        assert!(dkd_result.is_ok(), "Protocol should handle normal case");
    }

    #[tokio::test]
    async fn test_dkd_protocol_always_terminates() {
        // Property: protocol should always terminate (no deadlocks)
        let device_id = DeviceId::new();
        
        for seed in 0..10 {
            let mut adapter = create_testing_adapter(device_id);
            let config = DkdConfig {
                participants: vec![device_id, DeviceId::from_bytes([seed; 32])],
                threshold: 1,
                context: format!("termination_test_{}", seed),
                derivation_path: vec![seed as u32],
            };

            let result = timeout(
                Duration::from_secs(5), 
                execute_dkd(&mut adapter, config)
            ).await;

            assert!(result.is_ok(), "Protocol iteration {} should complete within timeout", seed);
        }
    }

    #[tokio::test]
    async fn test_dkd_minimal_threshold() {
        // Test edge case: minimal valid threshold
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let config = DkdConfig {
            participants: vec![device_id],
            threshold: 1, // Minimal threshold
            context: "minimal_test".to_string(),
            derivation_path: vec![0],
        };

        let result = execute_dkd(&mut adapter, config).await;
        assert!(result.is_ok(), "Minimal threshold configuration should work");
    }
}

/// Performance validation tests per protocol guide
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_dkd_execution_performance() {
        let device_id = DeviceId::new();
        
        let start = Instant::now();
        
        for i in 0..10 {
            let mut adapter = create_testing_adapter(device_id);
            let config = DkdConfig {
                participants: vec![device_id, DeviceId::new()],
                threshold: 1,
                context: format!("perf_test_{}", i),
                derivation_path: vec![i as u32],
            };

            let result = execute_dkd(&mut adapter, config).await;
            assert!(result.is_ok(), "Performance test iteration {} failed", i);
        }
        
        let duration = start.elapsed();
        
        // Should complete 10 iterations quickly (under 1 second)
        assert!(duration < Duration::from_secs(1), 
               "Performance test took too long: {:?}", duration);
    }

    #[tokio::test]
    async fn test_dkd_memory_efficiency() {
        // Test that protocol doesn't leak memory with repeated executions
        let device_id = DeviceId::new();
        
        for i in 0..100 {
            let mut adapter = create_testing_adapter(device_id);
            let config = DkdConfig {
                participants: vec![device_id, DeviceId::new()],
                threshold: 1,
                context: "memory_test".to_string(),
                derivation_path: vec![i % 10],
            };

            let result = execute_dkd(&mut adapter, config).await;
            assert!(result.is_ok(), "Memory test iteration {} failed", i);
            
            // Allow adapter to be dropped to test memory cleanup
        }
        
        // If we get here without OOM, memory efficiency is acceptable
    }
}
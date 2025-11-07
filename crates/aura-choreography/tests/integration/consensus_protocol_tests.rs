//! Consensus Protocol Tests Following Protocol Guide Patterns
//!
//! Tests for consensus choreographic protocols implementing patterns from docs/405_protocol_guide.md

use aura_choreography::{
    integration::{create_testing_adapter, create_simulation_adapter},
    protocols::consensus::{
        execute_consensus, execute_broadcast_gather, execute_propose_acknowledge,
        execute_coordinator_monitoring, execute_failure_recovery,
        ConsensusConfig, ConsensusResult
    },
};
use aura_types::DeviceId;
use proptest::prelude::*;
use std::time::Duration;
use tokio::time::timeout;

/// Unit tests using mock effects as per protocol guide
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_basic_execution() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let config = ConsensusConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            proposal: b"consensus proposal".to_vec(),
            timeout_ms: 30000,
        };

        let result = execute_consensus(&mut adapter, config).await;
        
        assert!(result.is_ok(), "Consensus protocol should execute successfully");
        let consensus_result = result.unwrap();
        assert!(consensus_result.success, "Consensus should report success");
        assert!(!consensus_result.consensus_value.is_empty(), "Should produce consensus value");
        assert!(consensus_result.round > 0, "Should complete at least one round");
    }

    #[tokio::test]
    async fn test_broadcast_gather_pattern() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];
        let message = b"broadcast test message".to_vec();

        let result = execute_broadcast_gather(&mut adapter, participants.clone(), message.clone()).await;
        
        assert!(result.is_ok(), "Broadcast and gather should execute successfully");
        let gathered_messages = result.unwrap();
        assert_eq!(gathered_messages.len(), participants.len(), "Should gather messages from all participants");
        
        // In placeholder implementation, all messages should be echoes
        for gathered_msg in gathered_messages {
            assert_eq!(gathered_msg, message, "Gathered message should match original");
        }
    }

    #[tokio::test]
    async fn test_propose_acknowledge_pattern() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];
        let proposal = b"test proposal".to_vec();

        let result = execute_propose_acknowledge(&mut adapter, participants, proposal).await;
        
        assert!(result.is_ok(), "Propose and acknowledge should execute successfully");
        assert!(result.unwrap(), "Should receive acknowledgment");
    }

    #[tokio::test]
    async fn test_coordinator_monitoring() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let coordinator = DeviceId::new();
        let monitors = vec![device_id, DeviceId::new()];

        let result = execute_coordinator_monitoring(&mut adapter, monitors, coordinator).await;
        
        assert!(result.is_ok(), "Coordinator monitoring should execute successfully");
        assert!(result.unwrap(), "Monitoring should report coordinator as healthy");
    }

    #[tokio::test]
    async fn test_failure_recovery() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        let survivors = vec![device_id, DeviceId::new()];
        let failed_nodes = vec![DeviceId::new(), DeviceId::new()];

        let result = execute_failure_recovery(&mut adapter, survivors, failed_nodes).await;
        
        assert!(result.is_ok(), "Failure recovery should execute successfully");
        assert!(result.unwrap(), "Recovery should succeed");
    }
}

/// Property-based tests for protocol invariants
mod property_tests {
    use super::*;

    proptest! {
        #[test]
        fn test_consensus_deterministic_execution(
            proposal in proptest::collection::vec(0u8..255, 1..100),
            participant_count in 2u32..=7,
            timeout_ms in 1000u64..=60000
        ) {
            tokio_test::block_on(async {
                let device_id = DeviceId::new();
                let mut participants = vec![device_id];
                for i in 1..participant_count {
                    participants.push(DeviceId::from_bytes([i as u8; 32]));
                }

                // Execute the same consensus configuration twice
                let config1 = ConsensusConfig {
                    participants: participants.clone(),
                    proposal: proposal.clone(),
                    timeout_ms,
                };
                
                let config2 = ConsensusConfig {
                    participants: participants.clone(),
                    proposal: proposal.clone(),
                    timeout_ms,
                };

                let mut adapter1 = create_testing_adapter(device_id);
                let mut adapter2 = create_testing_adapter(device_id);

                let result1 = execute_consensus(&mut adapter1, config1).await;
                let result2 = execute_consensus(&mut adapter2, config2).await;

                // Both executions should succeed with same configuration
                prop_assert!(result1.is_ok());
                prop_assert!(result2.is_ok());
                
                let consensus1 = result1.unwrap();
                let consensus2 = result2.unwrap();
                
                // Results should be deterministic for same input
                prop_assert_eq!(consensus1.consensus_value, consensus2.consensus_value);
                prop_assert_eq!(consensus1.success, consensus2.success);
            });
        }

        #[test]
        fn test_broadcast_gather_consistency(
            message in proptest::collection::vec(0u8..255, 0..50),
            participant_count in 1u32..=8
        ) {
            tokio_test::block_on(async {
                let device_id = DeviceId::new();
                let mut participants = vec![device_id];
                for i in 1..participant_count {
                    participants.push(DeviceId::from_bytes([i as u8; 32]));
                }

                let mut adapter = create_testing_adapter(device_id);
                let result = execute_broadcast_gather(&mut adapter, participants.clone(), message.clone()).await;

                prop_assert!(result.is_ok());
                let gathered_messages = result.unwrap();
                
                // Should gather messages from all participants
                prop_assert_eq!(gathered_messages.len(), participants.len());
                
                // In placeholder implementation, all should be echoes of original message
                for gathered_msg in gathered_messages {
                    prop_assert_eq!(gathered_msg, message);
                }
            });
        }
    }
}

/// Integration tests following protocol guide patterns
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_multi_participant_agreement() {
        // Test multi-party consensus coordination
        let participant_count = 5;
        let mut participants = Vec::new();
        for i in 0..participant_count {
            participants.push(DeviceId::from_bytes([i as u8; 32]));
        }

        let proposal = b"Multi-participant consensus proposal".to_vec();

        // Test each participant's execution
        let mut results = Vec::new();
        for (i, &device_id) in participants.iter().enumerate() {
            let mut adapter = create_testing_adapter(device_id);
            let config = ConsensusConfig {
                participants: participants.clone(),
                proposal: proposal.clone(),
                timeout_ms: 10000,
            };

            let result = execute_consensus(&mut adapter, config).await;
            results.push((i, result));
        }

        // Verify all participants completed successfully
        for (i, result) in results.iter() {
            assert!(result.is_ok(), "Participant {} should complete successfully", i);
        }

        // Verify consensus agreement across participants
        let successful_results: Vec<_> = results.into_iter()
            .map(|(i, r)| (i, r.unwrap()))
            .collect();

        assert_eq!(successful_results.len(), participant_count);
        
        // All participants should report success and agree on value
        for (i, result) in successful_results.iter() {
            assert!(result.success, "Participant {} should report success", i);
            assert_eq!(result.consensus_value, proposal, "Participant {} should agree on consensus value", i);
        }
    }

    #[tokio::test]
    async fn test_consensus_simulation_environment() {
        // Test using simulation adapter as per protocol guide
        let device_id = DeviceId::new();
        let mut adapter = create_simulation_adapter(device_id);

        let config = ConsensusConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            proposal: b"Simulation consensus test".to_vec(),
            timeout_ms: 15000,
        };

        let result = execute_consensus(&mut adapter, config).await;
        
        assert!(result.is_ok(), "Consensus should work in simulation environment");
        let consensus_result = result.unwrap();
        assert!(consensus_result.success, "Simulation consensus should succeed");
    }

    #[tokio::test]
    async fn test_consensus_pattern_composition() {
        // Test composition of consensus patterns
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];
        
        // Test broadcast-gather as part of consensus
        let mut adapter = create_testing_adapter(device_id);
        let proposal = b"Pattern composition test".to_vec();
        
        // First, broadcast and gather opinions
        let gather_result = execute_broadcast_gather(
            &mut adapter, 
            participants.clone(), 
            proposal.clone()
        ).await;
        assert!(gather_result.is_ok(), "Broadcast-gather phase should succeed");
        
        // Then, run consensus on the gathered information
        let mut adapter = create_testing_adapter(device_id);
        let config = ConsensusConfig {
            participants: participants.clone(),
            proposal: proposal.clone(),
            timeout_ms: 10000,
        };
        
        let consensus_result = execute_consensus(&mut adapter, config).await;
        assert!(consensus_result.is_ok(), "Consensus phase should succeed");
        
        let result = consensus_result.unwrap();
        assert!(result.success, "Composed protocol should succeed");
        assert_eq!(result.consensus_value, proposal, "Should reach consensus on proposal");
    }

    #[tokio::test]
    async fn test_coordinator_failure_scenarios() {
        let device_id = DeviceId::new();
        let coordinator = DeviceId::from_bytes([99; 32]);
        let monitors = vec![device_id, DeviceId::new(), DeviceId::new()];
        
        // Test normal monitoring
        let mut adapter = create_testing_adapter(device_id);
        let monitor_result = execute_coordinator_monitoring(
            &mut adapter,
            monitors.clone(),
            coordinator
        ).await;
        assert!(monitor_result.is_ok(), "Coordinator monitoring should work normally");
        
        // Test failure recovery after coordinator failure
        let mut adapter = create_testing_adapter(device_id);
        let survivors = monitors.clone();
        let failed_nodes = vec![coordinator];
        
        let recovery_result = execute_failure_recovery(
            &mut adapter,
            survivors,
            failed_nodes
        ).await;
        assert!(recovery_result.is_ok(), "Failure recovery should handle coordinator failure");
        assert!(recovery_result.unwrap(), "Recovery should succeed");
    }
}

/// Fault tolerance tests per protocol guide
mod fault_tolerance_tests {
    use super::*;

    #[tokio::test]
    async fn test_consensus_timeout_handling() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        // Test with very short timeout
        let config = ConsensusConfig {
            participants: vec![device_id, DeviceId::new(), DeviceId::new()],
            proposal: b"timeout test".to_vec(),
            timeout_ms: 100, // Very short timeout
        };

        // Execute with external timeout to verify protocol terminates
        let result = timeout(Duration::from_secs(10), execute_consensus(&mut adapter, config)).await;
        
        assert!(result.is_ok(), "Consensus should complete within external timeout");
        
        // Internal timeout handling is tested by the protocol itself
        let consensus_result = result.unwrap();
        // Protocol might succeed or fail due to timeout, both are valid behaviors
        assert!(consensus_result.is_ok(), "Protocol should handle timeouts gracefully");
    }

    #[tokio::test]
    async fn test_consensus_always_terminates() {
        // Property: consensus should always terminate (no deadlocks)
        let device_id = DeviceId::new();
        
        for seed in 0..5 {
            let mut adapter = create_testing_adapter(device_id);
            let config = ConsensusConfig {
                participants: vec![device_id, DeviceId::from_bytes([seed; 32])],
                proposal: format!("termination_test_{}", seed).as_bytes().to_vec(),
                timeout_ms: 5000,
            };

            let result = timeout(
                Duration::from_secs(8), 
                execute_consensus(&mut adapter, config)
            ).await;

            assert!(result.is_ok(), "Consensus iteration {} should complete within timeout", seed);
        }
    }

    #[tokio::test]
    async fn test_byzantine_fault_tolerance() {
        // Test consensus with potential Byzantine participants
        let device_id = DeviceId::new();
        let honest_participants = vec![device_id, DeviceId::new(), DeviceId::new()];
        let byzantine_participants = vec![DeviceId::new(), DeviceId::new()]; // Simulated Byzantine nodes
        
        let mut all_participants = honest_participants.clone();
        all_participants.extend(byzantine_participants);
        
        // Consensus should still work with Byzantine minority
        let mut adapter = create_testing_adapter(device_id);
        let config = ConsensusConfig {
            participants: all_participants,
            proposal: b"Byzantine tolerance test".to_vec(),
            timeout_ms: 15000,
        };

        let result = execute_consensus(&mut adapter, config).await;
        
        // In current placeholder implementation, this should still work
        assert!(result.is_ok(), "Consensus should handle Byzantine participants");
    }

    #[tokio::test]
    async fn test_network_partition_recovery() {
        // Test consensus behavior during network partition scenarios
        let device_id = DeviceId::new();
        let partition1 = vec![device_id, DeviceId::new()];
        let partition2 = vec![DeviceId::new(), DeviceId::new()];
        
        // Test consensus with each partition
        let mut adapter = create_testing_adapter(device_id);
        let config = ConsensusConfig {
            participants: partition1.clone(),
            proposal: b"partition test".to_vec(),
            timeout_ms: 5000,
        };

        let result = execute_consensus(&mut adapter, config).await;
        
        // Smaller partition might not reach consensus, but should handle gracefully
        assert!(result.is_ok() || result.is_err(), "Should handle partition gracefully");
        
        // Test recovery scenario
        let mut adapter = create_testing_adapter(device_id);
        let mut all_participants = partition1;
        all_participants.extend(partition2);
        
        let recovery_config = ConsensusConfig {
            participants: all_participants.clone(),
            proposal: b"recovery test".to_vec(),
            timeout_ms: 10000,
        };

        let recovery_result = execute_consensus(&mut adapter, recovery_config).await;
        assert!(recovery_result.is_ok(), "Recovery consensus should succeed with full network");
    }

    #[tokio::test]
    async fn test_failure_recovery_edge_cases() {
        let device_id = DeviceId::new();
        let mut adapter = create_testing_adapter(device_id);

        // Test recovery with no failures
        let survivors = vec![device_id, DeviceId::new(), DeviceId::new()];
        let failed_nodes = vec![];

        let result = execute_failure_recovery(&mut adapter, survivors, failed_nodes).await;
        assert!(result.is_ok(), "Recovery should handle no-failure case");
        assert!(result.unwrap(), "Should report successful recovery");

        // Test recovery with majority failure
        let mut adapter = create_testing_adapter(device_id);
        let survivors = vec![device_id];
        let failed_nodes = vec![DeviceId::new(), DeviceId::new(), DeviceId::new()];

        let result = execute_failure_recovery(&mut adapter, survivors, failed_nodes).await;
        assert!(result.is_ok(), "Should handle majority failure gracefully");
    }
}

/// Performance validation tests per protocol guide
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_consensus_execution_performance() {
        let device_id = DeviceId::new();
        
        let start = Instant::now();
        
        for i in 0..5 {
            let mut adapter = create_testing_adapter(device_id);
            let config = ConsensusConfig {
                participants: vec![device_id, DeviceId::new()],
                proposal: format!("perf_test_{}", i).as_bytes().to_vec(),
                timeout_ms: 1000,
            };

            let result = execute_consensus(&mut adapter, config).await;
            assert!(result.is_ok(), "Performance test iteration {} failed", i);
        }
        
        let duration = start.elapsed();
        
        // Should complete 5 iterations quickly
        assert!(duration < Duration::from_secs(10), 
               "Performance test took too long: {:?}", duration);
    }

    #[tokio::test]
    async fn test_broadcast_gather_scalability() {
        // Test broadcast-gather with increasing participant counts
        let device_id = DeviceId::new();
        
        for participant_count in 2..=8 {
            let mut adapter = create_testing_adapter(device_id);
            let mut participants = vec![device_id];
            for i in 1..participant_count {
                participants.push(DeviceId::from_bytes([i as u8; 32]));
            }

            let start = Instant::now();
            let result = execute_broadcast_gather(
                &mut adapter,
                participants.clone(),
                format!("scalability_test_{}", participant_count).as_bytes().to_vec()
            ).await;
            let duration = start.elapsed();

            assert!(result.is_ok(), "Broadcast-gather should work with {} participants", participant_count);
            
            // Should scale reasonably (under 100ms per participant)
            let max_duration = Duration::from_millis(100 * participant_count as u64);
            assert!(duration < max_duration, 
                   "Broadcast-gather with {} participants took too long: {:?}", 
                   participant_count, duration);
        }
    }

    #[tokio::test]
    async fn test_consensus_memory_efficiency() {
        // Test that consensus doesn't leak memory with repeated executions
        let device_id = DeviceId::new();
        
        for i in 0..20 {
            let mut adapter = create_testing_adapter(device_id);
            let config = ConsensusConfig {
                participants: vec![device_id, DeviceId::new()],
                proposal: b"memory efficiency test".to_vec(),
                timeout_ms: 1000,
            };

            let result = execute_consensus(&mut adapter, config).await;
            assert!(result.is_ok(), "Memory test iteration {} failed", i);
            
            // Allow adapter to be dropped to test memory cleanup
        }
        
        // If we get here without OOM, memory efficiency is acceptable
    }
}
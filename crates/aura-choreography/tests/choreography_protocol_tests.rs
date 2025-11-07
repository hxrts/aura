//! Choreography Protocol Test Suite Following Protocol Guide
//!
//! Comprehensive test suite for all choreographic protocols implementing
//! patterns from docs/405_protocol_guide.md

use aura_choreography::{
    integration::{create_production_adapter, create_simulation_adapter, create_testing_adapter},
    protocols::{
        consensus::{execute_consensus, ConsensusConfig},
        dkd::{execute_dkd, DkdConfig},
        frost::{execute_frost_signing, FrostConfig},
    },
};
use aura_types::DeviceId;
use proptest::prelude::*;
use std::time::Duration;
use tokio::time::timeout;

/// Test adapter factory patterns per protocol guide
mod adapter_factory_tests {
    use super::*;

    #[tokio::test]
    async fn test_all_adapter_factories() {
        let device_id = DeviceId::new();

        // Test all factory methods work
        let testing_adapter = create_testing_adapter(device_id);
        let simulation_adapter = create_simulation_adapter(device_id);
        let production_adapter = create_production_adapter(device_id);

        assert_eq!(testing_adapter.device_id, device_id);
        assert_eq!(simulation_adapter.device_id, device_id);
        assert_eq!(production_adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_adapter_factory_consistency() {
        let device_id = DeviceId::new();

        // Multiple calls should produce consistent adapters
        for _ in 0..10 {
            let adapter1 = create_testing_adapter(device_id);
            let adapter2 = create_testing_adapter(device_id);

            assert_eq!(adapter1.device_id, adapter2.device_id);
        }
    }
}

/// Cross-protocol integration tests per protocol guide
mod cross_protocol_tests {
    use super::*;

    #[tokio::test]
    async fn test_dkd_then_frost_workflow() {
        // Test workflow: DKD key derivation followed by FROST signing
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Step 1: Derive keys using DKD
        let mut dkd_adapter = create_testing_adapter(device_id);
        let dkd_config = DkdConfig {
            participants: participants.clone(),
            threshold: 2,
            context: "workflow_keys".to_string(),
            derivation_path: vec![0, 1],
        };

        let dkd_result = execute_dkd(&mut dkd_adapter, dkd_config).await;
        assert!(dkd_result.is_ok(), "DKD step should succeed");
        let derived_keys = dkd_result.unwrap();
        assert!(derived_keys.success, "DKD should derive keys successfully");

        // Step 2: Use derived context for FROST signing
        let mut frost_adapter = create_testing_adapter(device_id);
        let frost_config = FrostConfig {
            participants: participants.clone(),
            threshold: 2,
            message: b"Message to sign with derived keys".to_vec(),
            signing_package: derived_keys.derived_keys, // Use derived keys as signing package
        };

        let frost_result = execute_frost_signing(&mut frost_adapter, frost_config).await;
        assert!(frost_result.is_ok(), "FROST step should succeed");
        let signature_result = frost_result.unwrap();
        assert!(signature_result.success, "FROST signing should succeed");

        // Verify the workflow produced valid results
        assert!(
            !signature_result.signature.is_empty(),
            "Should produce signature"
        );
        assert!(
            !signature_result.public_key.is_empty(),
            "Should have public key"
        );
    }

    #[tokio::test]
    async fn test_consensus_then_frost_workflow() {
        // Test workflow: Consensus on message, then FROST signing
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Step 1: Reach consensus on message to sign
        let mut consensus_adapter = create_testing_adapter(device_id);
        let consensus_config = ConsensusConfig {
            participants: participants.clone(),
            proposal: b"Agreed message for signing".to_vec(),
            timeout_ms: 10000,
        };

        let consensus_result = execute_consensus(&mut consensus_adapter, consensus_config).await;
        assert!(consensus_result.is_ok(), "Consensus step should succeed");
        let agreed_message = consensus_result.unwrap();
        assert!(agreed_message.success, "Should reach consensus");

        // Step 2: Sign the agreed message with FROST
        let mut frost_adapter = create_testing_adapter(device_id);
        let frost_config = FrostConfig {
            participants: participants.clone(),
            threshold: 2,
            message: agreed_message.consensus_value, // Use consensus result as message
            signing_package: vec![1, 2, 3, 4],
        };

        let frost_result = execute_frost_signing(&mut frost_adapter, frost_config).await;
        assert!(frost_result.is_ok(), "FROST signing should succeed");
        let signature = frost_result.unwrap();
        assert!(signature.success, "Should sign consensus message");
    }

    #[tokio::test]
    async fn test_full_protocol_composition() {
        // Test complete workflow: DKD + Consensus + FROST
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Step 1: Derive shared context with DKD
        let mut dkd_adapter = create_testing_adapter(device_id);
        let dkd_result = execute_dkd(
            &mut dkd_adapter,
            DkdConfig {
                participants: participants.clone(),
                threshold: 2,
                context: "shared_context".to_string(),
                derivation_path: vec![0],
            },
        )
        .await;
        assert!(
            dkd_result.is_ok() && dkd_result.as_ref().unwrap().success,
            "DKD should succeed"
        );

        // Step 2: Reach consensus on operation
        let mut consensus_adapter = create_testing_adapter(device_id);
        let consensus_result = execute_consensus(
            &mut consensus_adapter,
            ConsensusConfig {
                participants: participants.clone(),
                proposal: b"Operation to authorize".to_vec(),
                timeout_ms: 10000,
            },
        )
        .await;
        assert!(
            consensus_result.is_ok() && consensus_result.as_ref().unwrap().success,
            "Consensus should succeed"
        );

        // Step 3: FROST sign the authorized operation
        let mut frost_adapter = create_testing_adapter(device_id);
        let frost_result = execute_frost_signing(
            &mut frost_adapter,
            FrostConfig {
                participants: participants.clone(),
                threshold: 2,
                message: consensus_result.unwrap().consensus_value,
                signing_package: dkd_result.unwrap().derived_keys,
            },
        )
        .await;
        assert!(
            frost_result.is_ok() && frost_result.as_ref().unwrap().success,
            "FROST should succeed"
        );

        // Verify complete workflow
        let signature = frost_result.unwrap();
        assert!(
            !signature.signature.is_empty(),
            "Complete workflow should produce signature"
        );
    }
}

/// Session type safety tests per protocol guide
mod session_type_tests {
    use super::*;

    #[tokio::test]
    async fn test_protocol_type_safety() {
        // Test that protocols maintain type safety through choreographic execution
        let device_id = DeviceId::new();

        // Test DKD type safety
        let mut dkd_adapter = create_testing_adapter(device_id);
        let dkd_config = DkdConfig {
            participants: vec![device_id],
            threshold: 1,
            context: "type_safety_test".to_string(),
            derivation_path: vec![42],
        };

        let dkd_result = execute_dkd(&mut dkd_adapter, dkd_config).await;
        assert!(dkd_result.is_ok(), "DKD should maintain type safety");

        // Test FROST type safety
        let mut frost_adapter = create_testing_adapter(device_id);
        let frost_config = FrostConfig {
            participants: vec![device_id],
            threshold: 1,
            message: b"type safety message".to_vec(),
            signing_package: vec![0xaa, 0xbb],
        };

        let frost_result = execute_frost_signing(&mut frost_adapter, frost_config).await;
        assert!(frost_result.is_ok(), "FROST should maintain type safety");

        // Test Consensus type safety
        let mut consensus_adapter = create_testing_adapter(device_id);
        let consensus_config = ConsensusConfig {
            participants: vec![device_id],
            proposal: b"type safety proposal".to_vec(),
            timeout_ms: 5000,
        };

        let consensus_result = execute_consensus(&mut consensus_adapter, consensus_config).await;
        assert!(
            consensus_result.is_ok(),
            "Consensus should maintain type safety"
        );
    }

    #[tokio::test]
    async fn test_protocol_deadlock_freedom() {
        // Test that protocols are deadlock-free (always terminate)
        let device_id = DeviceId::new();

        let protocols = vec![
            // DKD protocol
            Box::new(|| {
                let device_copy = device_id;
                Box::pin(async move {
                    let mut adapter = create_testing_adapter(device_copy);
                    execute_dkd(
                        &mut adapter,
                        DkdConfig {
                            participants: vec![device_copy],
                            threshold: 1,
                            context: "deadlock_test_dkd".to_string(),
                            derivation_path: vec![1],
                        },
                    )
                    .await
                    .map(|_| ())
                })
            }) as Box<dyn Fn() -> _>,
            // FROST protocol
            Box::new(|| {
                let device_copy = device_id;
                Box::pin(async move {
                    let mut adapter = create_testing_adapter(device_copy);
                    execute_frost_signing(
                        &mut adapter,
                        FrostConfig {
                            participants: vec![device_copy],
                            threshold: 1,
                            message: b"deadlock test".to_vec(),
                            signing_package: vec![1, 2],
                        },
                    )
                    .await
                    .map(|_| ())
                })
            }),
            // Consensus protocol
            Box::new(|| {
                let device_copy = device_id;
                Box::pin(async move {
                    let mut adapter = create_testing_adapter(device_copy);
                    execute_consensus(
                        &mut adapter,
                        ConsensusConfig {
                            participants: vec![device_copy],
                            proposal: b"deadlock test".to_vec(),
                            timeout_ms: 1000,
                        },
                    )
                    .await
                    .map(|_| ())
                })
            }),
        ];

        // All protocols should terminate within timeout
        for (i, protocol) in protocols.iter().enumerate() {
            let result = timeout(Duration::from_secs(10), protocol()).await;
            assert!(result.is_ok(), "Protocol {} should not deadlock", i);
        }
    }

    #[tokio::test]
    async fn test_communication_safety() {
        // Test that every send has a matching receive (communication safety)
        let device_id = DeviceId::new();

        // Test with multi-participant protocols to verify communication patterns
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // DKD communication safety
        let mut adapter = create_testing_adapter(device_id);
        let dkd_result = execute_dkd(
            &mut adapter,
            DkdConfig {
                participants: participants.clone(),
                threshold: 2,
                context: "communication_safety".to_string(),
                derivation_path: vec![1, 2],
            },
        )
        .await;
        assert!(
            dkd_result.is_ok(),
            "DKD should maintain communication safety"
        );

        // FROST communication safety
        let mut adapter = create_testing_adapter(device_id);
        let frost_result = execute_frost_signing(
            &mut adapter,
            FrostConfig {
                participants: participants.clone(),
                threshold: 2,
                message: b"communication safety test".to_vec(),
                signing_package: vec![1, 2, 3],
            },
        )
        .await;
        assert!(
            frost_result.is_ok(),
            "FROST should maintain communication safety"
        );
    }
}

/// Property-based tests for all protocols
mod property_tests {
    use super::*;

    proptest! {
        #[test]
        fn test_all_protocols_always_terminate(
            participant_count in 1u32..=5,
            threshold in 1u32..=5,
            seed in 0u64..1000
        ) {
            let threshold = threshold.min(participant_count);

            tokio_test::block_on(async {
                let device_id = DeviceId::from_bytes([(seed % 256) as u8; 32]);
                let mut participants = vec![device_id];
                for i in 1..participant_count {
                    participants.push(DeviceId::from_bytes([i as u8; 32]));
                }

                // Test DKD termination
                let mut dkd_adapter = create_testing_adapter(device_id);
                let dkd_result = timeout(
                    Duration::from_secs(5),
                    execute_dkd(&mut dkd_adapter, DkdConfig {
                        participants: participants.clone(),
                        threshold,
                        context: format!("prop_test_{}", seed),
                        derivation_path: vec![(seed % 100) as u32],
                    })
                ).await;
                prop_assert!(dkd_result.is_ok(), "DKD should always terminate");

                // Test FROST termination
                let mut frost_adapter = create_testing_adapter(device_id);
                let frost_result = timeout(
                    Duration::from_secs(5),
                    execute_frost_signing(&mut frost_adapter, FrostConfig {
                        participants: participants.clone(),
                        threshold,
                        message: format!("prop_test_message_{}", seed).as_bytes().to_vec(),
                        signing_package: vec![(seed % 256) as u8],
                    })
                ).await;
                prop_assert!(frost_result.is_ok(), "FROST should always terminate");

                // Test Consensus termination
                let mut consensus_adapter = create_testing_adapter(device_id);
                let consensus_result = timeout(
                    Duration::from_secs(5),
                    execute_consensus(&mut consensus_adapter, ConsensusConfig {
                        participants: participants.clone(),
                        proposal: format!("prop_test_proposal_{}", seed).as_bytes().to_vec(),
                        timeout_ms: 2000,
                    })
                ).await;
                prop_assert!(consensus_result.is_ok(), "Consensus should always terminate");
            });
        }

        #[test]
        fn test_protocol_determinism(
            context in "[a-zA-Z0-9_]{1,30}",
            threshold in 1u32..=3,
            participant_count in 2u32..=5
        ) {
            let threshold = threshold.min(participant_count);

            tokio_test::block_on(async {
                let device_id = DeviceId::new();
                let mut participants = vec![device_id];
                for i in 1..participant_count {
                    participants.push(DeviceId::from_bytes([i as u8; 32]));
                }

                // Test DKD determinism
                let mut adapter1 = create_testing_adapter(device_id);
                let mut adapter2 = create_testing_adapter(device_id);

                let dkd_config1 = DkdConfig {
                    participants: participants.clone(),
                    threshold,
                    context: context.clone(),
                    derivation_path: vec![1, 2],
                };
                let dkd_config2 = dkd_config1.clone();

                let result1 = execute_dkd(&mut adapter1, dkd_config1).await;
                let result2 = execute_dkd(&mut adapter2, dkd_config2).await;

                prop_assert!(result1.is_ok() && result2.is_ok());

                let dkd1 = result1.unwrap();
                let dkd2 = result2.unwrap();

                // Same configuration should produce same results
                prop_assert_eq!(dkd1.derived_keys, dkd2.derived_keys);
                prop_assert_eq!(dkd1.success, dkd2.success);
            });
        }
    }
}

/// Performance benchmarking tests per protocol guide
mod performance_benchmarks {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn test_protocol_execution_benchmarks() {
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new()];

        // Benchmark DKD execution
        let start = Instant::now();
        for i in 0..10 {
            let mut adapter = create_testing_adapter(device_id);
            let result = execute_dkd(
                &mut adapter,
                DkdConfig {
                    participants: participants.clone(),
                    threshold: 1,
                    context: format!("bench_dkd_{}", i),
                    derivation_path: vec![i as u32],
                },
            )
            .await;
            assert!(result.is_ok(), "DKD benchmark iteration {} failed", i);
        }
        let dkd_duration = start.elapsed();

        // Benchmark FROST execution
        let start = Instant::now();
        for i in 0..10 {
            let mut adapter = create_testing_adapter(device_id);
            let result = execute_frost_signing(
                &mut adapter,
                FrostConfig {
                    participants: participants.clone(),
                    threshold: 1,
                    message: format!("bench_frost_{}", i).as_bytes().to_vec(),
                    signing_package: vec![i as u8],
                },
            )
            .await;
            assert!(result.is_ok(), "FROST benchmark iteration {} failed", i);
        }
        let frost_duration = start.elapsed();

        // Benchmark Consensus execution
        let start = Instant::now();
        for i in 0..10 {
            let mut adapter = create_testing_adapter(device_id);
            let result = execute_consensus(
                &mut adapter,
                ConsensusConfig {
                    participants: participants.clone(),
                    proposal: format!("bench_consensus_{}", i).as_bytes().to_vec(),
                    timeout_ms: 1000,
                },
            )
            .await;
            assert!(result.is_ok(), "Consensus benchmark iteration {} failed", i);
        }
        let consensus_duration = start.elapsed();

        // Performance assertions (adjust based on acceptable performance)
        assert!(
            dkd_duration < Duration::from_secs(2),
            "DKD benchmark took too long: {:?}",
            dkd_duration
        );
        assert!(
            frost_duration < Duration::from_secs(2),
            "FROST benchmark took too long: {:?}",
            frost_duration
        );
        assert!(
            consensus_duration < Duration::from_secs(5),
            "Consensus benchmark took too long: {:?}",
            consensus_duration
        );

        println!("Benchmark results:");
        println!("  DKD (10 iterations): {:?}", dkd_duration);
        println!("  FROST (10 iterations): {:?}", frost_duration);
        println!("  Consensus (10 iterations): {:?}", consensus_duration);
    }

    #[tokio::test]
    async fn test_adapter_creation_overhead() {
        let device_id = DeviceId::new();
        let iterations = 100;

        // Benchmark adapter creation overhead
        let start = Instant::now();
        for _ in 0..iterations {
            let _adapter = create_testing_adapter(device_id);
            // Adapter is dropped here
        }
        let duration = start.elapsed();

        // Should be very fast
        assert!(
            duration < Duration::from_millis(100),
            "Adapter creation overhead too high: {:?}",
            duration
        );

        println!(
            "Adapter creation benchmark ({} iterations): {:?}",
            iterations, duration
        );
    }
}

/// Network simulation tests following protocol guide
mod network_simulation_tests {
    use super::*;

    #[tokio::test]
    async fn test_protocols_in_simulation_environment() {
        // Test all protocols work correctly in simulation environment
        let device_id = DeviceId::new();
        let participants = vec![device_id, DeviceId::new(), DeviceId::new()];

        // Test DKD in simulation
        let mut sim_adapter = create_simulation_adapter(device_id);
        let dkd_result = execute_dkd(
            &mut sim_adapter,
            DkdConfig {
                participants: participants.clone(),
                threshold: 2,
                context: "simulation_dkd".to_string(),
                derivation_path: vec![1, 2, 3],
            },
        )
        .await;
        assert!(dkd_result.is_ok(), "DKD should work in simulation");

        // Test FROST in simulation
        let mut sim_adapter = create_simulation_adapter(device_id);
        let frost_result = execute_frost_signing(
            &mut sim_adapter,
            FrostConfig {
                participants: participants.clone(),
                threshold: 2,
                message: b"simulation FROST test".to_vec(),
                signing_package: vec![0x01, 0x02, 0x03],
            },
        )
        .await;
        assert!(frost_result.is_ok(), "FROST should work in simulation");

        // Test Consensus in simulation
        let mut sim_adapter = create_simulation_adapter(device_id);
        let consensus_result = execute_consensus(
            &mut sim_adapter,
            ConsensusConfig {
                participants: participants.clone(),
                proposal: b"simulation consensus test".to_vec(),
                timeout_ms: 5000,
            },
        )
        .await;
        assert!(
            consensus_result.is_ok(),
            "Consensus should work in simulation"
        );
    }

    #[tokio::test]
    async fn test_mixed_adapter_environments() {
        // Test protocols work across different adapter types
        let device_id = DeviceId::new();

        // Use different adapters for different stages of a workflow
        let mut testing_adapter = create_testing_adapter(device_id);
        let dkd_result = execute_dkd(
            &mut testing_adapter,
            DkdConfig {
                participants: vec![device_id],
                threshold: 1,
                context: "mixed_test".to_string(),
                derivation_path: vec![1],
            },
        )
        .await;
        assert!(dkd_result.is_ok(), "Testing adapter should work for DKD");

        let mut sim_adapter = create_simulation_adapter(device_id);
        let frost_result = execute_frost_signing(
            &mut sim_adapter,
            FrostConfig {
                participants: vec![device_id],
                threshold: 1,
                message: b"mixed environment test".to_vec(),
                signing_package: dkd_result.unwrap().derived_keys,
            },
        )
        .await;
        assert!(
            frost_result.is_ok(),
            "Simulation adapter should work for FROST"
        );

        let mut prod_adapter = create_production_adapter(device_id);
        let consensus_result = execute_consensus(
            &mut prod_adapter,
            ConsensusConfig {
                participants: vec![device_id],
                proposal: b"production environment test".to_vec(),
                timeout_ms: 3000,
            },
        )
        .await;
        assert!(
            consensus_result.is_ok(),
            "Production adapter should work for Consensus"
        );
    }
}

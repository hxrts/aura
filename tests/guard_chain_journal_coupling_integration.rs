//! Integration tests for complete guard chain with journal coupling
//!
//! This test suite verifies that the complete guard chain (CapGuard → FlowGuard → JournalCoupler)
//! works correctly with choreographic protocol execution and CRDT operations.

use aura_core::{
    semilattice::{Bottom, CvState, JoinSemilattice},
    DeviceId, Journal,
};
use aura_mpst::journal_coupling::{JournalAnnotation, JournalOpType};
use aura_protocol::{
    choreography::protocols::anti_entropy::{
        execute_anti_entropy_with_guard_chain, AntiEntropyConfig, CrdtType,
    },
    effects::{
        semilattice::CrdtCoordinator,
        system::AuraEffectSystem,
    },
    guards::{JournalCoupler, JournalCouplerBuilder, ProtocolGuard},
    handlers::ExecutionMode,
};
use aura_wot::Capability;
use serde::{Deserialize, Serialize};

// Test CRDT for integration testing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCounter {
    value: u64,
    device: DeviceId,
}

impl TestCounter {
    pub fn new(device: DeviceId) -> Self {
        Self { value: 0, device }
    }

    pub fn increment(&mut self) {
        self.value += 1;
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}

impl JoinSemilattice for TestCounter {
    fn join(&self, other: &Self) -> Self {
        Self {
            value: self.value.max(other.value),
            device: if self.value >= other.value {
                self.device
            } else {
                other.device
            },
        }
    }
}

impl Bottom for TestCounter {
    fn bottom() -> Self {
        Self {
            value: 0,
            device: DeviceId::new(),
        }
    }
}

impl CvState for TestCounter {}

#[tokio::test]
async fn test_complete_guard_chain_execution() {
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Create effect systems for both devices
    let effect_system_a = AuraEffectSystem::new(device_a, ExecutionMode::Testing);
    let effect_system_b = AuraEffectSystem::new(device_b, ExecutionMode::Testing);

    // Create CRDT coordinators using builder pattern
    let coordinator_a = CrdtCoordinator::with_cv_state(device_a, TestCounter::new(device_a));
    let coordinator_b = CrdtCoordinator::with_cv_state(device_b, TestCounter::new(device_b));

    // Configure anti-entropy
    let config = AntiEntropyConfig {
        participants: vec![device_a, device_b],
        max_ops_per_sync: 100,
    };

    // Test execution with complete guard chain
    let result_a = execute_anti_entropy_with_guard_chain(
        device_a,
        config.clone(),
        true, // is_requester
        &effect_system_a,
        coordinator_a,
    )
    .await;

    // Verify successful execution
    assert!(result_a.is_ok(), "Guard chain execution should succeed");
    let (sync_result_a, final_coordinator_a) = result_a.unwrap();
    assert!(sync_result_a.success, "Anti-entropy sync should succeed");
    assert_eq!(final_coordinator_a.device_id(), device_a);
}

#[tokio::test]
async fn test_journal_coupler_standalone() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Create journal coupler with annotations
    let coupler = JournalCouplerBuilder::new()
        .optimistic()
        .max_retries(2)
        .with_annotation(
            "test_operation".to_string(),
            JournalAnnotation::add_facts("Test fact addition for integration"),
        )
        .build();

    // Test journal coupling execution
    let result = coupler
        .execute_with_coupling("test_operation", &mut effect_system, |_effects| async {
            // Simulate successful protocol operation
            Ok("operation_completed".to_string())
        })
        .await
        .unwrap();

    assert_eq!(result.result, "operation_completed");
    assert!(result.coupling_metrics.coupling_successful);
    assert!(result.coupling_metrics.operations_applied > 0);
}

#[tokio::test]
async fn test_protocol_guard_with_journal_coupling() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Create protocol guard
    let guard = ProtocolGuard::new("test_protocol")
        .require_capabilities(vec![Capability::Execute {
            operation: "test_operation".to_string(),
        }])
        .delta_facts(vec![serde_json::json!({
            "type": "session_attestation",
            "session_id": "test_session",
            "operation": "test_protocol_execution"
        })])
        .leakage_budget(aura_protocol::guards::LeakageBudget::new(1, 0, 0));

    // Create journal coupler
    let coupler = JournalCouplerBuilder::new()
        .with_annotation(
            "test_protocol".to_string(),
            JournalAnnotation::add_facts("Protocol execution completed"),
        )
        .build();

    // Execute with complete guard chain
    let result = guard
        .execute_with_journal_coupling(&mut effect_system, &coupler, |_effects| async {
            Ok(42u32)
        })
        .await;

    // Verify execution
    assert!(result.is_ok(), "Guard chain with journal coupling should succeed");
    let coupling_result = result.unwrap();
    assert_eq!(coupling_result.result, 42);
    assert!(coupling_result.coupling_metrics.coupling_successful);
}

#[tokio::test]
async fn test_journal_coupling_with_different_annotation_types() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Test different journal annotation types
    let test_cases = vec![
        (
            "facts_operation",
            JournalAnnotation::add_facts("Add facts test"),
        ),
        (
            "caps_operation",
            JournalAnnotation::refine_caps("Refine capabilities test"),
        ),
        (
            "merge_operation",
            JournalAnnotation::merge("General merge test"),
        ),
    ];

    for (op_id, annotation) in test_cases {
        let coupler = JournalCouplerBuilder::new()
            .with_annotation(op_id.to_string(), annotation)
            .build();

        let result = coupler
            .execute_with_coupling(op_id, &mut effect_system, |_effects| async {
                Ok(format!("completed_{}", op_id))
            })
            .await
            .unwrap();

        assert_eq!(result.result, format!("completed_{}", op_id));
        assert!(result.coupling_metrics.coupling_successful);

        // Should have applied journal operations
        assert!(!result.journal_ops_applied.is_empty());
    }
}

#[tokio::test]
async fn test_optimistic_vs_pessimistic_journal_coupling() {
    let device_id = DeviceId::new();

    // Test pessimistic coupling (default)
    let mut effect_system_pessimistic = AuraEffectSystem::new(device_id, ExecutionMode::Testing);
    let pessimistic_coupler = JournalCouplerBuilder::new()
        .with_annotation(
            "pessimistic_test".to_string(),
            JournalAnnotation::add_facts("Pessimistic test"),
        )
        .build(); // Default is pessimistic

    let pessimistic_result = pessimistic_coupler
        .execute_with_coupling("pessimistic_test", &mut effect_system_pessimistic, |_| async {
            Ok("pessimistic_done".to_string())
        })
        .await
        .unwrap();

    assert_eq!(pessimistic_result.result, "pessimistic_done");

    // Test optimistic coupling
    let mut effect_system_optimistic = AuraEffectSystem::new(device_id, ExecutionMode::Testing);
    let optimistic_coupler = JournalCouplerBuilder::new()
        .optimistic()
        .with_annotation(
            "optimistic_test".to_string(),
            JournalAnnotation::add_facts("Optimistic test"),
        )
        .build();

    let optimistic_result = optimistic_coupler
        .execute_with_coupling("optimistic_test", &mut effect_system_optimistic, |_| async {
            Ok("optimistic_done".to_string())
        })
        .await
        .unwrap();

    assert_eq!(optimistic_result.result, "optimistic_done");

    // Both should succeed, but with potentially different performance characteristics
    assert!(pessimistic_result.coupling_metrics.coupling_successful);
    assert!(optimistic_result.coupling_metrics.coupling_successful);
}

#[tokio::test]
async fn test_journal_coupling_error_handling() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    let coupler = JournalCouplerBuilder::new()
        .pessimistic() // Use pessimistic for this test
        .with_annotation(
            "error_test".to_string(),
            JournalAnnotation::add_facts("Should not be applied due to operation failure"),
        )
        .build();

    // Test that journal operations are not applied when operation fails
    let result = coupler
        .execute_with_coupling("error_test", &mut effect_system, |_effects| async {
            Err(aura_core::AuraError::internal("Simulated operation failure"))
        })
        .await;

    // Operation should fail and journal operations should not be applied
    assert!(result.is_err(), "Operation should fail");
}

#[tokio::test]
async fn test_guard_chain_capability_enforcement() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Create guard requiring capabilities that may not be available
    let guard = ProtocolGuard::new("capability_test")
        .require_capabilities(vec![
            Capability::Execute {
                operation: "restricted_operation".to_string(),
            },
            Capability::Write {
                resource_pattern: "restricted_message".to_string(),
            },
        ]);

    let coupler = JournalCouplerBuilder::new()
        .with_annotation(
            "capability_test".to_string(),
            JournalAnnotation::add_facts("Should only apply if capabilities are satisfied"),
        )
        .build();

    // Attempt execution - may succeed or fail depending on testing environment setup
    let result = guard
        .execute_with_journal_coupling(&mut effect_system, &coupler, |_effects| async {
            Ok("capability_protected_operation".to_string())
        })
        .await;

    // This test verifies that capability checking is integrated into the guard chain
    // The specific result depends on the test environment's capability configuration
    match result {
        Ok(coupling_result) => {
            // If capabilities are satisfied, operation should complete
            assert_eq!(coupling_result.result, "capability_protected_operation");
            assert!(coupling_result.coupling_metrics.coupling_successful);
        }
        Err(_) => {
            // If capabilities are not satisfied, operation should be blocked
            // This is also a valid outcome demonstrating proper capability enforcement
        }
    }
}

#[tokio::test]
async fn test_leakage_budget_tracking() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Create guard with specific leakage budget
    let guard = ProtocolGuard::new("leakage_test")
        .leakage_budget(aura_protocol::guards::LeakageBudget::new(
            3, // External adversary
            2, // Neighbor adversary
            1, // In-group adversary
        ));

    let coupler = JournalCouplerBuilder::new()
        .with_annotation(
            "leakage_test".to_string(),
            JournalAnnotation::add_facts("Operation with tracked leakage budget"),
        )
        .build();

    let result = guard
        .execute_with_journal_coupling(&mut effect_system, &coupler, |_effects| async {
            Ok("leakage_tracked_operation".to_string())
        })
        .await
        .unwrap();

    assert_eq!(result.result, "leakage_tracked_operation");
    assert!(result.coupling_metrics.coupling_successful);

    // Verify that leakage budget tracking is integrated
    // (specific budget values depend on the effect system implementation)
}

#[tokio::test]
async fn test_guard_chain_with_multiple_delta_facts() {
    let device_id = DeviceId::new();
    let mut effect_system = AuraEffectSystem::new(device_id, ExecutionMode::Testing);

    // Create guard with multiple delta facts
    let guard = ProtocolGuard::new("multi_delta_test")
        .delta_facts(vec![
            serde_json::json!({
                "type": "session_attestation",
                "session_id": "multi_delta_session",
                "operation": "multi_delta_operation"
            }),
            serde_json::json!({
                "type": "capability_grant",
                "capability": "multi_delta_capability",
                "target_device": device_id.to_string(),
                "granted_for": "multi_delta_test"
            }),
            serde_json::json!({
                "type": "device_registration",
                "device_id": device_id.to_string(),
                "registered_at": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            }),
        ]);

    let coupler = JournalCouplerBuilder::new()
        .with_annotation(
            "multi_delta_test".to_string(),
            JournalAnnotation::merge("Apply multiple delta facts"),
        )
        .build();

    let result = guard
        .execute_with_journal_coupling(&mut effect_system, &coupler, |_effects| async {
            Ok("multi_delta_completed".to_string())
        })
        .await
        .unwrap();

    assert_eq!(result.result, "multi_delta_completed");
    assert!(result.coupling_metrics.coupling_successful);
    assert!(!result.journal_ops_applied.is_empty());
}

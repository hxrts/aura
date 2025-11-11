//! CRDT Convergence Property Tests
//!
//! This test suite verifies that all CRDT implementations in Aura satisfy
//! the fundamental convergence properties required for distributed consistency:
//!
//! 1. **Strong Eventual Consistency (SEC)**: All replicas that have received
//!    the same set of operations converge to the same state
//! 2. **Commutativity**: The order of operation application doesn't affect
//!    the final state for commutative CRDTs
//! 3. **Associativity**: Grouping of operations doesn't affect the final state
//! 4. **Idempotence**: Applying the same operation multiple times has the
//!    same effect as applying it once
//! 5. **Monotonicity**: States only grow (for join semilattices) or shrink
//!    (for meet semilattices) but never oscillate
//!
//! These properties are essential for ensuring that Aura's distributed
//! journal and capability system maintains consistency across all devices.

use aura_core::{
    semilattice::{
        Bottom, CausalOp, CmApply, CvState, Dedup, DeltaState, JoinSemilattice, MeetSemilattice,
        MvState,
    },
    CausalContext, DeviceId, Journal, VectorClock,
};
use aura_protocol::{
    effects::semilattice::{
        CmHandler, CvHandler, DeltaHandler, MvHandler,
    },
    handlers::ExecutionMode,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::SystemTime};

// Test CRDT types for property verification

/// Test counter CRDT (convergent/state-based)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCounter {
    pub value: u64,
    pub device: DeviceId,
}

impl TestCounter {
    pub fn new(device: DeviceId) -> Self {
        Self { value: 0, device }
    }

    pub fn increment(&mut self) -> u64 {
        self.value += 1;
        self.value
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

/// Test operation for commutative CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestIncrement {
    pub operation_id: String,
    pub device_id: DeviceId,
    pub increment_value: u32,
    pub timestamp: SystemTime,
    pub causal_context: CausalContext,
}

impl TestIncrement {
    pub fn new(device_id: DeviceId, increment_value: u32) -> Self {
        Self {
            operation_id: format!("inc_{}_{}", device_id, increment_value),
            device_id,
            increment_value,
            timestamp: SystemTime::now(),
            causal_context: CausalContext::new(),
        }
    }
}

impl CausalOp for TestIncrement {
    type Id = String;
    type Ctx = CausalContext;

    fn id(&self) -> &Self::Id {
        &self.operation_id
    }

    fn causal_context(&self) -> &Self::Ctx {
        &self.causal_context
    }
}

/// Test commutative state that applies increments
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCommutativeCounter {
    pub total: u64,
    pub applied_ops: HashMap<String, TestIncrement>,
}

impl TestCommutativeCounter {
    pub fn new() -> Self {
        Self {
            total: 0,
            applied_ops: HashMap::new(),
        }
    }
}

impl CmApply<TestIncrement> for TestCommutativeCounter {
    fn apply(&mut self, op: &TestIncrement) {
        if !self.applied_ops.contains_key(&op.operation_id) {
            self.total += op.increment_value as u64;
            self.applied_ops.insert(op.operation_id.clone(), op.clone());
        }
    }
}

impl Dedup<String> for TestCommutativeCounter {
    fn has_applied(&self, op_id: &String) -> bool {
        self.applied_ops.contains_key(op_id)
    }
}

/// Test delta state for delta-based CRDT
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestDeltaCounter {
    pub value: u64,
    pub device: DeviceId,
}

impl TestDeltaCounter {
    pub fn new(device: DeviceId) -> Self {
        Self { value: 0, device }
    }

    pub fn increment(&mut self) -> TestDelta {
        self.value += 1;
        TestDelta {
            increment: 1,
            device: self.device,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestDelta {
    pub increment: u64,
    pub device: DeviceId,
}

impl JoinSemilattice for TestDeltaCounter {
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

impl Bottom for TestDeltaCounter {
    fn bottom() -> Self {
        Self {
            value: 0,
            device: DeviceId::new(),
        }
    }
}

impl CvState for TestDeltaCounter {}

impl DeltaState for TestDeltaCounter {
    type Delta = TestDelta;

    fn apply_delta(&mut self, delta: &Self::Delta) {
        self.value += delta.increment;
        if delta.increment > 0 {
            self.device = delta.device;
        }
    }
}

/// Test meet semilattice state (constraint-based)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestConstraint {
    pub max_value: u64,
    pub min_value: u64,
}

impl TestConstraint {
    pub fn new(max_value: u64, min_value: u64) -> Self {
        Self {
            max_value: max_value.max(min_value),
            min_value,
        }
    }
}

impl MeetSemilattice for TestConstraint {
    fn meet(&self, other: &Self) -> Self {
        Self {
            max_value: self.max_value.min(other.max_value),
            min_value: self.min_value.max(other.min_value),
        }
    }
}

impl Bottom for TestConstraint {
    fn bottom() -> Self {
        Self {
            max_value: u64::MAX,
            min_value: 0,
        }
    }
}

impl MvState for TestConstraint {}

// Property-based tests

#[tokio::test]
async fn test_cv_convergent_state_convergence() {
    // Test that CvHandler achieves convergence for state-based CRDTs
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    let device_c = DeviceId::new();

    let mut handler_a = CvHandler::new(device_a, TestCounter::new(device_a));
    let mut handler_b = CvHandler::new(device_b, TestCounter::new(device_b));
    let mut handler_c = CvHandler::new(device_c, TestCounter::new(device_c));

    // Apply different operations on each replica
    let mut state_a = TestCounter::new(device_a);
    state_a.value = 10;
    handler_a.update_state(state_a).await.unwrap();

    let mut state_b = TestCounter::new(device_b);
    state_b.value = 15;
    handler_b.update_state(state_b).await.unwrap();

    let mut state_c = TestCounter::new(device_c);
    state_c.value = 8;
    handler_c.update_state(state_c).await.unwrap();

    // Simulate synchronization: each replica receives the others' states
    let final_state_a = handler_a.current_state();
    let final_state_b = handler_b.current_state();
    let final_state_c = handler_c.current_state();

    // All should converge after synchronization
    let converged_ab = final_state_a.join(&final_state_b);
    let converged_abc = converged_ab.join(&final_state_c);

    // Verify convergence property: max value wins
    assert_eq!(converged_abc.value, 15);
    assert_eq!(converged_abc.device, device_b);

    // Test commutativity: different order should yield same result
    let converged_ba = final_state_b.join(&final_state_a);
    let converged_bac = converged_ba.join(&final_state_c);
    assert_eq!(converged_abc, converged_bac);
}

#[tokio::test]
async fn test_cm_commutative_operation_convergence() {
    // Test that CmHandler achieves convergence for operation-based CRDTs
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    let mut handler_a = CmHandler::new(device_a, TestCommutativeCounter::new());
    let mut handler_b = CmHandler::new(device_b, TestCommutativeCounter::new());

    // Create operations
    let op1 = TestIncrement::new(device_a, 5);
    let op2 = TestIncrement::new(device_a, 3);
    let op3 = TestIncrement::new(device_b, 7);

    // Apply operations in different orders on different replicas
    // Replica A: op1, op2, op3
    handler_a.apply_operation(op1.clone()).await.unwrap();
    handler_a.apply_operation(op2.clone()).await.unwrap();
    handler_a.apply_operation(op3.clone()).await.unwrap();

    // Replica B: op3, op1, op2
    handler_b.apply_operation(op3.clone()).await.unwrap();
    handler_b.apply_operation(op1.clone()).await.unwrap();
    handler_b.apply_operation(op2.clone()).await.unwrap();

    // Both should converge to the same total
    let state_a = handler_a.current_state();
    let state_b = handler_b.current_state();

    assert_eq!(state_a.total, state_b.total);
    assert_eq!(state_a.total, 15); // 5 + 3 + 7
    assert_eq!(state_a.applied_ops.len(), 3);
    assert_eq!(state_b.applied_ops.len(), 3);
}

#[tokio::test]
async fn test_delta_bandwidth_optimization_convergence() {
    // Test that DeltaHandler achieves convergence while optimizing bandwidth
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    let mut handler_a = DeltaHandler::new(device_a, TestDeltaCounter::new(device_a));
    let mut handler_b = DeltaHandler::new(device_b, TestDeltaCounter::new(device_b));

    // Apply deltas
    let delta1 = TestDelta {
        increment: 10,
        device: device_a,
    };
    let delta2 = TestDelta {
        increment: 5,
        device: device_b,
    };

    handler_a.apply_delta(delta1.clone()).await.unwrap();
    handler_b.apply_delta(delta2.clone()).await.unwrap();

    // Cross-apply deltas
    handler_a.apply_delta(delta2).await.unwrap();
    handler_b.apply_delta(delta1).await.unwrap();

    // Both should converge
    let state_a = handler_a.current_state();
    let state_b = handler_b.current_state();

    assert_eq!(state_a.value, state_b.value);
    assert_eq!(state_a.value, 15); // 10 + 5
}

#[tokio::test]
async fn test_mv_meet_semilattice_convergence() {
    // Test that MvHandler achieves convergence for meet semilattices
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    let mut handler_a = MvHandler::new(device_a, TestConstraint::new(100, 10));
    let mut handler_b = MvHandler::new(device_b, TestConstraint::new(80, 20));

    // Apply constraint refinements
    let refinement1 = TestConstraint::new(90, 15);
    let refinement2 = TestConstraint::new(85, 25);

    handler_a.apply_constraint(refinement1).await.unwrap();
    handler_b.apply_constraint(refinement2).await.unwrap();

    // Both should converge to the intersection of constraints
    let state_a = handler_a.current_state();
    let state_b = handler_b.current_state();

    // Meet operation: min of max_values, max of min_values
    let expected_max = 80.min(90).min(85); // 80
    let expected_min = 20.max(15).max(25); // 25

    assert_eq!(state_a.max_value, expected_max);
    assert_eq!(state_a.min_value, expected_min);
    assert_eq!(state_b.max_value, expected_max);
    assert_eq!(state_b.min_value, expected_min);
}

#[tokio::test]
async fn test_crdt_coordinator_convergence() {
    // Test that the CrdtCoordinator properly coordinates all CRDT types
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();

    // Use builder pattern to create coordinators with all CRDT types
    use aura_protocol::effects::semilattice::CrdtCoordinator;

    let coordinator_a = CrdtCoordinator::new(device_a)
        .with_cv_handler(CvHandler::with_state(TestCounter::new(device_a)))
        .with_cm_handler(CmHandler::new(TestCommutativeCounter::new()))
        .with_delta_handler(DeltaHandler::with_state(TestDeltaCounter::new(device_a)))
        .with_mv_handler(MvHandler::with_state(TestConstraint::new(100, 10)));

    let coordinator_b = CrdtCoordinator::new(device_b)
        .with_cv_handler(CvHandler::with_state(TestCounter::new(device_b)))
        .with_cm_handler(CmHandler::new(TestCommutativeCounter::new()))
        .with_delta_handler(DeltaHandler::with_state(TestDeltaCounter::new(device_b)))
        .with_mv_handler(MvHandler::with_state(TestConstraint::new(100, 10)));

    // Both coordinators should properly manage all CRDT types
    assert!(coordinator_a.has_cv_handler());
    assert!(coordinator_a.has_cm_handler());
    assert!(coordinator_a.has_delta_handler());
    assert!(coordinator_a.has_mv_handler());

    assert!(coordinator_b.has_cv_handler());
    assert!(coordinator_b.has_cm_handler());
    assert!(coordinator_b.has_delta_handler());
    assert!(coordinator_b.has_mv_handler());
}

#[tokio::test]
async fn test_idempotence_property() {
    // Test that applying the same operation multiple times is idempotent
    let device_id = DeviceId::new();
    let mut handler = CmHandler::new(device_id, TestCommutativeCounter::new());

    let op = TestIncrement::new(device_id, 5);

    // Apply the same operation multiple times
    handler.apply_operation(op.clone()).await.unwrap();
    handler.apply_operation(op.clone()).await.unwrap();
    handler.apply_operation(op.clone()).await.unwrap();

    let state = handler.current_state();

    // Should only be applied once due to deduplication
    assert_eq!(state.total, 5);
    assert_eq!(state.applied_ops.len(), 1);
}

#[tokio::test]
async fn test_associativity_property() {
    // Test that grouping of operations doesn't affect the result
    let device_id = DeviceId::new();

    let counter1 = TestCounter { value: 10, device: device_id };
    let counter2 = TestCounter { value: 20, device: device_id };
    let counter3 = TestCounter { value: 15, device: device_id };

    // Test (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
    let left_assoc = counter1.join(&counter2).join(&counter3);
    let right_assoc = counter1.join(&counter2.join(&counter3));

    assert_eq!(left_assoc, right_assoc);
    assert_eq!(left_assoc.value, 20); // Max of 10, 20, 15
}

#[tokio::test]
async fn test_monotonicity_property() {
    // Test that join operations are monotonic (values only increase)
    let device_id = DeviceId::new();

    let initial = TestCounter { value: 10, device: device_id };
    let update1 = TestCounter { value: 15, device: device_id };
    let update2 = TestCounter { value: 12, device: device_id }; // Lower value

    let after_update1 = initial.join(&update1);
    assert!(after_update1.value >= initial.value);

    let after_update2 = after_update1.join(&update2);
    assert!(after_update2.value >= after_update1.value);

    // Final value should be the maximum seen
    assert_eq!(after_update2.value, 15);
}

#[tokio::test]
async fn test_concurrent_operations_convergence() {
    // Test convergence under concurrent operations from multiple devices
    let devices: Vec<DeviceId> = (0..5).map(|_| DeviceId::new()).collect();

    let mut handlers: Vec<CmHandler<TestCommutativeCounter, TestIncrement, String>> = devices
        .iter()
        .map(|&device_id| CmHandler::new(device_id, TestCommutativeCounter::new()))
        .collect();

    // Create operations from different devices
    let operations: Vec<TestIncrement> = devices
        .iter()
        .enumerate()
        .map(|(i, &device_id)| TestIncrement::new(device_id, (i + 1) as u32 * 10))
        .collect();

    // Apply all operations to all handlers (simulating gossip protocol)
    for handler in &mut handlers {
        for op in &operations {
            handler.apply_operation(op.clone()).await.unwrap();
        }
    }

    // All handlers should converge to the same state
    let expected_total: u64 = operations.iter().map(|op| op.increment_value as u64).sum();

    for handler in &handlers {
        let state = handler.current_state();
        assert_eq!(state.total, expected_total);
        assert_eq!(state.applied_ops.len(), operations.len());
    }
}

#[test]
fn test_counter_join_properties_deterministic() {
    // Test join properties with deterministic values
    let device = DeviceId::new();
    let values = vec![10u64, 25, 8, 42, 15];
    let counters: Vec<TestCounter> = values
        .iter()
        .map(|&v| TestCounter { value: v, device })
        .collect();

    // Find expected maximum
    let expected_max = values.iter().max().unwrap();

    // Compute join in arbitrary order
    let mut result = counters[0].clone();
    for counter in counters.iter().skip(1) {
        result = result.join(counter);
    }

    // Result should be the maximum
    assert_eq!(result.value, *expected_max);
}

#[test]
fn test_commutativity_property_deterministic() {
    // Test commutativity with specific values
    let test_cases = vec![(10u64, 20u64), (100, 50), (0, 1), (42, 42)];

    for (a, b) in test_cases {
        let device = DeviceId::new();
        let counter_a = TestCounter { value: a, device };
        let counter_b = TestCounter { value: b, device };

        // a ⊔ b = b ⊔ a
        let ab = counter_a.join(&counter_b);
        let ba = counter_b.join(&counter_a);

        assert_eq!(ab, ba);
    }
}

#[tokio::test]
async fn test_network_partition_healing() {
    // Test that partitioned networks heal properly when reconnected
    let device_a = DeviceId::new();
    let device_b = DeviceId::new();
    let device_c = DeviceId::new();

    let mut handler_a = CvHandler::new(device_a, TestCounter::new(device_a));
    let mut handler_b = CvHandler::new(device_b, TestCounter::new(device_b));
    let mut handler_c = CvHandler::new(device_c, TestCounter::new(device_c));

    // Simulate network partition: A-B connected, C isolated
    let mut state_a = TestCounter::new(device_a);
    state_a.value = 10;
    handler_a.update_state(state_a.clone()).await.unwrap();

    // A and B sync
    handler_b.update_state(state_a).await.unwrap();

    // C operates in isolation
    let mut state_c = TestCounter::new(device_c);
    state_c.value = 25;
    handler_c.update_state(state_c.clone()).await.unwrap();

    // Verify partition state
    assert_eq!(handler_a.current_state().value, 10);
    assert_eq!(handler_b.current_state().value, 10);
    assert_eq!(handler_c.current_state().value, 25);

    // Partition heals: C rejoins
    let final_state_a = handler_a.current_state().join(&handler_c.current_state());
    let final_state_b = handler_b.current_state().join(&handler_c.current_state());
    let final_state_c = handler_c.current_state().join(&handler_a.current_state());

    // All should converge to the maximum value
    assert_eq!(final_state_a.value, 25);
    assert_eq!(final_state_b.value, 25);
    assert_eq!(final_state_c.value, 25);
}

#[tokio::test]
async fn test_stress_concurrent_updates() {
    // Stress test with many concurrent updates
    let device_id = DeviceId::new();
    let mut handler = CmHandler::new(device_id, TestCommutativeCounter::new());

    // Create many operations concurrently
    let num_operations = 100;
    let operations: Vec<TestIncrement> = (0..num_operations)
        .map(|i| TestIncrement::new(device_id, i % 10 + 1))
        .collect();

    // Apply all operations
    for op in &operations {
        handler.apply_operation(op.clone()).await.unwrap();
    }

    let state = handler.current_state();

    // Each unique operation should be applied exactly once
    let unique_ops: std::collections::HashSet<String> = operations
        .iter()
        .map(|op| op.operation_id.clone())
        .collect();

    assert_eq!(state.applied_ops.len(), unique_ops.len());

    // Total should be sum of all unique increments
    let expected_total: u64 = operations
        .iter()
        .map(|op| op.increment_value as u64)
        .sum();

    assert_eq!(state.total, expected_total);
}

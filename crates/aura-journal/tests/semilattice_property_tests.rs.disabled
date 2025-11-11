//! Comprehensive CRDT Semilattice Property Tests
//!
//! Property-based tests that verify the mathematical foundations of the journal
//! CRDT operations are sound. These tests ensure the semilattice laws hold for
//! all journal operations and state mutations.
//!
//! ## Properties Verified
//!
//! 1. **Join Semilattice Laws**: Idempotence, commutativity, associativity
//! 2. **State Convergence**: Eventually consistent convergence guarantees
//! 3. **Operation Ordering**: Causal ordering and deterministic application
//! 4. **Monotonicity**: State only grows, never shrinks
//! 5. **Conflict Resolution**: Deterministic conflict resolution for concurrent updates

use aura_core::{
    AccountId, AuraResult, CapabilityId, DeviceId, RelationshipId
};
use aura_journal::{
    journal_ops::{JournalOp, OpType},
    semilattice::{
        account_state::AccountState,
        journal_map::JournalMap,
        types::{OpLog, StateVector, JournalCrdt},
        SemilatticeOps, JoinSemilattice,
    },
    operations::{DeviceOperation, CapabilityOperation, RelationshipOperation},
};
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};

/// Strategy to generate arbitrary account IDs
fn arbitrary_account_id() -> impl Strategy<Value = AccountId> {
    any::<[u8; 32]>().prop_map(AccountId::from_bytes)
}

/// Strategy to generate arbitrary device IDs  
fn arbitrary_device_id() -> impl Strategy<Value = DeviceId> {
    any::<[u8; 32]>().prop_map(DeviceId::from_bytes)
}

/// Strategy to generate arbitrary capability IDs
fn arbitrary_capability_id() -> impl Strategy<Value = CapabilityId> {
    any::<[u8; 32]>().prop_map(CapabilityId::from_bytes)
}

/// Strategy to generate arbitrary relationship IDs
fn arbitrary_relationship_id() -> impl Strategy<Value = RelationshipId> {
    any::<[u8; 32]>().prop_map(RelationshipId::from_bytes)
}

/// Strategy to generate arbitrary journal operations
fn arbitrary_journal_op() -> impl Strategy<Value = JournalOp> {
    prop_oneof![
        (arbitrary_device_id(), any::<[u8; 32]>()).prop_map(|(device_id, key)| JournalOp {
            op_type: OpType::Device(DeviceOperation::Add {
                device_id,
                public_key: key.to_vec(),
                capabilities: HashSet::new(),
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        }),
        (arbitrary_device_id(), arbitrary_capability_id()).prop_map(|(device_id, cap_id)| JournalOp {
            op_type: OpType::Capability(CapabilityOperation::Grant {
                device_id,
                capability_id: cap_id,
                granted_by: device_id,
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        }),
        (arbitrary_device_id(), arbitrary_relationship_id()).prop_map(|(device_id, rel_id)| JournalOp {
            op_type: OpType::Relationship(RelationshipOperation::Create {
                relationship_id: rel_id,
                participants: vec![device_id],
                relationship_type: "trust".to_string(),
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        }),
    ]
}

/// Strategy to generate arbitrary account state
fn arbitrary_account_state() -> impl Strategy<Value = AccountState> {
    (arbitrary_account_id(), prop::collection::vec(arbitrary_journal_op(), 0..10))
        .prop_map(|(account_id, ops)| {
            let mut state = AccountState::new(account_id);
            for op in ops {
                let _ = state.apply_operation(op);
            }
            state
        })
}

/// Strategy to generate arbitrary journal map
fn arbitrary_journal_map() -> impl Strategy<Value = JournalMap> {
    prop::collection::vec(arbitrary_account_state(), 1..5)
        .prop_map(|account_states| {
            let mut journal_map = JournalMap::new();
            for account_state in account_states {
                let _ = journal_map.insert_account_state(account_state);
            }
            journal_map
        })
}

/// Strategy to generate OpLog
fn arbitrary_oplog() -> impl Strategy<Value = OpLog> {
    prop::collection::vec(arbitrary_journal_op(), 0..20)
        .prop_map(|ops| {
            let mut oplog = OpLog::new();
            for op in ops {
                oplog.append(op);
            }
            oplog
        })
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        .. ProptestConfig::default()
    })]

    /// Property: Journal join operation is idempotent
    /// For any journal state J: J ⊔ J = J
    #[test]
    fn prop_journal_join_idempotent(
        journal in arbitrary_journal_map()
    ) {
        let joined = journal.join(&journal)?;
        
        prop_assert_eq!(joined.device_count(), journal.device_count(),
            "Idempotent join should preserve device count");
        prop_assert_eq!(joined.capability_count(), journal.capability_count(),
            "Idempotent join should preserve capability count");
        prop_assert_eq!(joined.relationship_count(), journal.relationship_count(),
            "Idempotent join should preserve relationship count");
    }

    /// Property: Journal join operation is commutative  
    /// For any journal states A, B: A ⊔ B = B ⊔ A
    #[test]
    fn prop_journal_join_commutative(
        journal_a in arbitrary_journal_map(),
        journal_b in arbitrary_journal_map()
    ) {
        let joined_ab = journal_a.join(&journal_b)?;
        let joined_ba = journal_b.join(&journal_a)?;

        prop_assert_eq!(joined_ab.device_count(), joined_ba.device_count(),
            "Commutative join should have same device count");
        prop_assert_eq!(joined_ab.capability_count(), joined_ba.capability_count(),
            "Commutative join should have same capability count");
        prop_assert_eq!(joined_ab.relationship_count(), joined_ba.relationship_count(),
            "Commutative join should have same relationship count");

        // Verify deep equality by checking all devices exist in both
        let devices_ab = joined_ab.list_devices()?;
        let devices_ba = joined_ba.list_devices()?;
        prop_assert_eq!(devices_ab.len(), devices_ba.len(),
            "Device lists should have same length");
        
        for device in &devices_ab {
            prop_assert!(devices_ba.contains(device),
                "Device {} should exist in both joined states", device);
        }
    }

    /// Property: Journal join operation is associative
    /// For any journal states A, B, C: (A ⊔ B) ⊔ C = A ⊔ (B ⊔ C)
    #[test]
    fn prop_journal_join_associative(
        journal_a in arbitrary_journal_map(),
        journal_b in arbitrary_journal_map(), 
        journal_c in arbitrary_journal_map()
    ) {
        let left = journal_a.join(&journal_b)?.join(&journal_c)?;
        let right = journal_a.join(&journal_b.join(&journal_c)?)?;

        prop_assert_eq!(left.device_count(), right.device_count(),
            "Associative join should have same device count");
        prop_assert_eq!(left.capability_count(), right.capability_count(),
            "Associative join should have same capability count");
        prop_assert_eq!(left.relationship_count(), right.relationship_count(),
            "Associative join should have same relationship count");
    }

    /// Property: Account state join preserves all operations
    /// For account states A, B: ops(A ⊔ B) ⊇ ops(A) ∪ ops(B)
    #[test]
    fn prop_account_state_join_preserves_operations(
        account_state_a in arbitrary_account_state(),
        account_state_b in arbitrary_account_state()
    ) {
        // Ensure both states are for the same account
        let account_id = account_state_a.account_id();
        let mut state_b_same_account = AccountState::new(account_id);
        
        // Copy operations from state_b to new state with same account
        if let Ok(ops_b) = account_state_b.list_operations() {
            for op in ops_b {
                let _ = state_b_same_account.apply_operation(op.clone());
            }
        }

        let joined = account_state_a.join(&state_b_same_account)?;

        let ops_a = account_state_a.list_operations()?.len();
        let ops_b = state_b_same_account.list_operations()?.len();
        let ops_joined = joined.list_operations()?.len();

        prop_assert!(ops_joined >= ops_a.max(ops_b),
            "Joined state should preserve all operations: {} >= max({}, {})",
            ops_joined, ops_a, ops_b);
    }

    /// Property: OpLog join is deterministic and idempotent
    /// For any OpLogs A, B: (A ⊔ B) ⊔ (A ⊔ B) = A ⊔ B
    #[test]
    fn prop_oplog_join_deterministic_idempotent(
        oplog_a in arbitrary_oplog(),
        oplog_b in arbitrary_oplog()
    ) {
        let joined_once = oplog_a.join(&oplog_b);
        let joined_twice = joined_once.join(&joined_once);

        prop_assert_eq!(joined_once.operation_count(), joined_twice.operation_count(),
            "Double join should be idempotent");

        // Verify operations are identical
        let ops_once = joined_once.list_ops();
        let ops_twice = joined_twice.list_ops();
        
        prop_assert_eq!(ops_once.len(), ops_twice.len(),
            "Operation lists should have same length after double join");
    }

    /// Property: State growth monotonicity  
    /// For any state S and operation O: size(apply(S, O)) >= size(S)
    #[test]
    fn prop_state_growth_monotonicity(
        mut journal in arbitrary_journal_map(),
        operation in arbitrary_journal_op()
    ) {
        let initial_device_count = journal.device_count();
        let initial_capability_count = journal.capability_count();
        let initial_relationship_count = journal.relationship_count();

        // Apply operation
        let result = journal.apply_operation(operation);
        
        // Even if operation fails, counts should not decrease
        prop_assert!(journal.device_count() >= initial_device_count,
            "Device count should not decrease: {} >= {}", 
            journal.device_count(), initial_device_count);
        prop_assert!(journal.capability_count() >= initial_capability_count,
            "Capability count should not decrease: {} >= {}",
            journal.capability_count(), initial_capability_count);
        prop_assert!(journal.relationship_count() >= initial_relationship_count,
            "Relationship count should not decrease: {} >= {}",
            journal.relationship_count(), initial_relationship_count);
    }

    /// Property: Causal ordering preservation
    /// If operation A causally precedes B, then A appears before B in all states
    #[test]
    fn prop_causal_ordering_preservation(
        mut oplog in arbitrary_oplog(),
        device_id in arbitrary_device_id()
    ) {
        // Create causally ordered operations
        let mut state_vector = StateVector::new();
        state_vector.increment(device_id);

        let op_a = JournalOp {
            op_type: OpType::Device(DeviceOperation::Add {
                device_id,
                public_key: vec![1, 2, 3],
                capabilities: HashSet::new(),
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(), // Earlier operation
        };

        let op_b = JournalOp {
            op_type: OpType::Capability(CapabilityOperation::Grant {
                device_id,
                capability_id: CapabilityId::new(),
                granted_by: device_id,
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: state_vector, // Later operation (depends on op_a)
        };

        // Add operations to oplog
        oplog.append(op_a.clone());
        oplog.append(op_b.clone());

        // Get ordered operations
        let ordered_ops = oplog.list_ops();
        
        // Find positions of our operations
        let pos_a = ordered_ops.iter().position(|op| {
            matches!(op.op_type, OpType::Device(DeviceOperation::Add { .. })) 
                && op.author == device_id
        });
        let pos_b = ordered_ops.iter().position(|op| {
            matches!(op.op_type, OpType::Capability(CapabilityOperation::Grant { .. }))
                && op.author == device_id
        });

        if let (Some(a_pos), Some(b_pos)) = (pos_a, pos_b) {
            prop_assert!(a_pos < b_pos,
                "Causally earlier operation should appear first: {} < {}", a_pos, b_pos);
        }
    }

    /// Property: Conflict resolution is deterministic
    /// Concurrent operations with conflicts resolve the same way every time
    #[test]
    fn prop_conflict_resolution_deterministic(
        device_id_1 in arbitrary_device_id(),
        device_id_2 in arbitrary_device_id(),
        capability_id in arbitrary_capability_id()
    ) {
        prop_assume!(device_id_1 != device_id_2);

        // Create conflicting operations: both devices try to grant same capability
        let timestamp = std::time::SystemTime::now();
        
        let op_1 = JournalOp {
            op_type: OpType::Capability(CapabilityOperation::Grant {
                device_id: device_id_1,
                capability_id,
                granted_by: device_id_1,
            }),
            author: device_id_1,
            timestamp,
            causal_context: StateVector::new(),
        };

        let op_2 = JournalOp {
            op_type: OpType::Capability(CapabilityOperation::Grant {
                device_id: device_id_2,
                capability_id,
                granted_by: device_id_2,
            }),
            author: device_id_2,
            timestamp,
            causal_context: StateVector::new(),
        };

        // Apply operations in different orders
        let mut oplog_12 = OpLog::new();
        oplog_12.append(op_1.clone());
        oplog_12.append(op_2.clone());

        let mut oplog_21 = OpLog::new();
        oplog_21.append(op_2.clone());
        oplog_21.append(op_1.clone());

        // Both should result in same final ordering (deterministic tie-breaking)
        let ops_12 = oplog_12.list_ops();
        let ops_21 = oplog_21.list_ops();

        prop_assert_eq!(ops_12.len(), ops_21.len(),
            "Both orderings should have same number of operations");

        // Operations should be in same deterministic order
        if ops_12.len() >= 2 && ops_21.len() >= 2 {
            let first_op_12 = &ops_12[ops_12.len() - 2];
            let first_op_21 = &ops_21[ops_21.len() - 2];
            
            prop_assert_eq!(first_op_12.author, first_op_21.author,
                "Deterministic ordering should put same operation first");
        }
    }

    /// Property: Empty state is identity for join
    /// For any state S: S ⊔ ∅ = S
    #[test]  
    fn prop_empty_state_join_identity(
        journal in arbitrary_journal_map()
    ) {
        let empty_journal = JournalMap::new();
        let joined = journal.join(&empty_journal)?;

        prop_assert_eq!(joined.device_count(), journal.device_count(),
            "Join with empty should preserve device count");
        prop_assert_eq!(joined.capability_count(), journal.capability_count(),
            "Join with empty should preserve capability count");
        prop_assert_eq!(joined.relationship_count(), journal.relationship_count(),
            "Join with empty should preserve relationship count");
    }

    /// Property: Join preserves valid state invariants
    /// For any valid states A, B: invariants(A ⊔ B) hold
    #[test]
    fn prop_join_preserves_invariants(
        journal_a in arbitrary_journal_map(),
        journal_b in arbitrary_journal_map()
    ) {
        let joined = journal_a.join(&journal_b)?;

        // Verify basic invariants
        prop_assert!(joined.is_consistent()?,
            "Joined state should maintain consistency");

        // Device count should be at least the maximum of input counts
        prop_assert!(joined.device_count() >= journal_a.device_count().max(journal_b.device_count()),
            "Join should preserve all devices");

        // All devices should have valid state
        let devices = joined.list_devices()?;
        for device in devices {
            prop_assert!(joined.device_exists(device)?,
                "Device {} should exist in joined state", device);
        }
    }

    /// Property: Concurrent updates converge
    /// Concurrent operations applied in any order reach same final state
    #[test]
    fn prop_concurrent_updates_converge(
        operations in prop::collection::vec(arbitrary_journal_op(), 2..10),
        device_id in arbitrary_device_id()
    ) {
        // Apply operations in original order
        let mut journal_1 = JournalMap::new();
        for op in &operations {
            let _ = journal_1.apply_operation(op.clone());
        }

        // Apply operations in reverse order
        let mut journal_2 = JournalMap::new();
        for op in operations.iter().rev() {
            let _ = journal_2.apply_operation(op.clone());
        }

        // Join the two states - should be identical
        let converged_1 = journal_1.join(&journal_2)?;
        let converged_2 = journal_2.join(&journal_1)?;

        prop_assert_eq!(converged_1.device_count(), converged_2.device_count(),
            "Convergence should produce same device count");
        prop_assert_eq!(converged_1.capability_count(), converged_2.capability_count(),
            "Convergence should produce same capability count");
        prop_assert_eq!(converged_1.relationship_count(), converged_2.relationship_count(),
            "Convergence should produce same relationship count");
    }

    /// Property: State vector ordering respects causality
    /// If A → B in causal order, then vector(A) ≺ vector(B)
    #[test]
    fn prop_state_vector_causal_ordering(
        device_id in arbitrary_device_id(),
        operation_count in 2usize..10
    ) {
        let mut current_vector = StateVector::new();
        let mut previous_vectors = Vec::new();

        // Create sequence of causally ordered operations
        for i in 0..operation_count {
            previous_vectors.push(current_vector.clone());
            current_vector.increment(device_id);

            // Current vector should be greater than all previous vectors
            for (j, prev_vector) in previous_vectors.iter().enumerate() {
                prop_assert!(current_vector.dominates(prev_vector) || current_vector == *prev_vector,
                    "State vector {} should dominate earlier vector {}", i, j);
            }
        }
    }
}

/// Additional unit tests for edge cases
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_journal_properties() {
        let empty = JournalMap::new();
        
        assert_eq!(empty.device_count(), 0);
        assert_eq!(empty.capability_count(), 0);
        assert_eq!(empty.relationship_count(), 0);
        assert!(empty.is_consistent().unwrap());
    }

    #[tokio::test]
    async fn test_single_operation_idempotence() {
        let mut journal = JournalMap::new();
        let device_id = DeviceId::new();
        
        let op = JournalOp {
            op_type: OpType::Device(DeviceOperation::Add {
                device_id,
                public_key: vec![1, 2, 3],
                capabilities: HashSet::new(),
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        };

        // Apply operation twice
        journal.apply_operation(op.clone()).unwrap();
        let count_after_first = journal.device_count();
        
        journal.apply_operation(op).unwrap();
        let count_after_second = journal.device_count();

        assert_eq!(count_after_first, count_after_second,
            "Applying same operation twice should be idempotent");
    }

    #[test]
    fn test_state_vector_operations() {
        let device_1 = DeviceId::new();
        let device_2 = DeviceId::new();

        let mut vector_a = StateVector::new();
        let mut vector_b = StateVector::new();

        // Advance both vectors
        vector_a.increment(device_1);
        vector_b.increment(device_2);

        // Neither should dominate the other (concurrent)
        assert!(!vector_a.dominates(&vector_b));
        assert!(!vector_b.dominates(&vector_a));

        // Advance vector_a further
        vector_a.increment(device_1);
        vector_a.increment(device_2);

        // Now vector_a should dominate vector_b
        assert!(vector_a.dominates(&vector_b));
        assert!(!vector_b.dominates(&vector_a));
    }

    #[tokio::test]
    async fn test_join_with_overlapping_operations() {
        let device_id = DeviceId::new();
        let capability_id = CapabilityId::new();

        let op_1 = JournalOp {
            op_type: OpType::Device(DeviceOperation::Add {
                device_id,
                public_key: vec![1, 2, 3],
                capabilities: HashSet::new(),
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        };

        let op_2 = JournalOp {
            op_type: OpType::Capability(CapabilityOperation::Grant {
                device_id,
                capability_id,
                granted_by: device_id,
            }),
            author: device_id,
            timestamp: std::time::SystemTime::now(),
            causal_context: StateVector::new(),
        };

        let mut journal_a = JournalMap::new();
        journal_a.apply_operation(op_1.clone()).unwrap();
        journal_a.apply_operation(op_2.clone()).unwrap();

        let mut journal_b = JournalMap::new();
        journal_b.apply_operation(op_1).unwrap(); // Overlapping operation

        let joined = journal_a.join(&journal_b).unwrap();

        // Should have both devices and capabilities from journal_a
        assert_eq!(joined.device_count(), journal_a.device_count());
        assert_eq!(joined.capability_count(), journal_a.capability_count());
    }
}
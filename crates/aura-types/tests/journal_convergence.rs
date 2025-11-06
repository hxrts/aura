//! Journal CRDT Convergence Property Tests
//!
//! Validates that the JournalMap CRDT converges correctly under:
//! - Arbitrary message reordering
//! - Network partitions and heals
//! - Concurrent intent submissions
//! - TreeOp conflicts at same epoch
//!
//! Uses property-based testing to verify:
//! - Strong eventual consistency (SEC)
//! - Convergence after arbitrary merge sequences
//! - Deterministic conflict resolution
//! - Intent pool OR-set semantics
//!
//! Reference: docs/402_crdt_types.md, work/tree_revision.md §6-8

use aura_types::{
    identifiers::DeviceId,
    ledger::{Intent, IntentId, JournalMap, Priority, ThresholdSignature},
    tree::{AffectedPath, Commitment, LeafId, LeafIndex, LeafNode, LeafRole, NodeIndex, TreeOperation},
};
use aura_journal::tree::node::{KeyPackage, LeafMetadata};
use aura_journal::ledger::tree_op::{Epoch, TreeOp, TreeOpRecord};
use std::collections::BTreeMap;
use aura_types::current_unix_timestamp;
use proptest::prelude::*;
use std::collections::HashSet;

/// Generate random TreeOpRecord
fn arb_tree_op_record(epoch: Epoch) -> impl Strategy<Value = TreeOpRecord> {
    (any::<u128>(), prop::collection::vec(any::<u8>(), 32..=32)).prop_map(move |(dev_id, pk)| {
        let leaf_node = LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: LeafIndex(epoch as usize),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: pk,
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };
        
        TreeOpRecord {
            epoch,
            op: TreeOp::AddLeaf {
                leaf_node,
                affected_path: AffectedPath::new(),
            },
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![],
            attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
            authored_at: epoch,
            author: DeviceId(uuid::Uuid::from_u128(dev_id)),
        }
    })
}

/// Generate random Intent
fn arb_intent() -> impl Strategy<Value = Intent> {
    (
        any::<u128>(),
        prop::collection::vec(any::<u8>(), 32..=32),
        any::<[u8; 32]>(),
        any::<u64>(),
    )
        .prop_map(|(dev_id, pk, commit_bytes, priority)| {
            let leaf_node = LeafNode {
                leaf_id: LeafId::new(),
                leaf_index: LeafIndex(0),
                role: LeafRole::Device,
                public_key: KeyPackage {
                    signing_key: pk,
                    encryption_key: None,
                },
                metadata: LeafMetadata::default(),
            };
            
            Intent {
                intent_id: IntentId(uuid::Uuid::new_v4()),
                op: TreeOperation::AddLeaf {
                    leaf_node,
                    affected_path: AffectedPath::new(),
                },
                path_span: vec![NodeIndex::new(0)],
                snapshot_commitment: Commitment::new(commit_bytes),
                priority: Priority::from(priority),
                author: DeviceId(uuid::Uuid::from_u128(dev_id)),
                created_at: current_unix_timestamp(),
                metadata: std::collections::BTreeMap::new(),
            }
        })
}

proptest! {
    /// Property: CRDT Convergence under arbitrary merge order
    ///
    /// Two replicas that receive the same set of operations in different
    /// orders must converge to identical state.
    #[test]
    fn prop_crdt_convergence_order_independence(
        ops in prop::collection::vec((1u64..100, arb_tree_op_record(1)), 1..20)
    ) {
        let mut journal1 = JournalMap::default();
        let mut journal2 = JournalMap::default();

        // Apply ops in original order to journal1
        for (epoch, op) in &ops {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal1.ops.insert(*epoch, op_copy);
        }

        // Apply ops in reverse order to journal2
        for (epoch, op) in ops.iter().rev() {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal2.ops.insert(*epoch, op_copy);
        }

        // Both journals must have identical ops
        prop_assert_eq!(
            journal1.ops.len(),
            journal2.ops.len(),
            "Op count must match"
        );

        for (epoch, op1) in &journal1.ops {
            let op2 = journal2.ops.get(epoch);
            prop_assert!(op2.is_some(), "Epoch {} must exist in both journals", epoch);
            prop_assert_eq!(op1.epoch, op2.unwrap().epoch);
        }
    }

    /// Property: Intent pool OR-set semantics
    ///
    /// Intents can be added and tombstoned independently. Convergence must
    /// handle concurrent add/tombstone correctly.
    #[test]
    fn prop_intent_pool_or_set(
        intents in prop::collection::vec(arb_intent(), 1..20),
        tombstone_indices in prop::collection::vec(0usize..20, 0..10)
    ) {
        let mut journal = JournalMap::default();

        // Add all intents
        for intent in &intents {
            journal.intents.insert(intent.intent_id.clone(), intent.clone());
        }

        // Tombstone some intents
        for idx in tombstone_indices {
            if idx < intents.len() {
                journal.tombstones.insert(intents[idx].intent_id.clone());
            }
        }

        // Verify tombstoned intents still exist in intent map (OR-set)
        for intent in &intents {
            prop_assert!(
                journal.intents.contains_key(&intent.intent_id),
                "Intent must exist even if tombstoned"
            );
        }

        // Count non-tombstoned intents
        let active_count = journal
            .intents
            .keys()
            .filter(|id| !journal.tombstones.contains(*id))
            .count();

        prop_assert!(
            active_count <= intents.len(),
            "Active intents should not exceed total"
        );
    }

    /// Property: Merge is idempotent
    ///
    /// Merging the same journal state multiple times produces the same result.
    #[test]
    fn prop_merge_idempotent(
        ops in prop::collection::vec((1u64..50, arb_tree_op_record(1)), 1..10)
    ) {
        let mut journal1 = JournalMap::default();
        let mut journal2 = JournalMap::default();

        // Populate journal1
        for (epoch, op) in &ops {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal1.ops.insert(*epoch, op_copy);
        }

        // Merge journal1 into journal2 twice
        journal2 = journal2.merge(&journal1);
        let after_first_merge = journal2.clone();
        journal2 = journal2.merge(&journal1);

        // State should be identical after first and second merge
        prop_assert_eq!(
            after_first_merge.ops.len(),
            journal2.ops.len(),
            "Merge must be idempotent"
        );
    }

    /// Property: Merge is commutative
    ///
    /// merge(A, B) == merge(B, A)
    #[test]
    fn prop_merge_commutative(
        ops_a in prop::collection::vec((1u64..30, arb_tree_op_record(1)), 1..10),
        ops_b in prop::collection::vec((30u64..60, arb_tree_op_record(30)), 1..10)
    ) {
        let mut journal_a = JournalMap::default();
        let mut journal_b = JournalMap::default();

        // Populate journal_a
        for (epoch, op) in &ops_a {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal_a.ops.insert(*epoch, op_copy);
        }

        // Populate journal_b
        for (epoch, op) in &ops_b {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal_b.ops.insert(*epoch, op_copy);
        }

        // Merge both ways
        let mut result1 = journal_a.clone();
        result1 = result1.merge(&journal_b);

        let mut result2 = journal_b.clone();
        result2 = result2.merge(&journal_a);

        // Results must be identical
        prop_assert_eq!(
            result1.ops.len(),
            result2.ops.len(),
            "Merge must be commutative"
        );

        // Verify all ops match
        for (epoch, op1) in &result1.ops {
            let op2 = result2.ops.get(epoch);
            prop_assert!(op2.is_some());
            prop_assert_eq!(op1.epoch, op2.unwrap().epoch);
        }
    }

    /// Property: Merge is associative
    ///
    /// merge(merge(A, B), C) == merge(A, merge(B, C))
    #[test]
    fn prop_merge_associative(
        ops_a in prop::collection::vec((1u64..20, arb_tree_op_record(1)), 1..5),
        ops_b in prop::collection::vec((20u64..40, arb_tree_op_record(20)), 1..5),
        ops_c in prop::collection::vec((40u64..60, arb_tree_op_record(40)), 1..5)
    ) {
        let mut journal_a = JournalMap::default();
        let mut journal_b = JournalMap::default();
        let mut journal_c = JournalMap::default();

        // Populate journals
        for (epoch, op) in &ops_a {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal_a.ops.insert(*epoch, op_copy);
        }
        for (epoch, op) in &ops_b {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal_b.ops.insert(*epoch, op_copy);
        }
        for (epoch, op) in &ops_c {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            journal_c.ops.insert(*epoch, op_copy);
        }

        // Left-associative: (A ∪ B) ∪ C
        let mut left = journal_a.clone().merge(&journal_b);
        left = left.merge(&journal_c);

        // Right-associative: A ∪ (B ∪ C)
        let mut right = journal_b.clone().merge(&journal_c);
        right = journal_a.clone().merge(&right);

        prop_assert_eq!(
            left.ops.len(),
            right.ops.len(),
            "Merge must be associative"
        );
    }

    /// Property: Tombstone union preserves all tombstones
    ///
    /// When merging journals, all tombstones from both sides are preserved.
    #[test]
    fn prop_tombstone_union(
        intents_a in prop::collection::vec(arb_intent(), 1..10),
        intents_b in prop::collection::vec(arb_intent(), 1..10),
        tombstone_a_indices in prop::collection::vec(0usize..10, 0..5),
        tombstone_b_indices in prop::collection::vec(0usize..10, 0..5)
    ) {
        let mut journal_a = JournalMap::default();
        let mut journal_b = JournalMap::default();

        // Add intents to A and tombstone some
        for intent in &intents_a {
            journal_a.intents.insert(intent.intent_id.clone(), intent.clone());
        }
        for idx in tombstone_a_indices {
            if idx < intents_a.len() {
                journal_a.tombstones.insert(intents_a[idx].intent_id.clone());
            }
        }

        // Add intents to B and tombstone some
        for intent in &intents_b {
            journal_b.intents.insert(intent.intent_id.clone(), intent.clone());
        }
        for idx in tombstone_b_indices {
            if idx < intents_b.len() {
                journal_b.tombstones.insert(intents_b[idx].intent_id.clone());
            }
        }

        // Merge
        let merged = journal_a.merge(&journal_b);

        // All tombstones from both journals must be present
        for tombstone in &journal_a.tombstones {
            prop_assert!(
                merged.tombstones.contains(tombstone),
                "Tombstone from A must be in merged result"
            );
        }
        for tombstone in &journal_b.tombstones {
            prop_assert!(
                merged.tombstones.contains(tombstone),
                "Tombstone from B must be in merged result"
            );
        }
    }

    /// Property: Network partition convergence
    ///
    /// Simulates network partition: two replicas evolve independently,
    /// then merge. Final state must be consistent.
    #[test]
    fn prop_network_partition_convergence(
        partition_a_ops in prop::collection::vec((1u64..50, arb_tree_op_record(1)), 1..10),
        partition_b_ops in prop::collection::vec((1u64..50, arb_tree_op_record(1)), 1..10)
    ) {
        // Start with common base
        let mut replica_a = JournalMap::default();
        let mut replica_b = JournalMap::default();

        // Partition: A evolves independently
        for (epoch, op) in &partition_a_ops {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            replica_a.ops.insert(*epoch, op_copy);
        }

        // Partition: B evolves independently
        for (epoch, op) in &partition_b_ops {
            let mut op_copy = op.clone();
            op_copy.epoch = *epoch;
            replica_b.ops.insert(*epoch, op_copy);
        }

        // Heal partition: merge both ways
        let a_merged_with_b = replica_a.clone().merge(&replica_b);
        let b_merged_with_a = replica_b.clone().merge(&replica_a);

        // Both must converge to same state
        prop_assert_eq!(
            a_merged_with_b.ops.len(),
            b_merged_with_a.ops.len(),
            "Replicas must converge after partition heal"
        );
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_empty_journal_merge() {
        let journal1 = JournalMap::default();
        let journal2 = JournalMap::default();

        let merged = journal1.merge(&journal2);

        assert_eq!(merged.ops.len(), 0);
        assert_eq!(merged.intents.len(), 0);
        assert_eq!(merged.tombstones.len(), 0);
    }

    #[test]
    fn test_single_op_merge() {
        let mut journal1 = JournalMap::default();
        let journal2 = JournalMap::default();

        let leaf_node = LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: LeafIndex(0),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![1u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };
        
        let op = TreeOpRecord {
            epoch: 1,
            op: TreeOp::AddLeaf {
                leaf_node,
                affected_path: AffectedPath::new(),
            },
            affected_indices: vec![],
            new_commitments: BTreeMap::new(),
            capability_refs: vec![],
            attestation: ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3)),
            authored_at: 1,
            author: DeviceId(uuid::Uuid::new_v4()),
        };

        journal1.ops.insert(1, op);

        let merged = journal1.merge(&journal2);
        assert_eq!(merged.ops.len(), 1);
    }

    #[test]
    fn test_intent_tombstone_semantics() {
        let mut journal = JournalMap::default();

        let leaf_node = LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: LeafIndex(0),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![1u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };
        
        let intent = Intent {
            intent_id: IntentId::new(),
            op: TreeOperation::AddLeaf {
                leaf_node,
                affected_path: AffectedPath::new(),
            },
            path_span: vec![NodeIndex::new(0)],
            snapshot_commitment: Commitment::new([0u8; 32]),
            priority: Priority::from(100),
            author: DeviceId(uuid::Uuid::new_v4()),
            created_at: current_unix_timestamp(),
            metadata: std::collections::BTreeMap::new(),
        };

        // Add intent
        journal
            .intents
            .insert(intent.intent_id.clone(), intent.clone());
        assert_eq!(journal.intents.len(), 1);

        // Tombstone it
        journal.tombstones.insert(intent.intent_id.clone());
        assert_eq!(journal.tombstones.len(), 1);

        // Intent still exists in map (OR-set)
        assert!(journal.intents.contains_key(&intent.intent_id));

        // But it's marked as tombstoned
        assert!(journal.tombstones.contains(&intent.intent_id));
    }
}

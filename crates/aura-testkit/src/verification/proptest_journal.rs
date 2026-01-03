//! Proptest strategies for Lean-compatible journal types.
//!
//! These strategies generate random instances of journal types that match
//! the Lean verification model's JSON schema, enabling differential testing.
//!
//! ## Usage
//!
//! ```ignore
//! use aura_testkit::verification::proptest_journal::*;
//! use proptest::prelude::*;
//!
//! proptest! {
//!     #[test]
//!     fn test_journal_merge(j1 in journal_strategy(), j2 in journal_strategy()) {
//!         // Ensure same namespace for merge
//!         let j2 = LeanJournal { namespace: j1.namespace.clone(), ..j2 };
//!         // Test merge...
//!     }
//! }
//! ```

use super::lean_types::*;
use proptest::prelude::*;

// ============================================================================
// Foundation Types
// ============================================================================

/// Strategy for generating random 32-byte arrays.
pub fn byte_array32_strategy() -> impl Strategy<Value = ByteArray32> {
    prop::collection::vec(any::<u8>(), 32).prop_map(|bytes| {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        ByteArray32::new(arr)
    })
}

/// Strategy for generating OrderTime values.
pub fn order_time_strategy() -> impl Strategy<Value = OrderTime> {
    byte_array32_strategy().prop_map(|b| OrderTime(b))
}

/// Strategy for generating incrementing OrderTime values.
/// Useful for creating ordered fact sequences.
pub fn ordered_order_times(count: usize) -> impl Strategy<Value = Vec<OrderTime>> {
    prop::collection::vec(any::<u64>(), count).prop_map(|seeds| {
        let mut times: Vec<OrderTime> = seeds
            .into_iter()
            .enumerate()
            .map(|(i, seed)| {
                let mut bytes = [0u8; 32];
                // Use index as primary ordering, seed as secondary
                bytes[0..8].copy_from_slice(&(i as u64).to_be_bytes());
                bytes[8..16].copy_from_slice(&seed.to_be_bytes());
                OrderTime::new(bytes)
            })
            .collect();
        times.sort();
        times
    })
}

// ============================================================================
// TimeStamp Strategies
// ============================================================================

/// Strategy for OrderClock timestamps.
pub fn order_clock_timestamp_strategy() -> impl Strategy<Value = LeanTimeStamp> {
    order_time_strategy().prop_map(LeanTimeStamp::order_clock)
}

/// Strategy for Physical timestamps.
pub fn physical_timestamp_strategy() -> impl Strategy<Value = LeanTimeStamp> {
    (0u64..u64::MAX, prop::option::of(0u64..10000))
        .prop_map(|(ts_ms, uncertainty)| LeanTimeStamp::physical(ts_ms, uncertainty))
}

/// Strategy for Logical timestamps.
pub fn logical_timestamp_strategy() -> impl Strategy<Value = LeanTimeStamp> {
    (0u64..u64::MAX).prop_map(LeanTimeStamp::logical)
}

/// Strategy for Range timestamps.
pub fn range_timestamp_strategy() -> impl Strategy<Value = LeanTimeStamp> {
    (0u64..u64::MAX / 2).prop_flat_map(|start| {
        (Just(start), start..u64::MAX).prop_map(|(s, e)| LeanTimeStamp::range(s, e))
    })
}

/// Strategy for any TimeStamp variant.
pub fn timestamp_strategy() -> impl Strategy<Value = LeanTimeStamp> {
    prop_oneof![
        order_clock_timestamp_strategy(),
        physical_timestamp_strategy(),
        logical_timestamp_strategy(),
        range_timestamp_strategy(),
    ]
}

// ============================================================================
// Namespace Strategies
// ============================================================================

/// Strategy for Authority namespace.
pub fn authority_namespace_strategy() -> impl Strategy<Value = LeanNamespace> {
    byte_array32_strategy().prop_map(|id| LeanNamespace::Authority { id })
}

/// Strategy for Context namespace.
pub fn context_namespace_strategy() -> impl Strategy<Value = LeanNamespace> {
    byte_array32_strategy().prop_map(|id| LeanNamespace::Context { id })
}

/// Strategy for any namespace.
pub fn namespace_strategy() -> impl Strategy<Value = LeanNamespace> {
    prop_oneof![authority_namespace_strategy(), context_namespace_strategy(),]
}

/// Fixed namespace for testing same-namespace merges.
pub fn fixed_namespace() -> LeanNamespace {
    LeanNamespace::Authority {
        id: ByteArray32::zero(),
    }
}

// ============================================================================
// TreeOp Strategies
// ============================================================================

/// Strategy for LeafRole.
/// Note: Only generates variants that exist in Lean model (Device, Guardian).
pub fn leaf_role_strategy() -> impl Strategy<Value = LeafRole> {
    prop_oneof![
        Just(LeafRole::Device),
        Just(LeafRole::Guardian),
        // Note: Delegate is not in the Lean model, only Device and Guardian
    ]
}

/// Strategy for TreeOpKind.
pub fn tree_op_kind_strategy() -> impl Strategy<Value = TreeOpKind> {
    prop_oneof![
        (
            prop::collection::vec(any::<u8>(), 32..64),
            leaf_role_strategy()
        )
            .prop_map(|(public_key, role)| TreeOpKind::AddLeaf { public_key, role }),
        (0u64..1000).prop_map(|leaf_index| TreeOpKind::RemoveLeaf { leaf_index }),
        (1u64..10).prop_map(|threshold| TreeOpKind::UpdatePolicy { threshold }),
        Just(TreeOpKind::RotateEpoch),
    ]
}

/// Strategy for AttestedOp.
pub fn attested_op_strategy() -> impl Strategy<Value = AttestedOp> {
    (
        tree_op_kind_strategy(),
        byte_array32_strategy(),
        byte_array32_strategy(),
        1u64..10,
        prop::collection::vec(any::<u8>(), 64..128),
    )
        .prop_map(
            |(tree_op, parent_commitment, new_commitment, witness_threshold, signature)| {
                AttestedOp {
                    tree_op,
                    parent_commitment,
                    new_commitment,
                    witness_threshold,
                    signature,
                }
            },
        )
}

// ============================================================================
// Protocol Relational Fact Strategies
// ============================================================================

/// Strategy for ChannelCheckpoint.
pub fn channel_checkpoint_strategy() -> impl Strategy<Value = ChannelCheckpoint> {
    (
        byte_array32_strategy(),
        0u64..10000,
        byte_array32_strategy(),
    )
        .prop_map(|(channel_id, sequence, state_hash)| ChannelCheckpoint {
            channel_id,
            sequence,
            state_hash,
        })
}

/// Strategy for SnapshotFact.
pub fn snapshot_fact_strategy() -> impl Strategy<Value = SnapshotFact> {
    (
        byte_array32_strategy(),
        prop::collection::vec(order_time_strategy(), 0..5),
        0u64..10000,
    )
        .prop_map(|(state_hash, superseded_facts, sequence)| SnapshotFact {
            state_hash,
            superseded_facts,
            sequence,
        })
}

/// Strategy for a subset of ProtocolRelationalFact (only variants with matching Lean field names).
/// Note: AmpChannelCheckpoint and other complex variants are excluded due to field name
/// differences between Rust and Lean models.
pub fn protocol_relational_fact_strategy() -> impl Strategy<Value = ProtocolRelationalFact> {
    prop_oneof![
        (
            byte_array32_strategy(),
            byte_array32_strategy(),
            byte_array32_strategy()
        )
            .prop_map(|(account_id, guardian_id, binding_hash)| {
                ProtocolRelationalFact::GuardianBinding {
                    account_id,
                    guardian_id,
                    binding_hash,
                }
            }),
        (
            byte_array32_strategy(),
            byte_array32_strategy(),
            byte_array32_strategy()
        )
            .prop_map(|(account_id, guardian_id, grant_hash)| {
                ProtocolRelationalFact::RecoveryGrant {
                    account_id,
                    guardian_id,
                    grant_hash,
                }
            }),
        (
            byte_array32_strategy(),
            byte_array32_strategy(),
            any::<bool>(),
            0u64..20
        )
            .prop_map(
                |(consensus_id, operation_hash, threshold_met, participant_count)| {
                    ProtocolRelationalFact::Consensus {
                        consensus_id,
                        operation_hash,
                        threshold_met,
                        participant_count,
                    }
                }
            ),
        // Note: AmpChannelCheckpoint excluded - Rust uses channel_id/sequence/state_hash
        // but Lean uses channel/chan_epoch/ck_commitment
    ]
}

/// Strategy for RelationalFact.
pub fn relational_fact_strategy() -> impl Strategy<Value = RelationalFact> {
    prop_oneof![
        protocol_relational_fact_strategy().prop_map(|data| RelationalFact::Protocol { data }),
        (
            byte_array32_strategy(),
            "[a-z]{4,10}",
            prop::collection::vec(any::<u8>(), 0..100)
        )
            .prop_map(|(context_id, binding_type, binding_data)| {
                RelationalFact::Generic {
                    context_id,
                    binding_type,
                    binding_data,
                }
            }),
    ]
}

// ============================================================================
// FactContent Strategies
// ============================================================================

/// Strategy for AttestedOp content.
pub fn attested_op_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    attested_op_strategy().prop_map(|data| LeanFactContent::AttestedOp { data })
}

/// Strategy for Relational content.
pub fn relational_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    relational_fact_strategy().prop_map(|data| LeanFactContent::Relational { data })
}

/// Strategy for Snapshot content.
pub fn snapshot_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    snapshot_fact_strategy().prop_map(|data| LeanFactContent::Snapshot { data })
}

/// Strategy for RendezvousReceipt content.
pub fn rendezvous_receipt_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    (
        byte_array32_strategy(),
        byte_array32_strategy(),
        timestamp_strategy(),
        prop::collection::vec(any::<u8>(), 64..128),
    )
        .prop_map(|(envelope_id, authority_id, timestamp, signature)| {
            LeanFactContent::RendezvousReceipt {
                envelope_id,
                authority_id,
                timestamp,
                signature,
            }
        })
}

/// Strategy for any FactContent variant.
pub fn fact_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    prop_oneof![
        attested_op_content_strategy(),
        relational_content_strategy(),
        snapshot_content_strategy(),
        rendezvous_receipt_content_strategy(),
    ]
}

/// Strategy for FactContent variants that can be converted to Rust types.
/// Excludes Relational facts which have structural differences between Lean and Rust.
pub fn convertible_fact_content_strategy() -> impl Strategy<Value = LeanFactContent> {
    prop_oneof![
        attested_op_content_strategy(),
        snapshot_content_strategy(),
        rendezvous_receipt_content_strategy(),
    ]
}

/// Strategy for simple snapshot content (for basic testing).
pub fn simple_snapshot_content() -> impl Strategy<Value = LeanFactContent> {
    (byte_array32_strategy(), 0u64..10000).prop_map(|(state_hash, sequence)| {
        LeanFactContent::Snapshot {
            data: SnapshotFact {
                state_hash,
                superseded_facts: vec![],
                sequence,
            },
        }
    })
}

// ============================================================================
// Fact Strategies
// ============================================================================

/// Strategy for generating a full LeanFact.
pub fn fact_strategy() -> impl Strategy<Value = LeanFact> {
    (
        order_time_strategy(),
        timestamp_strategy(),
        fact_content_strategy(),
    )
        .prop_map(|(order, timestamp, content)| LeanFact::new(order, timestamp, content))
}

/// Strategy for generating a LeanFact that can be converted to Rust types.
pub fn convertible_fact_strategy() -> impl Strategy<Value = LeanFact> {
    (
        order_time_strategy(),
        timestamp_strategy(),
        convertible_fact_content_strategy(),
    )
        .prop_map(|(order, timestamp, content)| LeanFact::new(order, timestamp, content))
}

/// Strategy for generating a simple fact (snapshot content only).
pub fn simple_fact_strategy() -> impl Strategy<Value = LeanFact> {
    (
        order_time_strategy(),
        logical_timestamp_strategy(),
        simple_snapshot_content(),
    )
        .prop_map(|(order, timestamp, content)| LeanFact::new(order, timestamp, content))
}

/// Strategy for generating ordered facts (with incrementing OrderTime).
pub fn ordered_facts_strategy(count: usize) -> impl Strategy<Value = Vec<LeanFact>> {
    (
        ordered_order_times(count),
        prop::collection::vec(timestamp_strategy(), count),
        prop::collection::vec(fact_content_strategy(), count),
    )
        .prop_map(|(orders, timestamps, contents)| {
            orders
                .into_iter()
                .zip(timestamps)
                .zip(contents)
                .map(|((order, timestamp), content)| LeanFact::new(order, timestamp, content))
                .collect()
        })
}

// ============================================================================
// Journal Strategies
// ============================================================================

/// Strategy for generating an empty journal.
pub fn empty_journal_strategy() -> impl Strategy<Value = LeanJournal> {
    namespace_strategy().prop_map(LeanJournal::empty)
}

/// Strategy for generating a journal with random facts.
pub fn journal_strategy() -> impl Strategy<Value = LeanJournal> {
    (
        namespace_strategy(),
        prop::collection::vec(fact_strategy(), 0..10),
    )
        .prop_map(|(namespace, facts)| LeanJournal::new(namespace, facts))
}

/// Strategy for generating a journal with simple facts.
pub fn simple_journal_strategy() -> impl Strategy<Value = LeanJournal> {
    (
        namespace_strategy(),
        prop::collection::vec(simple_fact_strategy(), 0..10),
    )
        .prop_map(|(namespace, facts)| LeanJournal::new(namespace, facts))
}

/// Strategy for generating two journals with the same namespace (for merge testing).
pub fn same_namespace_journals_strategy() -> impl Strategy<Value = (LeanJournal, LeanJournal)> {
    namespace_strategy().prop_flat_map(|ns| {
        (
            prop::collection::vec(fact_strategy(), 0..10),
            prop::collection::vec(fact_strategy(), 0..10),
        )
            .prop_map(move |(facts1, facts2)| {
                (
                    LeanJournal::new(ns.clone(), facts1),
                    LeanJournal::new(ns.clone(), facts2),
                )
            })
    })
}

/// Strategy for generating two journals with convertible facts (for differential testing).
/// These can be converted to Rust types for comparison with the Rust CRDT implementation.
pub fn convertible_journals_strategy() -> impl Strategy<Value = (LeanJournal, LeanJournal)> {
    namespace_strategy().prop_flat_map(|ns| {
        (
            prop::collection::vec(convertible_fact_strategy(), 0..10),
            prop::collection::vec(convertible_fact_strategy(), 0..10),
        )
            .prop_map(move |(facts1, facts2)| {
                (
                    LeanJournal::new(ns.clone(), facts1),
                    LeanJournal::new(ns.clone(), facts2),
                )
            })
    })
}

/// Strategy for generating a single convertible journal (for CRDT law testing).
pub fn convertible_journal_strategy() -> impl Strategy<Value = LeanJournal> {
    (
        namespace_strategy(),
        prop::collection::vec(convertible_fact_strategy(), 0..10),
    )
        .prop_map(|(namespace, facts)| LeanJournal::new(namespace, facts))
}

/// Strategy for generating two journals with different namespaces.
pub fn different_namespace_journals_strategy() -> impl Strategy<Value = (LeanJournal, LeanJournal)>
{
    (
        authority_namespace_strategy(),
        context_namespace_strategy(),
        prop::collection::vec(simple_fact_strategy(), 0..5),
        prop::collection::vec(simple_fact_strategy(), 0..5),
    )
        .prop_map(|(ns1, ns2, facts1, facts2)| {
            (LeanJournal::new(ns1, facts1), LeanJournal::new(ns2, facts2))
        })
}

/// Strategy for generating a journal with ordered facts.
pub fn ordered_journal_strategy() -> impl Strategy<Value = LeanJournal> {
    (namespace_strategy(), ordered_facts_strategy(5))
        .prop_map(|(namespace, facts)| LeanJournal::new(namespace, facts))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a minimal fact for testing.
pub fn minimal_fact(seq: u64) -> LeanFact {
    let mut order_bytes = [0u8; 32];
    order_bytes[0..8].copy_from_slice(&seq.to_be_bytes());

    LeanFact::new(
        OrderTime::new(order_bytes),
        LeanTimeStamp::logical(seq),
        LeanFactContent::Snapshot {
            data: SnapshotFact {
                state_hash: ByteArray32::zero(),
                superseded_facts: vec![],
                sequence: seq,
            },
        },
    )
}

/// Create a minimal journal with n facts.
pub fn minimal_journal(namespace: LeanNamespace, n: usize) -> LeanJournal {
    let facts: Vec<LeanFact> = (0..n as u64).map(minimal_fact).collect();
    LeanJournal::new(namespace, facts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    #[test]
    fn test_byte_array32_strategy_produces_valid() {
        let mut runner = TestRunner::default();
        let strategy = byte_array32_strategy();

        for _ in 0..10 {
            let value = strategy.new_tree(&mut runner).unwrap().current();
            assert_eq!(value.0.len(), 32);
        }
    }

    #[test]
    fn test_ordered_order_times_are_sorted() {
        let mut runner = TestRunner::default();
        let strategy = ordered_order_times(5);

        for _ in 0..10 {
            let times = strategy.new_tree(&mut runner).unwrap().current();
            let mut sorted = times.clone();
            sorted.sort();
            assert_eq!(times, sorted);
        }
    }

    #[test]
    fn test_same_namespace_journals_have_matching_ns() {
        let mut runner = TestRunner::default();
        let strategy = same_namespace_journals_strategy();

        for _ in 0..10 {
            let (j1, j2) = strategy.new_tree(&mut runner).unwrap().current();
            assert_eq!(j1.namespace, j2.namespace);
        }
    }

    #[test]
    fn test_fact_json_roundtrip() {
        let mut runner = TestRunner::default();
        let strategy = fact_strategy();

        for _ in 0..10 {
            let fact = strategy.new_tree(&mut runner).unwrap().current();
            let json = serde_json::to_string(&fact).unwrap();
            let parsed: LeanFact = serde_json::from_str(&json).unwrap();
            assert_eq!(fact, parsed);
        }
    }

    #[test]
    fn test_journal_json_roundtrip() {
        let mut runner = TestRunner::default();
        let strategy = journal_strategy();

        for _ in 0..10 {
            let journal = strategy.new_tree(&mut runner).unwrap().current();
            let json = serde_json::to_string(&journal).unwrap();
            let parsed: LeanJournal = serde_json::from_str(&json).unwrap();
            assert_eq!(journal, parsed);
        }
    }
}

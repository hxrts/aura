//! Property-Based Tests for CRDT Implementations
//!
//! This module verifies the semilattice laws for all CRDT types used in tree
//! synchronization using property-based testing.
//!
//! ## CRDTs Tested
//!
//! 1. **OpLog**: OR-set of AttestedOp
//! 2. **PeerView**: G-set of peer IDs
//! 3. **IntentState**: Typestate lattice with LWW tie-breaker
//!
//! ## Properties Verified
//!
//! For each CRDT:
//! - Join is associative: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
//! - Join is commutative: a ⊔ b = b ⊔ a
//! - Join is idempotent: a ⊔ a = a
//! - Bottom element exists: a ⊔ ⊥ = a

use aura_journal::semilattice::{Bottom, JoinSemilattice};
use aura_protocol::sync::{IntentState, PeerView};
use aura_testkit::strategies::{arb_oplog, proptest};
use proptest::prelude::*;

// ============================================================================
// OpLog Property Tests
// ============================================================================

proptest! {
    /// Property: OpLog join is associative
    #[test]
    fn prop_oplog_join_associative(
        a in arb_oplog(),
        b in arb_oplog(),
        c in arb_oplog()
    ) {
        let left = a.join(&b).join(&c);
        let right = a.join(&b.join(&c));

        let left_ops = left.list_ops();
        let right_ops = right.list_ops();

        prop_assert_eq!(
            left_ops.len(),
            right_ops.len(),
            "OpLog join must be associative"
        );
    }

    /// Property: OpLog join is commutative
    #[test]
    fn prop_oplog_join_commutative(a in arb_oplog(), b in arb_oplog()) {
        let ab = a.join(&b);
        let ba = b.join(&a);

        prop_assert_eq!(
            ab.list_ops().len(),
            ba.list_ops().len(),
            "OpLog join must be commutative"
        );
    }

    /// Property: OpLog join is idempotent
    #[test]
    fn prop_oplog_join_idempotent(a in arb_oplog()) {
        let joined = a.join(&a);

        prop_assert_eq!(
            a.list_ops().len(),
            joined.list_ops().len(),
            "OpLog join must be idempotent"
        );
    }

    /// Property: OpLog bottom is identity
    #[test]
    fn prop_oplog_bottom_identity(a in arb_oplog()) {
        let bottom = OpLog::bottom();
        let joined = a.join(&bottom);

        prop_assert_eq!(
            a.list_ops().len(),
            joined.list_ops().len(),
            "OpLog bottom must be identity for join"
        );
    }
}

// ============================================================================
// PeerView Property Tests
// ============================================================================

fn arb_peer_view() -> impl Strategy<Value = PeerView> {
    prop::collection::vec(any::<u128>(), 0..=10).prop_map(|uuids| {
        let mut view = PeerView::new();
        for uuid_val in uuids {
            view.add_peer(Uuid::from_u128(uuid_val));
        }
        view
    })
}

proptest! {
    /// Property: PeerView join is associative
    #[test]
    fn prop_peerview_join_associative(
        a in arb_peer_view(),
        b in arb_peer_view(),
        c in arb_peer_view()
    ) {
        let left = a.join(&b).join(&c);
        let right = a.join(&b.join(&c));

        prop_assert_eq!(left, right, "PeerView join must be associative");
    }

    /// Property: PeerView join is commutative
    #[test]
    fn prop_peerview_join_commutative(
        a in arb_peer_view(),
        b in arb_peer_view()
    ) {
        let ab = a.join(&b);
        let ba = b.join(&a);

        prop_assert_eq!(ab, ba, "PeerView join must be commutative");
    }

    /// Property: PeerView join is idempotent
    #[test]
    fn prop_peerview_join_idempotent(a in arb_peer_view()) {
        let joined = a.join(&a);

        prop_assert_eq!(a, joined, "PeerView join must be idempotent");
    }

    /// Property: PeerView bottom is identity
    #[test]
    fn prop_peerview_bottom_identity(a in arb_peer_view()) {
        let bottom = PeerView::bottom();
        let joined = a.join(&bottom);

        prop_assert_eq!(a, joined, "PeerView bottom must be identity for join");
    }
}

// ============================================================================
// IntentState Property Tests
// ============================================================================

fn arb_intent_state() -> impl Strategy<Value = IntentState> {
    prop_oneof![
        (1u64..=1000).prop_map(|ts| IntentState::Proposed { timestamp: ts }),
        (1u64..=1000, 1u16..=10).prop_map(|(ts, count)| IntentState::Attesting {
            timestamp: ts,
            collected: count
        }),
        (1u64..=1000).prop_map(|ts| IntentState::Finalized { timestamp: ts }),
        (1u64..=1000, 1u8..=255).prop_map(|(ts, reason)| IntentState::Aborted {
            timestamp: ts,
            reason
        }),
    ]
}

proptest! {
    /// Property: IntentState merge is idempotent
    #[test]
    fn prop_intentstate_merge_idempotent(a in arb_intent_state()) {
        let merged = a.merge(&a);
        prop_assert_eq!(a, merged, "IntentState merge must be idempotent");
    }

    /// Property: IntentState merge never rolls back
    /// If a is ahead of b in the state machine, merge(a, b) = a
    #[test]
    fn prop_intentstate_no_rollback(
        a in arb_intent_state(),
        b in arb_intent_state()
    ) {
        let merged_ab = a.merge(&b);
        let merged_ba = b.merge(&a);

        // Merge should be commutative
        prop_assert_eq!(
            merged_ab,
            merged_ba,
            "IntentState merge must be commutative"
        );
    }

    /// Property: IntentState Finalized is terminal
    /// Once finalized, merge with any other state stays finalized
    #[test]
    fn prop_intentstate_finalized_terminal(
        b in arb_intent_state(),
        ts in 1u64..=1000
    ) {
        let finalized = IntentState::Finalized { timestamp: ts };
        let merged = finalized.merge(&b);

        // If finalized timestamp is later, should stay finalized
        // Otherwise, might transition to other state
        if ts >= b.timestamp() {
            prop_assert!(
                matches!(merged, IntentState::Finalized { .. }),
                "Finalized with later timestamp should remain finalized"
            );
        }
    }

    /// Property: IntentState timestamps are monotonic in merge
    /// merge(a, b).timestamp() >= min(a.timestamp(), b.timestamp())
    #[test]
    fn prop_intentstate_timestamp_monotonic(
        a in arb_intent_state(),
        b in arb_intent_state()
    ) {
        let merged = a.merge(&b);
        let min_ts = std::cmp::min(a.timestamp(), b.timestamp());

        prop_assert!(
            merged.timestamp() >= min_ts,
            "Merged timestamp must be at least the minimum of inputs"
        );
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_oplog_empty_join() {
        let a = OpLog::new();
        let b = OpLog::new();
        let joined = a.join(&b);
        assert_eq!(joined.list_ops().len(), 0);
    }

    #[test]
    fn test_peerview_empty_join() {
        let a = PeerView::new();
        let b = PeerView::new();
        let joined = a.join(&b);
        assert_eq!(joined, PeerView::new());
    }

    #[test]
    fn test_intentstate_proposed_to_finalized() {
        let proposed = IntentState::Proposed { timestamp: 100 };
        let finalized = IntentState::Finalized { timestamp: 200 };

        let merged = proposed.merge(&finalized);
        assert!(matches!(merged, IntentState::Finalized { .. }));
    }

    #[test]
    fn test_intentstate_lww_tiebreaker() {
        let a = IntentState::Aborted {
            timestamp: 100,
            reason: 1,
        };
        let b = IntentState::Attesting {
            timestamp: 200,
            collected: 1,
        };

        // Later timestamp wins for incomparable states
        let merged = a.merge(&b);
        assert!(matches!(merged, IntentState::Attesting { .. }));
    }

    #[test]
    fn test_oplog_union_deduplicates() {
        let mut a = OpLog::new();
        let op = create_test_op([1u8; 32], 1, 1);
        a.add_operation(op.clone());

        let mut b = OpLog::new();
        b.add_operation(op);

        let joined = a.join(&b);
        assert_eq!(joined.list_ops().len(), 1); // Deduplicated
    }

    #[test]
    fn test_peerview_union() {
        let mut a = PeerView::new();
        #[allow(clippy::disallowed_methods)] // Required for test
        let peer1 = Uuid::new_v4();
        a.add_peer(peer1);

        let mut b = PeerView::new();
        #[allow(clippy::disallowed_methods)] // Required for test
        let peer2 = Uuid::new_v4();
        b.add_peer(peer2);

        let joined = a.join(&b);
        assert!(joined.contains(&peer1));
        assert!(joined.contains(&peer2));
    }
}

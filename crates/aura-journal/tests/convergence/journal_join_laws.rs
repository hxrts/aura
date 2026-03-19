//! Journal CRDT join-law property tests.
//!
//! If journal join is not associative, commutative, or idempotent, replicas
//! that merge facts in different orders will diverge — the fundamental CRDT
//! convergence guarantee breaks.

use aura_core::{
    time::{OrderTime, TimeStamp},
    AuthorityId, ContextId, Hash32, JoinSemilattice,
};
use aura_journal::{
    fact::{
        AttestedOp, Fact, FactContent, Journal, JournalNamespace, ProtocolRelationalFact,
        RelationalFact, TreeOpKind,
    },
    reduction::{reduce_authority, reduce_context, RelationalBinding, RelationalState},
};
use proptest::prelude::*;

fn fact_from_seed(seed: [u8; 32]) -> Fact {
    let order = OrderTime(seed);
    let timestamp = TimeStamp::OrderClock(order.clone());
    let account_id = AuthorityId::new_from_entropy(seed);
    let mut guardian_seed = seed;
    guardian_seed[0] ^= 0x5a;
    let guardian_id = AuthorityId::new_from_entropy(guardian_seed);
    let binding_hash = Hash32::new(seed);
    let content = FactContent::Relational(RelationalFact::Protocol(
        ProtocolRelationalFact::GuardianBinding {
            account_id,
            guardian_id,
            binding_hash,
        },
    ));
    Fact::new(order, timestamp, content)
}

fn journal_from_seeds(namespace: JournalNamespace, seeds: &[[u8; 32]]) -> Journal {
    let mut journal = Journal::new(namespace);
    for seed in seeds {
        let _ = journal.add_fact(fact_from_seed(*seed));
    }
    journal
}

fn normalize_bindings(bindings: &[RelationalBinding]) -> Vec<(String, [u8; 16], Vec<u8>)> {
    let mut entries: Vec<_> = bindings
        .iter()
        .map(|binding| {
            (
                format!("{:?}", binding.binding_type),
                binding.context_id.to_bytes(),
                binding.data.clone(),
            )
        })
        .collect();
    entries.sort();
    entries
}

fn assert_relational_state_eq(left: &RelationalState, right: &RelationalState) {
    assert_eq!(
        normalize_bindings(&left.bindings),
        normalize_bindings(&right.bindings)
    );
    assert_eq!(left.flow_budgets, right.flow_budgets);
    assert_eq!(
        left.leakage_budget.external_consumed,
        right.leakage_budget.external_consumed
    );
    assert_eq!(
        left.leakage_budget.neighbor_consumed,
        right.leakage_budget.neighbor_consumed
    );
    assert_eq!(
        left.leakage_budget.in_group_consumed,
        right.leakage_budget.in_group_consumed
    );
    assert_eq!(left.channel_epochs, right.channel_epochs);
}

proptest! {
    #[test]
    fn journal_join_commutative(
        seeds_a in proptest::collection::vec(any::<[u8; 32]>(), 0..20),
        seeds_b in proptest::collection::vec(any::<[u8; 32]>(), 0..20),
    ) {
        let namespace = JournalNamespace::Context(ContextId::new_from_entropy([1u8; 32]));
        let journal_a = journal_from_seeds(namespace.clone(), &seeds_a);
        let journal_b = journal_from_seeds(namespace, &seeds_b);

        prop_assert_eq!(journal_a.join(&journal_b), journal_b.join(&journal_a));
    }

    #[test]
    fn journal_join_associative(
        seeds_a in proptest::collection::vec(any::<[u8; 32]>(), 0..16),
        seeds_b in proptest::collection::vec(any::<[u8; 32]>(), 0..16),
        seeds_c in proptest::collection::vec(any::<[u8; 32]>(), 0..16),
    ) {
        let namespace = JournalNamespace::Context(ContextId::new_from_entropy([2u8; 32]));
        let journal_a = journal_from_seeds(namespace.clone(), &seeds_a);
        let journal_b = journal_from_seeds(namespace.clone(), &seeds_b);
        let journal_c = journal_from_seeds(namespace, &seeds_c);

        prop_assert_eq!(
            journal_a.join(&journal_b).join(&journal_c),
            journal_a.join(&journal_b.join(&journal_c))
        );
    }

    #[test]
    fn journal_join_idempotent(seeds in proptest::collection::vec(any::<[u8; 32]>(), 0..20)) {
        let namespace = JournalNamespace::Context(ContextId::new_from_entropy([3u8; 32]));
        let journal = journal_from_seeds(namespace, &seeds);

        prop_assert_eq!(journal.join(&journal), journal);
    }

    #[test]
    fn reduce_context_is_deterministic(seeds in proptest::collection::vec(any::<[u8; 32]>(), 0..20)) {
        let context_id = ContextId::new_from_entropy([9u8; 32]);
        let namespace = JournalNamespace::Context(context_id);

        let mut journal_a = Journal::new(namespace.clone());
        for seed in seeds.iter() {
            let _ = journal_a.add_fact(fact_from_seed(*seed));
        }

        let mut journal_b = Journal::new(namespace);
        for seed in seeds.iter().rev() {
            let _ = journal_b.add_fact(fact_from_seed(*seed));
        }

        let state_a = reduce_context(&journal_a).expect("reduce_context should succeed");
        let state_b = reduce_context(&journal_b).expect("reduce_context should succeed");

        assert_relational_state_eq(&state_a, &state_b);
    }
}

// ============================================================================
// Authority reducer determinism
//
// reduce_authority() extracts AttestedOp facts and applies them. Since Journal
// uses a BTreeSet, iteration order is canonical. This test verifies that
// inserting facts in random order still produces the same AuthorityState.
// ============================================================================

fn authority_fact_from_seed(seed: u8) -> Fact {
    let mut order_bytes = [0u8; 32];
    order_bytes[0] = seed;
    let order = OrderTime(order_bytes);
    let timestamp = TimeStamp::OrderClock(order.clone());

    let mut parent = [0u8; 32];
    parent[0] = seed.wrapping_sub(1);
    let mut new_commit = [0u8; 32];
    new_commit[0] = seed;

    let content = FactContent::AttestedOp(AttestedOp {
        tree_op: TreeOpKind::AddLeaf {
            public_key: vec![seed; 32],
            role: aura_core::tree::LeafRole::Device,
        },
        parent_commitment: Hash32::new(parent),
        new_commitment: Hash32::new(new_commit),
        witness_threshold: 1,
        signature: vec![seed],
    });
    Fact::new(order, timestamp, content)
}

/// Authority reduction must produce the same state regardless of fact
/// insertion order. If it doesn't, replicas derive different signing key
/// trees from the same facts — threshold signatures break.
#[test]
fn reduce_authority_is_deterministic_across_insertion_orders() {
    let auth_id = AuthorityId::new_from_entropy([77u8; 32]);

    // Create facts with distinct seeds
    let seeds: Vec<u8> = (1..=8).collect();

    // Insert in forward order
    let mut journal_fwd = Journal::new(JournalNamespace::Authority(auth_id));
    for &seed in &seeds {
        let _ = journal_fwd.add_fact(authority_fact_from_seed(seed));
    }

    // Insert in reverse order
    let mut journal_rev = Journal::new(JournalNamespace::Authority(auth_id));
    for &seed in seeds.iter().rev() {
        let _ = journal_rev.add_fact(authority_fact_from_seed(seed));
    }

    // Insert in interleaved order
    let mut journal_interleaved = Journal::new(JournalNamespace::Authority(auth_id));
    for &seed in &[4, 1, 7, 2, 5, 8, 3, 6] {
        let _ = journal_interleaved.add_fact(authority_fact_from_seed(seed));
    }

    let state_fwd = reduce_authority(&journal_fwd).expect("forward reduction");
    let state_rev = reduce_authority(&journal_rev).expect("reverse reduction");
    let state_interleaved = reduce_authority(&journal_interleaved).expect("interleaved reduction");

    // All three must produce identical tree state
    assert_eq!(
        state_fwd.tree_state, state_rev.tree_state,
        "forward vs reverse must match"
    );
    assert_eq!(
        state_fwd.tree_state, state_interleaved.tree_state,
        "forward vs interleaved must match"
    );
    // All three must have the same facts
    assert_eq!(
        state_fwd.facts.len(),
        state_rev.facts.len(),
        "fact count must match"
    );
}

// ============================================================================
// Monotonic growth
//
// Journal is append-only: adding a fact must never remove existing facts.
// This is the foundational CRDT property that enables convergence — if
// adding facts could remove other facts, replicas would diverge.
// ============================================================================

/// Adding a fact to a journal must increase or maintain its size, never
/// decrease it. join(J, {f}) ⊇ J for any fact f.
#[test]
fn journal_add_fact_is_monotonic() {
    let ctx = ContextId::new_from_entropy([88u8; 32]);
    let namespace = JournalNamespace::Context(ctx);
    let mut journal = Journal::new(namespace);

    for i in 0u8..20 {
        let size_before = journal.size();
        let _ = journal.add_fact(fact_from_seed([i; 32]));
        assert!(
            journal.size() >= size_before,
            "adding fact {i} reduced journal size from {size_before} to {}",
            journal.size()
        );
    }
}

/// Journal join is monotonic: join(A, B) ⊇ A and join(A, B) ⊇ B.
/// Every fact in either input must appear in the output.
#[test]
fn journal_join_preserves_all_facts() {
    let ctx = ContextId::new_from_entropy([89u8; 32]);
    let namespace = JournalNamespace::Context(ctx);

    let mut journal_a = Journal::new(namespace.clone());
    let mut journal_b = Journal::new(namespace);

    for i in 0u8..5 {
        let _ = journal_a.add_fact(fact_from_seed([i; 32]));
    }
    for i in 3u8..8 {
        let _ = journal_b.add_fact(fact_from_seed([i; 32]));
    }

    let merged = journal_a.join(&journal_b);

    // merged must contain everything from A
    assert!(
        merged.size() >= journal_a.size(),
        "merged journal lost facts from A"
    );
    // merged must contain everything from B
    assert!(
        merged.size() >= journal_b.size(),
        "merged journal lost facts from B"
    );
    // merged should have union of both (some overlap at seeds 3,4)
    // A has 5 facts (0..5), B has 5 facts (3..8), union is 8 distinct
    assert_eq!(
        merged.size(),
        8,
        "merged journal should have union of facts"
    );
}

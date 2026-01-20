//! Property tests for journal semantics.

#![allow(clippy::expect_used, missing_docs)]

use aura_core::{
    identifiers::{AuthorityId, ContextId},
    time::{OrderTime, TimeStamp},
    Hash32, JoinSemilattice,
};
use aura_journal::{
    fact::{Fact, FactContent, Journal, JournalNamespace, ProtocolRelationalFact, RelationalFact},
    reduction::{reduce_context, RelationalBinding, RelationalState},
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

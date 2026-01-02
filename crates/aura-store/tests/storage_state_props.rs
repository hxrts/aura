use aura_core::{AuthorityId, ContentId, JoinSemilattice};
use aura_core::time::PhysicalTime;
use aura_store::{SearchIndexEntry, StorageState};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn entry_for_seed(seed: [u8; 32]) -> (ContentId, SearchIndexEntry, AuthorityId, PhysicalTime) {
    let content_id = ContentId::from_bytes(&seed);
    let authority = AuthorityId::new_from_entropy(seed);
    let timestamp = PhysicalTime {
        ts_ms: seed[0] as u64,
        uncertainty: None,
    };
    let terms: BTreeSet<String> = [format!("term-{}", seed[1])].into_iter().collect();
    let entry = SearchIndexEntry::new(content_id.to_string(), terms, Vec::new(), timestamp.clone());
    (content_id, entry, authority, timestamp)
}

fn state_from_seeds(seeds: &[[u8; 32]]) -> StorageState {
    let mut state = StorageState::new();
    for seed in seeds {
        let (content_id, entry, authority, timestamp) = entry_for_seed(*seed);
        state.add_content(content_id, entry, authority, timestamp);
    }
    state
}

proptest! {
    #[test]
    fn storage_state_join_commutative(
        seeds_a in proptest::collection::vec(any::<[u8; 32]>(), 0..12),
        seeds_b in proptest::collection::vec(any::<[u8; 32]>(), 0..12),
    ) {
        let state_a = state_from_seeds(&seeds_a);
        let state_b = state_from_seeds(&seeds_b);

        prop_assert_eq!(state_a.join(&state_b), state_b.join(&state_a));
    }

    #[test]
    fn storage_state_join_associative(
        seeds_a in proptest::collection::vec(any::<[u8; 32]>(), 0..8),
        seeds_b in proptest::collection::vec(any::<[u8; 32]>(), 0..8),
        seeds_c in proptest::collection::vec(any::<[u8; 32]>(), 0..8),
    ) {
        let state_a = state_from_seeds(&seeds_a);
        let state_b = state_from_seeds(&seeds_b);
        let state_c = state_from_seeds(&seeds_c);

        prop_assert_eq!(
            state_a.join(&state_b).join(&state_c),
            state_a.join(&state_b.join(&state_c))
        );
    }

    #[test]
    fn storage_state_join_idempotent(seeds in proptest::collection::vec(any::<[u8; 32]>(), 0..12)) {
        let state = state_from_seeds(&seeds);

        prop_assert_eq!(state.join(&state), state);
    }
}

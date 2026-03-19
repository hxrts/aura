//! Tier 1 holepunch NAT traversal tests.

#![allow(missing_docs)]

use std::time::Duration;

use aura_protocol::handlers::{
    CandidateGeneration, CandidateKind, ConnectionCandidate, NetworkGeneration, PeerConnectionActor,
};
use proptest::prelude::*;

#[derive(Debug, Clone)]
struct Candidate {
    kind: CandidateKind,
    recoverable: bool,
}

impl ConnectionCandidate for Candidate {
    fn kind(&self) -> CandidateKind {
        self.kind
    }

    fn is_recoverable(&self) -> bool {
        self.recoverable
    }
}

fn new_actor() -> PeerConnectionActor<Candidate> {
    PeerConnectionActor::new(3, Duration::from_millis(20), Duration::from_millis(400))
}

#[test]
fn path_selection_prefers_recoverable_relay_then_direct() {
    let mut actor = new_actor();
    let candidates = vec![
        Candidate {
            kind: CandidateKind::Direct,
            recoverable: true,
        },
        Candidate {
            kind: CandidateKind::Relay,
            recoverable: true,
        },
    ];
    actor.on_candidates_changed(CandidateGeneration(1), &candidates);

    let Some(selected) = actor.selected_path() else {
        panic!("selected path");
    };
    assert_eq!(selected.kind(), CandidateKind::Relay);

    let mut actor = new_actor();
    let candidates = vec![
        Candidate {
            kind: CandidateKind::Relay,
            recoverable: false,
        },
        Candidate {
            kind: CandidateKind::Direct,
            recoverable: true,
        },
    ];
    actor.on_candidates_changed(CandidateGeneration(2), &candidates);

    let Some(selected) = actor.selected_path() else {
        panic!("selected path");
    };
    assert_eq!(selected.kind(), CandidateKind::Direct);
}

#[test]
fn retry_budget_resets_on_network_generation_change() {
    let mut actor = new_actor();

    assert!(actor.next_retry_delay().is_some());
    assert!(actor.next_retry_delay().is_some());
    assert!(actor.next_retry_delay().is_some());
    assert!(actor.next_retry_delay().is_none());
    assert_eq!(actor.attempts_used(), actor.max_attempts());

    actor.on_network_changed(NetworkGeneration(1));
    assert_eq!(actor.attempts_used(), 0);
    assert!(actor.next_retry_delay().is_some());
}

proptest! {
    #[test]
    fn retry_attempts_are_monotone_unless_generation_changes(
        generation_changes in prop::collection::vec(any::<bool>(), 1..64)
    ) {
        let mut actor = new_actor();
        let mut generation = 0u64;
        let mut previous_attempts = actor.attempts_used();

        for changed in generation_changes {
            if changed {
                generation = generation.saturating_add(1);
                actor.on_network_changed(NetworkGeneration(generation));
                prop_assert_eq!(actor.attempts_used(), 0);
                previous_attempts = 0;
            }

            let _ = actor.next_retry_delay();
            let current = actor.attempts_used();
            prop_assert!(current >= previous_attempts);
            prop_assert!(current <= actor.max_attempts());
            previous_attempts = current;
        }
    }
}

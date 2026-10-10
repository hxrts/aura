//! Bounded proofs of the production consensus decision kernel and its storage adapter.
//!
//! Run from the default pinned shell: `nix develop --command just ci-kani`.
//! Bounds retain 2..=4 witnesses, 0..=3 existing proposals, three result
//! identities, and one incoming share. Identity representatives preserve every
//! equality/distinctness relation these decisions inspect; opaque share payloads
//! do not affect decisions. The additional refinement harness executes the real
//! heap-backed production wrappers and checks payload/commit custody separately.
#![cfg(kani)]

use super::super::{
    decision,
    state::{
        ConsensusPhase, ConsensusState, ConsensusThreshold, PathSelection, PureCommitFact,
        ShareData, ShareProposal,
    },
    transitions::{self, TransitionResult},
};
use crate::types::ConsensusId;
use aura_core::{AuthorityId, Hash32, OperationId};

#[derive(Clone, Copy, Debug)]
struct BoundedState {
    witness_count: u8,
    threshold: u8,
    equivocator_mask: u8,
    proposals: [(u8, u8); 4],
    len: usize,
    phase: ConsensusPhase,
    commit_result: Option<u8>,
    fallback_timer_active: bool,
}

impl BoundedState {
    fn proposals(&self) -> impl Clone + Iterator<Item = (u8, u8)> + '_ {
        self.proposals[..self.len].iter().copied()
    }
    fn equivocators(&self) -> impl Iterator<Item = u8> + '_ {
        (1..=self.witness_count).filter(|witness| self.equivocator_mask & (1 << witness) != 0)
    }
    fn has_proposal(&self, witness: u8) -> bool {
        decision::has_proposal(self.proposals(), witness)
    }
    fn threshold_met(&self) -> bool {
        decision::threshold_met(self.proposals(), usize::from(self.threshold))
    }
    fn invariants(&self) -> bool {
        decision::invariant_violation(
            self.phase,
            usize::from(self.threshold),
            self.proposals(),
            1..=self.witness_count,
            self.equivocators(),
            self.commit_result.is_some(),
        )
        .is_none()
            && (self.commit_result.is_none()
                || (self.threshold_met()
                    && decision::equivocators_excluded(self.proposals(), self.equivocators())))
    }
    fn apply_share(&self, proposal: (u8, u8)) -> Option<Self> {
        let update = decision::apply_share(
            self.phase,
            usize::from(self.threshold),
            self.proposals(),
            (1..=self.witness_count).contains(&proposal.0),
            self.equivocator_mask & (1 << proposal.0) != 0,
            proposal,
        )
        .ok()?;
        let mut next = *self;
        next.proposals[next.len] = proposal;
        next.len += 1;
        next.phase = update.phase;
        if update.commit_result.is_some() {
            next.commit_result = update.commit_result;
        }
        Some(next)
    }
    fn trigger_fallback(&self) -> Option<Self> {
        let mut next = *self;
        next.phase = decision::trigger_fallback(self.phase)?;
        next.fallback_timer_active = true;
        Some(next)
    }
    fn fail_consensus(&self) -> Option<Self> {
        let mut next = *self;
        next.phase = decision::fail_consensus(self.phase)?;
        Some(next)
    }
}

fn any_proposal() -> (u8, u8) {
    let witness: u8 = kani::any();
    let result: u8 = kani::any();
    kani::assume((1..=5).contains(&witness));
    kani::assume((1..=3).contains(&result));
    (witness, result)
}

fn any_state() -> BoundedState {
    let witness_count: u8 = kani::any();
    kani::assume((2..=4).contains(&witness_count));
    let threshold: u8 = kani::any();
    kani::assume(threshold >= 1 && threshold <= witness_count);
    let mut state = BoundedState {
        witness_count,
        threshold,
        equivocator_mask: 0,
        proposals: [(0, 0); 4],
        len: 0,
        phase: if kani::any() {
            ConsensusPhase::FastPathActive
        } else {
            ConsensusPhase::FallbackActive
        },
        commit_result: None,
        fallback_timer_active: false,
    };
    let count: usize = kani::any();
    kani::assume(count <= 3);
    for _ in 0..count {
        let proposal = any_proposal();
        if proposal.0 <= witness_count && !state.has_proposal(proposal.0) {
            state.proposals[state.len] = proposal;
            state.len += 1;
        }
    }
    let mask: u8 = kani::any();
    state.equivocator_mask = mask & ((1 << (witness_count + 1)) - 2);
    for index in 0..state.len {
        state.equivocator_mask &= !(1 << state.proposals[index].0);
    }
    state
}

fn reference_counts(state: &BoundedState) -> [usize; 3] {
    let mut counts = [0; 3];
    for (_, result) in state.proposals() {
        counts[usize::from(result - 1)] += 1;
    }
    counts
}

#[kani::proof]
#[kani::unwind(6)]
fn apply_share_preserves_invariants() {
    let state = any_state();
    kani::assume(state.invariants());
    if let Some(next) = state.apply_share(any_proposal()) {
        kani::assert(next.invariants(), "share preserves invariants");
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn trigger_fallback_preserves_invariants() {
    let state = any_state();
    kani::assume(state.invariants());
    if let Some(next) = state.trigger_fallback() {
        kani::assert(next.invariants(), "fallback preserves invariants");
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn fail_consensus_preserves_invariants() {
    let state = any_state();
    kani::assume(state.invariants());
    if let Some(next) = state.fail_consensus() {
        kani::assert(next.invariants(), "failure preserves invariants");
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn apply_share_monotonic_proposals() {
    let state = any_state();
    if let Some(next) = state.apply_share(any_proposal()) {
        kani::assert(next.len >= state.len, "proposals never shrink");
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn apply_share_monotonic_equivocators() {
    let state = any_state();
    // The production duplicate guard forbids admitting a second proposal from
    // a witness. Successful share admission never removes an equivocator.
    if let Some(next) = state.apply_share(any_proposal()) {
        kani::assert(
            next.equivocator_mask == state.equivocator_mask,
            "equivocators retained",
        );
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn apply_share_no_panic() {
    let state = any_state();
    kani::assume(state.invariants());
    let _ = state.apply_share(any_proposal());
}
#[kani::proof]
#[kani::unwind(6)]
fn trigger_fallback_no_panic() {
    let state = any_state();
    kani::assume(state.invariants());
    let _ = state.trigger_fallback();
}
#[kani::proof]
#[kani::unwind(6)]
fn fail_consensus_no_panic() {
    let state = any_state();
    kani::assume(state.invariants());
    let _ = state.fail_consensus();
}
#[kani::proof]
#[kani::unwind(6)]
fn committed_state_is_terminal() {
    let mut state = any_state();
    state.phase = ConsensusPhase::Committed;
    kani::assert(
        state.apply_share(any_proposal()).is_none(),
        "committed rejects shares",
    );
}
#[kani::proof]
#[kani::unwind(6)]
fn failed_state_is_terminal() {
    let mut state = any_state();
    state.phase = ConsensusPhase::Failed;
    kani::assert(
        state.apply_share(any_proposal()).is_none(),
        "failed rejects shares",
    );
}
#[kani::proof]
#[kani::unwind(6)]
fn phase_advances_forward() {
    let state = any_state();
    if let Some(next) = state.apply_share(any_proposal()) {
        kani::assert(
            next.phase == state.phase || next.phase == ConsensusPhase::Committed,
            "phase moves forward",
        );
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn commit_matches_threshold_result() {
    let state = any_state();
    kani::assume(state.invariants());
    kani::assume(!state.threshold_met());
    if let Some(next) = state.apply_share(any_proposal()) {
        if next.phase == ConsensusPhase::Committed {
            kani::assert(next.commit_result.is_some(), "commit has result");
            if let Some(result) = next.commit_result {
                kani::assert(
                    decision::count_result(next.proposals(), result) >= usize::from(next.threshold),
                    "commit has threshold",
                );
            }
        }
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn threshold_met_matches_reference() {
    let state = any_state();
    let counts = reference_counts(&state);
    kani::assert(
        state.threshold_met()
            == counts
                .iter()
                .any(|count| *count >= usize::from(state.threshold)),
        "threshold matches independent reference",
    );
    let winner = decision::majority_result(state.proposals(), usize::from(state.threshold));
    if let Some(result) = winner {
        let count = counts[usize::from(result - 1)];
        kani::assert(count >= usize::from(state.threshold), "winner qualifies");
        kani::assert(
            counts.iter().all(|other| *other <= count),
            "winner has maximal count",
        );
        // A unique maximal result must match exactly. Ties permit any maximal
        // qualifying result under the spec; production retains first encounter.
        for index in 0..3 {
            if counts[index] > 0
                && counts
                    .iter()
                    .filter(|other| **other == counts[index])
                    .count()
                    == 1
                && counts.iter().all(|other| *other <= counts[index])
            {
                kani::assert(
                    usize::from(result - 1) == index,
                    "unique winner matches reference exactly",
                );
            }
        }
    }
}
#[kani::proof]
#[kani::unwind(6)]
fn has_proposal_matches_reference() {
    let state = any_state();
    let witness: u8 = kani::any();
    kani::assume((1..=5).contains(&witness));
    let mut reference = false;
    for index in 0..state.len {
        reference |= state.proposals[index].0 == witness;
    }
    kani::assert(
        state.has_proposal(witness) == reference,
        "proposal membership matches reference",
    );
}

fn authority(index: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([index; 32])
}
fn result(index: u8) -> Hash32 {
    Hash32::new([index; 32])
}
fn share(proposal: (u8, u8)) -> ShareProposal {
    ShareProposal {
        witness: authority(proposal.0),
        result_id: result(proposal.1),
        share: ShareData {
            share_value: "share".into(),
            nonce_binding: "nonce".into(),
            data_binding: "binding".into(),
        },
    }
}
fn materialize(state: &BoundedState, identity: u8, operation: u8, prestate: u8) -> ConsensusState {
    let mut full = ConsensusState::new(
        ConsensusId(Hash32::new([identity; 32])),
        OperationId::new_from_entropy([operation; 32]),
        Hash32::new([prestate; 32]),
        ConsensusThreshold::new(u16::from(state.threshold)).expect("positive threshold"),
        (1..=state.witness_count).map(authority).collect(),
        authority(1),
        PathSelection::FastPath,
    );
    full.phase = state.phase;
    full.fallback_timer_active = state.fallback_timer_active;
    full.proposals = state.proposals().map(share).collect();
    full.equivocators = state.equivocators().map(authority).collect();
    full.commit_fact = state.commit_result.map(|winner| PureCommitFact {
        cid: full.cid,
        result_id: result(winner),
        prestate_hash: full.prestate_hash,
        signature: transitions::abstract_commit_signature(
            full.cid,
            result(winner),
            full.prestate_hash,
        ),
    });
    full
}
fn assert_storage_parity(
    full: &ConsensusState,
    bounded: &BoundedState,
    original: &ConsensusState,
    incoming: Option<&ShareProposal>,
) {
    kani::assert(full.phase == bounded.phase, "wrapper phase matches kernel");
    kani::assert(
        full.proposals.len() == bounded.len,
        "wrapper append matches kernel",
    );
    kani::assert(
        full.fallback_timer_active == bounded.fallback_timer_active,
        "wrapper timer matches kernel",
    );
    kani::assert(
        full.equivocators == original.equivocators,
        "wrapper preserves original equivocator set",
    );
    kani::assert(
        full.cid == original.cid
            && full.operation == original.operation
            && full.prestate_hash == original.prestate_hash,
        "wrapper preserves identity and prestate",
    );
    for index in 0..bounded.len {
        let expected = if index < original.proposals.len() {
            &original.proposals[index]
        } else {
            incoming.expect("appended proposal retains original input")
        };
        let actual = &full.proposals[index];
        kani::assert(
            actual.witness == expected.witness && actual.result_id == expected.result_id,
            "wrapper preserves proposal identity/order",
        );
        kani::assert(
            actual.share.share_value == expected.share.share_value
                && actual.share.nonce_binding == expected.share.nonce_binding
                && actual.share.data_binding == expected.share.data_binding,
            "wrapper preserves original payload",
        );
    }
    match (&full.commit_fact, bounded.commit_result) {
        (None, None) => {}
        (Some(commit), Some(expected)) => {
            kani::assert(
                commit.result_id == result(expected),
                "wrapper exact commit result matches kernel",
            );
            kani::assert(
                commit.cid == full.cid && commit.prestate_hash == full.prestate_hash,
                "commit retains original identity",
            );
            kani::assert(
                !commit.signature.is_empty(),
                "real signature producer emits nonempty binding",
            );
        }
        _ => kani::assert(false, "wrapper commit presence matches kernel"),
    }
}
/// This extra proof deliberately executes the real public wrappers, including
/// allocation, rejection formatting and the actual abstract signature producer.
/// It cannot be replaced with a stub or waived when kernel-only proofs pass.
#[kani::proof]
#[kani::unwind(130)]
fn production_wrappers_refine_bounded_decisions() {
    let mut state = any_state();
    let phase: u8 = kani::any();
    kani::assume(phase < 5);
    state.phase = match phase {
        0 => ConsensusPhase::Pending,
        1 => ConsensusPhase::FastPathActive,
        2 => ConsensusPhase::FallbackActive,
        3 => ConsensusPhase::Committed,
        _ => ConsensusPhase::Failed,
    };
    let proposal = any_proposal();
    if state.phase == ConsensusPhase::Committed {
        state.commit_result =
            decision::majority_result(state.proposals(), usize::from(state.threshold));
        kani::assume(state.commit_result.is_some());
    }
    state.fallback_timer_active = kani::any();
    // Boolean choices preserve the exact independent two-value domains while
    // exposing concrete representatives to constant propagation.
    let (identity, operation, prestate) =
        super::refinement_domain::bindings(kani::any(), kani::any(), kani::any());
    let full = materialize(&state, identity, operation, prestate);
    kani::assert(
        super::super::validation::check_all_invariants(&full),
        "refinement starts from actual well-formed stored state",
    );
    let retained_commit = full.commit_fact.clone();
    let mut incoming = share(proposal);
    incoming.share.share_value = if kani::any() { "a" } else { "b" }.into();
    incoming.share.nonce_binding = if kani::any() { "c" } else { "d" }.into();
    incoming.share.data_binding = if kani::any() { "e" } else { "f" }.into();
    // Universal nondeterminism covers every transition without executing
    // three independent heap-backed wrappers on each symbolic path.
    let choice: u8 = kani::any();
    kani::assume(choice < 3);
    match super::refinement_domain::transition(choice).expect("validated transition domain") {
        super::refinement_domain::Transition::ApplyShare => {
            match (
                transitions::apply_share(&full, incoming.clone()),
                state.apply_share(proposal),
            ) {
                (TransitionResult::Ok(actual), Some(expected)) => {
                    assert_storage_parity(&actual, &expected, &full, Some(&incoming))
                }
                (TransitionResult::NotEnabled(_), None) => {}
                _ => kani::assert(false, "wrapper admission matches kernel"),
            }
        }
        super::refinement_domain::Transition::TriggerFallback => {
            match (
                transitions::trigger_fallback(&full),
                state.trigger_fallback(),
            ) {
                (TransitionResult::Ok(actual), Some(expected)) => {
                    assert_storage_parity(&actual, &expected, &full, None)
                }
                (TransitionResult::NotEnabled(_), None) => {}
                _ => kani::assert(false, "wrapper fallback matches kernel"),
            }
        }
        super::refinement_domain::Transition::FailConsensus => {
            match (transitions::fail_consensus(&full), state.fail_consensus()) {
                (TransitionResult::Ok(actual), Some(expected)) => {
                    assert_storage_parity(&actual, &expected, &full, None)
                }
                (TransitionResult::NotEnabled(_), None) => {}
                _ => kani::assert(false, "wrapper failure matches kernel"),
            }
        }
    }
    match (&full.commit_fact, &retained_commit) {
        (Some(actual), Some(original)) => kani::assert(
            actual.cid == original.cid
                && actual.result_id == original.result_id
                && actual.prestate_hash == original.prestate_hash
                && actual.signature == original.signature,
            "terminal rejections preserve the original stored commit payload",
        ),
        (None, None) => {}
        _ => kani::assert(false, "original stored commit presence retained"),
    }
}

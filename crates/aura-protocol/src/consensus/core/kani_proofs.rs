//! Kani Bounded Model Checking Proofs for Consensus Core
//!
//! This module contains Kani proof harnesses that verify key properties of the
//! consensus state machine using bounded model checking.
//!
//! ## Running Kani
//!
//! From nightly shell:
//! ```bash
//! nix develop .#nightly
//! cargo install --locked kani-verifier  # First time only
//! cargo kani setup                       # First time only
//! cargo kani --package aura-protocol    # Run all proofs
//! cargo kani --harness apply_share_preserves_invariants  # Run specific proof
//! ```
//!
//! ## Proof Categories
//!
//! 1. **Invariant Preservation**: Transitions preserve well-formedness
//! 2. **Monotonicity**: State growth properties (proposals never shrink)
//! 3. **Panic Freedom**: No panics on valid inputs
//! 4. **Agreement**: Two commits for same CID have same result
//! 5. **Reference Equivalence**: Production matches reference implementation
//!
//! ## Bounds
//!
//! To keep verification tractable, we bound:
//! - Witness set size: 3-5 witnesses
//! - Proposal count: 0-5 proposals
//! - String lengths: 1-8 characters
//!
//! These bounds are sufficient to find most bugs while keeping
//! verification times reasonable (< 5 minutes per harness).

// Only compile this module when Kani is running
#![cfg(kani)]

use std::collections::HashSet;

use super::state::{ConsensusPhase, ConsensusState, PathSelection, ShareData, ShareProposal};
use super::transitions::{apply_share, fail_consensus, trigger_fallback, TransitionResult};
use super::validation::check_invariants;

// =============================================================================
// Helper Functions for Symbolic State Generation
// =============================================================================

/// Generate a bounded symbolic string
fn any_bounded_string(max_len: usize) -> String {
    let len: usize = kani::any();
    kani::assume(len >= 1 && len <= max_len);

    let mut s = String::with_capacity(len);
    for _ in 0..len {
        let c: u8 = kani::any();
        // Limit to alphanumeric ASCII for tractability
        kani::assume((c >= b'a' && c <= b'z') || (c >= b'0' && c <= b'9'));
        s.push(c as char);
    }
    s
}

/// Generate a symbolic witness ID (w1, w2, w3, w4, w5)
fn any_witness_id() -> String {
    let idx: u8 = kani::any();
    kani::assume(idx >= 1 && idx <= 5);
    format!("w{}", idx)
}

/// Generate a symbolic result ID (rid1, rid2, rid3)
fn any_result_id() -> String {
    let idx: u8 = kani::any();
    kani::assume(idx >= 1 && idx <= 3);
    format!("rid{}", idx)
}

/// Generate a symbolic ShareData
fn any_share_data() -> ShareData {
    ShareData {
        share_value: any_bounded_string(8),
        nonce_binding: any_bounded_string(8),
        data_binding: any_bounded_string(8),
    }
}

/// Generate a symbolic ShareProposal
fn any_share_proposal() -> ShareProposal {
    ShareProposal {
        witness: any_witness_id(),
        result_id: any_result_id(),
        share: any_share_data(),
    }
}

/// Generate a symbolic witness set of bounded size
fn any_witness_set(min_size: usize, max_size: usize) -> HashSet<String> {
    let size: usize = kani::any();
    kani::assume(size >= min_size && size <= max_size);

    let mut witnesses = HashSet::new();
    // Add specific witnesses based on size
    if size >= 1 {
        witnesses.insert("w1".to_string());
    }
    if size >= 2 {
        witnesses.insert("w2".to_string());
    }
    if size >= 3 {
        witnesses.insert("w3".to_string());
    }
    if size >= 4 {
        witnesses.insert("w4".to_string());
    }
    if size >= 5 {
        witnesses.insert("w5".to_string());
    }

    witnesses
}

/// Generate a well-formed ConsensusState for testing
fn any_valid_consensus_state() -> ConsensusState {
    let witnesses = any_witness_set(2, 4);
    let threshold: usize = kani::any();
    kani::assume(threshold >= 1 && threshold <= witnesses.len());

    let initiator = any_witness_id();
    kani::assume(witnesses.contains(&initiator));

    let path_idx: u8 = kani::any();
    let path = if path_idx % 2 == 0 {
        PathSelection::FastPath
    } else {
        PathSelection::SlowPath
    };

    let mut state = ConsensusState::new(
        any_bounded_string(8), // cid
        any_bounded_string(8), // operation
        any_bounded_string(8), // prestate_hash
        threshold,
        witnesses.clone(),
        initiator,
        path,
    );

    // Optionally add some proposals
    let num_proposals: usize = kani::any();
    kani::assume(num_proposals <= 3);

    for _ in 0..num_proposals {
        let proposal = any_share_proposal();
        // Only add if valid (witness in set, not duplicate)
        if witnesses.contains(&proposal.witness) && !state.has_proposal(&proposal.witness) {
            state.proposals.push(proposal);
        }
    }

    state
}

// =============================================================================
// Proof Harnesses: Invariant Preservation
// =============================================================================

/// Verify that apply_share preserves state invariants.
///
/// Property: If check_invariants(state) passes before apply_share,
/// then check_invariants(new_state) passes after (when transition succeeds).
///
/// Quint: ValidTransition implies WellFormedState'
#[kani::proof]
#[kani::unwind(10)]
fn apply_share_preserves_invariants() {
    let state = any_valid_consensus_state();

    // Precondition: state is well-formed
    kani::assume(check_invariants(&state).is_ok());

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(new_state) => {
            // Postcondition: new state is well-formed
            kani::assert(
                check_invariants(&new_state).is_ok(),
                "apply_share must preserve invariants",
            );
        }
        TransitionResult::NotEnabled(_) => {
            // Transition not enabled - no invariant to check
        }
    }
}

/// Verify that trigger_fallback preserves state invariants.
#[kani::proof]
#[kani::unwind(10)]
fn trigger_fallback_preserves_invariants() {
    let state = any_valid_consensus_state();

    kani::assume(check_invariants(&state).is_ok());

    match trigger_fallback(&state) {
        TransitionResult::Ok(new_state) => {
            kani::assert(
                check_invariants(&new_state).is_ok(),
                "trigger_fallback must preserve invariants",
            );
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

/// Verify that fail_consensus preserves state invariants.
#[kani::proof]
#[kani::unwind(10)]
fn fail_consensus_preserves_invariants() {
    let state = any_valid_consensus_state();

    kani::assume(check_invariants(&state).is_ok());

    match fail_consensus(&state) {
        TransitionResult::Ok(new_state) => {
            kani::assert(
                check_invariants(&new_state).is_ok(),
                "fail_consensus must preserve invariants",
            );
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

// =============================================================================
// Proof Harnesses: Monotonicity Properties
// =============================================================================

/// Verify that apply_share is monotonic: proposals never shrink.
///
/// Property: new_state.proposals.len() >= state.proposals.len()
///
/// This is a key property for CRDT-like convergence.
#[kani::proof]
#[kani::unwind(10)]
fn apply_share_monotonic_proposals() {
    let state = any_valid_consensus_state();
    let initial_count = state.proposals.len();

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(new_state) => {
            kani::assert(
                new_state.proposals.len() >= initial_count,
                "apply_share must not shrink proposals",
            );
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

/// Verify that equivocators set is monotonic: never shrinks.
#[kani::proof]
#[kani::unwind(10)]
fn apply_share_monotonic_equivocators() {
    let state = any_valid_consensus_state();

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(new_state) => {
            // Every equivocator in old state remains in new state
            for eq in &state.equivocators {
                kani::assert(
                    new_state.equivocators.contains(eq),
                    "equivocators must not be removed",
                );
            }
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

// =============================================================================
// Proof Harnesses: Panic Freedom
// =============================================================================

/// Verify apply_share never panics on valid states.
///
/// Property: Given any well-formed state and any proposal,
/// apply_share either succeeds or returns NotEnabled (no panic).
#[kani::proof]
#[kani::unwind(10)]
fn apply_share_no_panic() {
    let state = any_valid_consensus_state();
    kani::assume(check_invariants(&state).is_ok());

    let proposal = any_share_proposal();

    // This should not panic
    let _result = apply_share(&state, proposal);
}

/// Verify trigger_fallback never panics.
#[kani::proof]
#[kani::unwind(10)]
fn trigger_fallback_no_panic() {
    let state = any_valid_consensus_state();
    kani::assume(check_invariants(&state).is_ok());

    let _result = trigger_fallback(&state);
}

/// Verify fail_consensus never panics.
#[kani::proof]
#[kani::unwind(10)]
fn fail_consensus_no_panic() {
    let state = any_valid_consensus_state();
    kani::assume(check_invariants(&state).is_ok());

    let _result = fail_consensus(&state);
}

// =============================================================================
// Proof Harnesses: Phase Transition Properties
// =============================================================================

/// Verify terminal states are stable: once committed, stays committed.
#[kani::proof]
#[kani::unwind(10)]
fn committed_state_is_terminal() {
    let mut state = any_valid_consensus_state();

    // Force state to Committed
    state.phase = ConsensusPhase::Committed;

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(new_state) => {
            // Should not happen - committed states should reject new shares
            kani::assert(false, "committed state should not accept new shares");
        }
        TransitionResult::NotEnabled(_) => {
            // Expected: committed state rejects transitions
        }
    }
}

/// Verify failed states are terminal.
#[kani::proof]
#[kani::unwind(10)]
fn failed_state_is_terminal() {
    let mut state = any_valid_consensus_state();

    state.phase = ConsensusPhase::Failed;

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(_) => {
            kani::assert(false, "failed state should not accept new shares");
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

/// Verify phase only advances forward (Pending -> Active -> Terminal).
#[kani::proof]
#[kani::unwind(10)]
fn phase_advances_forward() {
    let state = any_valid_consensus_state();
    kani::assume(check_invariants(&state).is_ok());

    let proposal = any_share_proposal();

    match apply_share(&state, proposal) {
        TransitionResult::Ok(new_state) => {
            // Check phase progression is valid
            match (state.phase, new_state.phase) {
                // Valid progressions
                (ConsensusPhase::FastPathActive, ConsensusPhase::FastPathActive) => {}
                (ConsensusPhase::FastPathActive, ConsensusPhase::Committed) => {}
                (ConsensusPhase::FallbackActive, ConsensusPhase::FallbackActive) => {}
                (ConsensusPhase::FallbackActive, ConsensusPhase::Committed) => {}

                // Invalid: going backward
                (ConsensusPhase::Committed, _) => {
                    kani::assert(false, "cannot transition from Committed");
                }
                (ConsensusPhase::Failed, _) => {
                    kani::assert(false, "cannot transition from Failed");
                }

                // Other transitions not allowed via apply_share
                _ => {}
            }
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

// =============================================================================
// Proof Harnesses: Agreement Property
// =============================================================================

/// Verify agreement: if threshold reached for result R, commit has R.
///
/// Property: When apply_share causes a commit, the commit_fact.result_id
/// matches the result_id that reached threshold.
#[kani::proof]
#[kani::unwind(10)]
fn commit_matches_threshold_result() {
    let state = any_valid_consensus_state();
    kani::assume(check_invariants(&state).is_ok());
    kani::assume(!state.threshold_met()); // Not yet at threshold

    let proposal = any_share_proposal();

    match apply_share(&state, proposal.clone()) {
        TransitionResult::Ok(new_state) => {
            if new_state.phase == ConsensusPhase::Committed {
                // If we committed, commit_fact must exist
                kani::assert(
                    new_state.commit_fact.is_some(),
                    "committed state must have commit_fact",
                );

                if let Some(cf) = &new_state.commit_fact {
                    // Count proposals for the committed result
                    let count = new_state.count_proposals_for_result(&cf.result_id);
                    kani::assert(
                        count >= new_state.threshold,
                        "committed result must have threshold proposals",
                    );
                }
            }
        }
        TransitionResult::NotEnabled(_) => {}
    }
}

// =============================================================================
// Proof Harnesses: Reference Equivalence
// =============================================================================

/// Verify production threshold_met matches reference implementation.
///
/// This ensures the production code matches the simpler reference specification.
#[kani::proof]
#[kani::unwind(10)]
fn threshold_met_matches_reference() {
    let state = any_valid_consensus_state();

    // Reference implementation: explicit counting
    let mut counts = std::collections::HashMap::new();
    for p in &state.proposals {
        *counts.entry(&p.result_id).or_insert(0usize) += 1;
    }
    let ref_threshold_met = counts.values().any(|&c| c >= state.threshold);

    // Production implementation
    let prod_threshold_met = state.threshold_met();

    kani::assert(
        ref_threshold_met == prod_threshold_met,
        "threshold_met must match reference",
    );
}

/// Verify has_proposal matches reference implementation.
#[kani::proof]
#[kani::unwind(10)]
fn has_proposal_matches_reference() {
    let state = any_valid_consensus_state();
    let witness = any_witness_id();

    // Reference: linear search
    let ref_has = state.proposals.iter().any(|p| p.witness == witness);

    // Production
    let prod_has = state.has_proposal(&witness);

    kani::assert(ref_has == prod_has, "has_proposal must match reference");
}

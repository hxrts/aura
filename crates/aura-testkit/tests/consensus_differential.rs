//! Differential Tests: Production vs Reference Implementation
//!
//! Uses proptest to generate arbitrary inputs and verify that production
//! code behaves identically to reference implementations.
#![allow(clippy::expect_used)] // Test helper functions use expect for clarity
#![allow(clippy::clone_on_copy)] // Proptest code uses clone for clarity
//!
//! ## Strategy
//!
//! For each function pair (production, reference), we:
//! 1. Generate arbitrary valid inputs
//! 2. Run both implementations
//! 3. Assert outputs match
//!
//! This catches subtle bugs that ITF traces might miss due to limited coverage.

use aura_consensus::core::{
    state::{
        ConsensusPhase, ConsensusState, ConsensusThreshold, PathSelection, ShareData, ShareProposal,
    },
    transitions::{apply_share, fail_consensus, trigger_fallback, TransitionResult},
    validation::{check_invariants, is_equivocator, shares_consistent},
};
use aura_consensus::types::ConsensusId;
use aura_core::{AuthorityId, Hash32, OperationId};
use aura_testkit::consensus::{
    aggregate_shares_ref, apply_share_ref, check_invariants_ref, check_threshold_ref,
    detect_equivocators_ref, fail_consensus_ref, shares_consistent_ref, trigger_fallback_ref,
    TransitionResultRef, Vote,
};
use proptest::prelude::*;
use std::collections::BTreeSet;

fn threshold(value: u16) -> ConsensusThreshold {
    ConsensusThreshold::new(value).expect("threshold")
}

fn threshold_from_usize(value: usize) -> ConsensusThreshold {
    ConsensusThreshold::new(u16::try_from(value).expect("threshold fits in u16"))
        .expect("threshold")
}

fn authority_for_index(idx: usize) -> AuthorityId {
    AuthorityId::new_from_entropy([idx as u8; 32])
}

fn hash_for_label(label: &str) -> Hash32 {
    Hash32::from_bytes(label.as_bytes())
}

fn consensus_id_for_label(label: &str) -> ConsensusId {
    ConsensusId(Hash32::from_bytes(label.as_bytes()))
}

fn operation_for_index(idx: usize) -> OperationId {
    OperationId::new_from_entropy([idx as u8; 32])
}

// ============================================================================
// PROPTEST STRATEGIES
// ============================================================================
//
// Note: Some strategies are defined for future expansion of differential tests.
// The #[allow(dead_code)] attributes suppress warnings for currently unused strategies.

// ============================================================================
// DIFFERENTIAL TESTS: THRESHOLD CHECKING
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Differential: check_threshold_ref vs check_threshold_met
    ///
    /// Both should agree on whether threshold is met for same proposal set.
    #[test]
    fn diff_threshold_checking(
        n_witnesses in 3usize..=7,
        n_proposals in 0usize..=7,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        // Create proposals for first n_proposals witnesses, all same result
        let proposals: Vec<ShareProposal> = (0..n_proposals.min(n_witnesses))
            .map(|i| ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            })
            .collect();

        // Reference: direct proposal count check
        let ref_result = check_threshold_ref(&proposals, threshold);

        // Production: via ConsensusState.threshold_met()
        // Note: check_threshold_met() has different semantics (checks commit fact)
        // The correct equivalent to check_threshold_ref is state.threshold_met()
        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );
        state.proposals = proposals;
        let prod_result = state.threshold_met();

        // Both should agree
        prop_assert_eq!(
            ref_result, prod_result,
            "Threshold check mismatch: ref={}, prod={}",
            ref_result, prod_result
        );
    }

    /// Differential: shares_consistent_ref vs shares_consistent
    #[test]
    fn diff_shares_consistency(
        n_proposals in 1usize..=5,
        mix_results in prop::bool::ANY,
    ) {
        let result_ids = if mix_results {
            vec!["rid1", "rid2"]
        } else {
            vec!["rid1"]
        };

        let proposals: Vec<ShareProposal> = (0..n_proposals)
            .map(|i| {
                let rid = result_ids[i % result_ids.len()];
                ShareProposal {
                    witness: authority_for_index(i),
                    result_id: hash_for_label(rid),
                    share: ShareData {
                        share_value: format!("share_{i}"),
                        nonce_binding: format!("nonce_{i}"),
                        data_binding: format!("cns1:{rid}:hash"),
                    },
                }
            })
            .collect();

        // Reference: checks all proposals have same result_id and binding
        let ref_result = shares_consistent_ref(&proposals);

        // Production: checks specific result_id proposals have valid shares
        // Note: different semantics - production filters by result_id
        let prod_result = shares_consistent(&proposals, &hash_for_label("rid1"), &hash_for_label("hash"));

        // For single-result case, both should agree
        if !mix_results {
            prop_assert_eq!(
                ref_result, prod_result,
                "Consistency check mismatch for single result: ref={}, prod={}",
                ref_result, prod_result
            );
        }
        // For mixed results, reference returns false, production still true for rid1 subset
    }

    /// Differential: detect_equivocators_ref vs is_equivocator
    #[test]
    fn diff_equivocator_detection(
        n_votes in 2usize..=10,
        equivocator_index in 0usize..10,
    ) {
        // Create votes, possibly with one equivocator
        let mut votes: Vec<Vote> = (0..n_votes)
            .map(|i| Vote {
                witness: authority_for_index(i % 5),
                result_id: hash_for_label("rid1"),
                prestate_hash: hash_for_label("hash"),
            })
            .collect();

        // Make one witness equivocate (if index is valid)
        if equivocator_index < votes.len() {
            let target_witness = votes[equivocator_index].witness.clone();
            // Add conflicting vote
            votes.push(Vote {
                witness: target_witness,
                result_id: hash_for_label("rid2"), // Different result!
                prestate_hash: hash_for_label("hash"),
            });

            // Reference: detect all equivocators
            let ref_equivocators = detect_equivocators_ref(&votes);

            // Convert votes to proposals for production check
            let proposals: Vec<ShareProposal> = votes
                .iter()
                .map(|v| ShareProposal {
                    witness: v.witness,
                    result_id: v.result_id,
                    share: ShareData {
                        share_value: "share".to_string(),
                        nonce_binding: "nonce".to_string(),
                        data_binding: format!(
                            "cns1:{rid}:{hash}",
                            rid = v.result_id.to_hex(),
                            hash = v.prestate_hash.to_hex()
                        ),
                    },
                })
                .collect();

            // Production: check specific witness
            let prod_is_equivocator = is_equivocator(&proposals, &target_witness);

            // If reference detected them, production should too
            if ref_equivocators.contains(&target_witness) {
                prop_assert!(
                    prod_is_equivocator,
                    "Reference detected equivocator {} but production didn't",
                    target_witness
                );
            }
        }
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: AGGREGATION
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// Differential: aggregate_shares_ref produces valid output
    #[test]
    fn diff_aggregation_validity(
        n_proposals in 2usize..=5,
        threshold in 1usize..=3,
    ) {
        let threshold = threshold.min(n_proposals);

        // All proposals for same result (consistent)
        let proposals: Vec<ShareProposal> = (0..n_proposals)
            .map(|i| ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            })
            .collect();

        let result = aggregate_shares_ref(&proposals, threshold);

        // Should succeed if threshold met and consistent
        if n_proposals >= threshold {
            prop_assert!(
                result.is_some(),
                "Aggregation should succeed with {} proposals >= {} threshold",
                n_proposals, threshold
            );

            let sig = result.unwrap();
            prop_assert_eq!(sig.signer_set.len(), n_proposals);
            prop_assert_eq!(sig.bound_rid, hash_for_label("rid1"));
        } else {
            prop_assert!(
                result.is_none(),
                "Aggregation should fail with {} proposals < {} threshold",
                n_proposals, threshold
            );
        }
    }

    /// Differential: aggregation fails on inconsistent proposals
    #[test]
    fn diff_aggregation_rejects_inconsistent(
        n_proposals in 2usize..=5,
    ) {
        // Create inconsistent proposals (different result_ids)
        let proposals: Vec<ShareProposal> = (0..n_proposals)
            .map(|i| ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label(&format!("rid{i}")), // All different!
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: format!("cns1:rid{i}:hash"),
                },
            })
            .collect();

        let result = aggregate_shares_ref(&proposals, 2);

        // Should fail due to inconsistency (unless n_proposals == 1)
        if n_proposals > 1 {
            prop_assert!(
                result.is_none(),
                "Aggregation should reject inconsistent proposals"
            );
        }
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: STATE TRANSITIONS
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Invariant: apply_share preserves state invariants
    #[test]
    fn invariant_apply_share_preserves_invariants(
        n_witnesses in 3usize..=5,
        n_existing_proposals in 0usize..=2,
        new_witness_idx in 0usize..=4,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        // Add some existing proposals
        for i in 0..n_existing_proposals.min(n_witnesses) {
            state.proposals.push(ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            });
        }

        // Initial state should be valid
        prop_assert!(check_invariants(&state).is_ok(), "Initial state invalid");

        // Apply new share
        let new_witness = authority_for_index(new_witness_idx % n_witnesses);
        let proposal = ShareProposal {
            witness: new_witness,
            result_id: hash_for_label("rid1"),
            share: ShareData {
                share_value: "new_share".to_string(),
                nonce_binding: "new_nonce".to_string(),
                data_binding: "cns1:rid1:hash".to_string(),
            },
        };

        if let TransitionResult::Ok(new_state) = apply_share(&state, proposal) {
            // New state should also be valid
            prop_assert!(
                check_invariants(&new_state).is_ok(),
                "State after apply_share is invalid: {:?}",
                check_invariants(&new_state)
            );
        }
    }

    /// Invariant: trigger_fallback preserves invariants
    #[test]
    fn invariant_trigger_fallback_preserves_invariants(
        n_witnesses in 3usize..=5,
        n_proposals in 0usize..=2,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        // Add some proposals (not enough to reach threshold)
        for i in 0..n_proposals.min(threshold.saturating_sub(1)) {
            state.proposals.push(ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            });
        }

        prop_assert!(check_invariants(&state).is_ok(), "Initial state invalid");

        if let TransitionResult::Ok(new_state) = trigger_fallback(&state) {
            prop_assert!(
                check_invariants(&new_state).is_ok(),
                "State after trigger_fallback is invalid"
            );
            prop_assert_eq!(
                new_state.phase,
                ConsensusPhase::FallbackActive,
                "Phase should be FallbackActive"
            );
        }
    }

    /// Invariant: proposals are monotonic (never shrink)
    #[test]
    fn invariant_proposals_monotonic(
        n_witnesses in 3usize..=5,
        proposal_sequence in prop::collection::vec(0usize..5, 1..=5),
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        let mut prev_count = 0;

        for (seq_idx, witness_idx) in proposal_sequence.iter().enumerate() {
            let witness = authority_for_index(witness_idx % n_witnesses);

            if !state.is_active() {
                break; // Consensus committed or failed
            }

            let proposal = ShareProposal {
                witness,
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{seq_idx}"),
                    nonce_binding: format!("nonce_{seq_idx}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            };

            if let TransitionResult::Ok(new_state) = apply_share(&state, proposal) {
                // Proposals should never shrink
                prop_assert!(
                    new_state.proposals.len() >= prev_count,
                    "Proposals shrunk: {} -> {}",
                    prev_count,
                    new_state.proposals.len()
                );
                prev_count = new_state.proposals.len();
                state = new_state;
            }
        }
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: TRANSITIONS (Production vs Reference)
// ============================================================================

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Differential: apply_share vs apply_share_ref
    ///
    /// Both should agree on:
    /// - Whether transition is enabled
    /// - Resulting phase
    /// - Resulting proposal count
    /// - Commit fact presence
    #[test]
    fn diff_apply_share_equivalence(
        n_witnesses in 3usize..=5,
        n_existing in 0usize..=2,
        new_witness_idx in 0usize..=4,
        same_result in prop::bool::ANY,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        // Add some existing proposals
        for i in 0..n_existing.min(n_witnesses) {
            state.proposals.push(ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            });
        }

        // Create new proposal
        let new_witness = authority_for_index(new_witness_idx % n_witnesses);
        let result_id = if same_result { "rid1" } else { "rid2" };
        let proposal = ShareProposal {
            witness: new_witness,
            result_id: hash_for_label(result_id),
            share: ShareData {
                share_value: "new_share".to_string(),
                nonce_binding: "new_nonce".to_string(),
                data_binding: format!("cns1:{result_id}:hash"),
            },
        };

        // Run production
        let prod_result = apply_share(&state, proposal.clone());

        // Run reference
        let ref_result = apply_share_ref(&state, proposal);

        // Compare: both should agree on enabled/disabled
        prop_assert_eq!(
            prod_result.is_ok(),
            ref_result.is_ok(),
            "Enablement mismatch: prod={}, ref={}",
            prod_result.is_ok(),
            ref_result.is_ok()
        );

        // If both succeeded, compare resulting states
        if let (TransitionResult::Ok(prod_state), TransitionResultRef::Ok(ref_state)) =
            (prod_result, ref_result)
        {
            prop_assert_eq!(
                prod_state.phase,
                ref_state.phase,
                "Phase mismatch"
            );
            prop_assert_eq!(
                prod_state.proposals.len(),
                ref_state.proposals.len(),
                "Proposal count mismatch"
            );
            prop_assert_eq!(
                prod_state.commit_fact.is_some(),
                ref_state.commit_fact.is_some(),
                "Commit fact presence mismatch"
            );
            prop_assert_eq!(
                prod_state.equivocators.len(),
                ref_state.equivocators.len(),
                "Equivocator count mismatch"
            );
        }
    }

    /// Differential: trigger_fallback vs trigger_fallback_ref
    #[test]
    fn diff_trigger_fallback_equivalence(
        n_witnesses in 3usize..=5,
        start_in_fallback in prop::bool::ANY,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let path = if start_in_fallback {
            PathSelection::SlowPath
        } else {
            PathSelection::FastPath
        };

        let state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            path,
        );

        // Run production
        let prod_result = trigger_fallback(&state);

        // Run reference
        let ref_result = trigger_fallback_ref(&state);

        // Compare enablement
        prop_assert_eq!(
            prod_result.is_ok(),
            ref_result.is_ok(),
            "Trigger fallback enablement mismatch"
        );

        // If both succeeded, compare states
        if let (TransitionResult::Ok(prod_state), TransitionResultRef::Ok(ref_state)) =
            (prod_result, ref_result)
        {
            prop_assert_eq!(prod_state.phase, ref_state.phase);
            prop_assert_eq!(
                prod_state.fallback_timer_active,
                ref_state.fallback_timer_active
            );
        }
    }

    /// Differential: fail_consensus vs fail_consensus_ref
    #[test]
    fn diff_fail_consensus_equivalence(
        n_witnesses in 3usize..=5,
        phase_idx in 0usize..=4,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();
        let threshold = n_witnesses.div_ceil(2);

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        // Set phase based on index
        state.phase = match phase_idx % 5 {
            0 => ConsensusPhase::Pending,
            1 => ConsensusPhase::FastPathActive,
            2 => ConsensusPhase::FallbackActive,
            3 => ConsensusPhase::Committed,
            _ => ConsensusPhase::Failed,
        };

        // Run production
        let prod_result = fail_consensus(&state);

        // Run reference
        let ref_result = fail_consensus_ref(&state);

        // Compare enablement
        prop_assert_eq!(
            prod_result.is_ok(),
            ref_result.is_ok(),
            "Fail consensus enablement mismatch for phase {:?}",
            state.phase
        );

        // If both succeeded, compare states
        if let (TransitionResult::Ok(prod_state), TransitionResultRef::Ok(ref_state)) =
            (prod_result, ref_result)
        {
            prop_assert_eq!(prod_state.phase, ref_state.phase);
        }
    }

    /// Differential: check_invariants vs check_invariants_ref
    #[test]
    fn diff_check_invariants_equivalence(
        n_witnesses in 1usize..=5,
        threshold in 1usize..=5,
        n_proposals in 0usize..=3,
        has_invalid_proposal in prop::bool::ANY,
    ) {
        let witnesses: BTreeSet<AuthorityId> = (0..n_witnesses)
            .map(authority_for_index)
            .collect();

        let mut state = ConsensusState::new(
            consensus_id_for_label("cns1"),
            operation_for_index(1),
            hash_for_label("hash"),
            threshold_from_usize(threshold.min(n_witnesses)),
            witnesses,
            authority_for_index(0),
            PathSelection::FastPath,
        );

        // Override threshold to test threshold invariant
        state.threshold = threshold_from_usize(threshold);

        // Add some proposals
        for i in 0..n_proposals.min(n_witnesses) {
            state.proposals.push(ShareProposal {
                witness: authority_for_index(i),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: format!("share_{i}"),
                    nonce_binding: format!("nonce_{i}"),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            });
        }

        // Optionally add invalid proposal (from non-witness)
        if has_invalid_proposal {
            state.proposals.push(ShareProposal {
                witness: authority_for_index(99),
                result_id: hash_for_label("rid1"),
                share: ShareData {
                    share_value: "share".to_string(),
                    nonce_binding: "nonce".to_string(),
                    data_binding: "cns1:rid1:hash".to_string(),
                },
            });
        }

        // Run production
        let prod_result = check_invariants(&state);

        // Run reference
        let ref_result = check_invariants_ref(&state);

        // Compare: both should agree on validity
        let prod_valid = prod_result.is_ok();
        let ref_valid = ref_result.is_none();

        prop_assert_eq!(
            prod_valid,
            ref_valid,
            "Invariant check mismatch: prod={}, ref={}",
            prod_valid,
            ref_valid
        );
    }
}

// ============================================================================
// DETERMINISTIC TESTS
// ============================================================================

#[test]
fn test_threshold_equivalence_exact() {
    let proposals: Vec<ShareProposal> = (0..3)
        .map(|i| ShareProposal {
            witness: authority_for_index(i),
            result_id: hash_for_label("rid1"),
            share: ShareData {
                share_value: format!("share_{i}"),
                nonce_binding: format!("nonce_{i}"),
                data_binding: "cns1:rid1:hash".to_string(),
            },
        })
        .collect();

    let witnesses: BTreeSet<_> = (0..5).map(authority_for_index).collect();

    // threshold = 2: should pass with 3 proposals
    assert!(check_threshold_ref(&proposals, 2));

    let mut state = ConsensusState::new(
        consensus_id_for_label("cns1"),
        operation_for_index(1),
        hash_for_label("hash"),
        threshold(2),
        witnesses,
        authority_for_index(0),
        PathSelection::FastPath,
    );
    state.proposals = proposals.clone();
    // Use state.threshold_met() - the correct equivalent to check_threshold_ref
    assert!(state.threshold_met());

    // threshold = 4: should fail with 3 proposals
    assert!(!check_threshold_ref(&proposals, 4));

    state.threshold = threshold(4);
    assert!(!state.threshold_met());
}

#[test]
fn test_equivocator_detection_equivalence() {
    // Create votes with one equivocator (w1 votes for both rid1 and rid2)
    let votes = vec![
        Vote {
            witness: authority_for_index(0),
            result_id: hash_for_label("rid1"),
            prestate_hash: hash_for_label("h"),
        },
        Vote {
            witness: authority_for_index(1),
            result_id: hash_for_label("rid1"),
            prestate_hash: hash_for_label("h"),
        },
        Vote {
            witness: authority_for_index(1),
            result_id: hash_for_label("rid2"), // Equivocation!
            prestate_hash: hash_for_label("h"),
        },
        Vote {
            witness: authority_for_index(2),
            result_id: hash_for_label("rid1"),
            prestate_hash: hash_for_label("h"),
        },
    ];

    // Reference: should detect w1
    let equivocators = detect_equivocators_ref(&votes);
    assert!(equivocators.contains(&authority_for_index(1)));
    assert!(!equivocators.contains(&authority_for_index(0)));
    assert!(!equivocators.contains(&authority_for_index(2)));

    // Production: convert to proposals and check
    let proposals: Vec<ShareProposal> = votes
        .iter()
        .map(|v| ShareProposal {
            witness: v.witness,
            result_id: v.result_id,
            share: ShareData {
                share_value: "s".to_string(),
                nonce_binding: "n".to_string(),
                data_binding: "b".to_string(),
            },
        })
        .collect();

    assert!(is_equivocator(&proposals, &authority_for_index(1)));
    assert!(!is_equivocator(&proposals, &authority_for_index(0)));
    assert!(!is_equivocator(&proposals, &authority_for_index(2)));
}

#[test]
fn test_aggregation_bindings() {
    let proposals: Vec<ShareProposal> = (0..3)
        .map(|i| ShareProposal {
            witness: authority_for_index(i),
            result_id: hash_for_label("rid1"),
            share: ShareData {
                share_value: format!("share_{i}"),
                nonce_binding: format!("nonce_{i}"),
                data_binding: "cns1:rid1:hash123".to_string(),
            },
        })
        .collect();

    let sig = aggregate_shares_ref(&proposals, 2).unwrap_or_else(|| panic!("Should aggregate"));

    // Verify bindings extracted correctly
    assert_eq!(sig.bound_cid, consensus_id_for_label("cns1"));
    assert_eq!(sig.bound_rid, hash_for_label("rid1"));
    assert_eq!(sig.bound_phash, hash_for_label("hash123"));
    assert_eq!(sig.signer_set.len(), 3);
}

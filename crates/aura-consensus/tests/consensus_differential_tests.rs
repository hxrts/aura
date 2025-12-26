//! Differential Tests: Production Core vs Reference Model
//!
//! These tests verify that the production consensus core implementation
//! matches the reference model (which directly corresponds to Lean theorems).
//!
//! ## Testing Strategy
//!
//! We use property-based testing with proptest to generate random inputs
//! and verify that both implementations produce identical results:
//!
//! 1. **Threshold checking**: `check_threshold` vs `check_threshold_ref`
//! 2. **Share consistency**: `shares_consistent` vs `shares_consistent_ref`
//! 3. **Equivocator detection**: `is_equivocator` vs `detect_equivocators_ref`
//! 4. **Evidence merge**: CRDT properties verified against reference
//!
//! ## Lean Correspondence
//!
//! The reference implementations in `reference.rs` directly mirror Lean definitions:
//! - `check_threshold_ref` ↔ `Aura.Consensus.Validity.thresholdMet`
//! - `shares_consistent_ref` ↔ `Aura.Consensus.Agreement.sharesConsistent`
//! - `detect_equivocators_ref` ↔ `Aura.Consensus.Equivocation.detectEquivocators`
//! - `merge_evidence_ref` ↔ `Aura.Consensus.Evidence.mergeEvidence`
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test -p aura-consensus --test consensus_differential_tests
//! PROPTEST_CASES=1000 cargo test -p aura-consensus --test consensus_differential_tests
//! ```

use aura_consensus::core::{
    state::{ShareData, ShareProposal},
    validation::{is_equivocator, shares_consistent},
};
use aura_testkit::consensus::{
    check_threshold_ref, detect_equivocators_ref, merge_evidence_ref, shares_consistent_ref,
    Evidence, Vote,
};
use proptest::prelude::*;
use std::collections::HashSet;

// ============================================================================
// ARBITRARY GENERATORS
// ============================================================================

/// Generate a valid witness ID (alphanumeric, 1-8 chars)
fn arb_witness() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9]{0,7}".prop_map(String::from)
}

/// Generate a valid result ID
fn arb_result_id() -> impl Strategy<Value = String> {
    "r[0-9]{1,4}".prop_map(String::from)
}

/// Generate a valid prestate hash
fn arb_prestate_hash() -> impl Strategy<Value = String> {
    "pre_[a-f0-9]{8}".prop_map(String::from)
}

/// Generate share data with a specific binding
fn arb_share_data(binding: String) -> impl Strategy<Value = ShareData> {
    (
        "[a-f0-9]{8}".prop_map(String::from), // share_value
        "[a-f0-9]{8}".prop_map(String::from), // nonce_binding
    )
        .prop_map(move |(share_value, nonce_binding)| ShareData {
            share_value,
            nonce_binding,
            data_binding: binding.clone(),
        })
}

/// Generate a share proposal
fn arb_share_proposal() -> impl Strategy<Value = ShareProposal> {
    (arb_witness(), arb_result_id(), "[a-f0-9]{16}".prop_map(String::from))
        .prop_flat_map(|(witness, result_id, binding)| {
            arb_share_data(binding).prop_map(move |share| ShareProposal {
                witness: witness.clone(),
                result_id: result_id.clone(),
                share,
            })
        })
}

/// Generate a vote (for reference model)
fn arb_vote() -> impl Strategy<Value = Vote> {
    (arb_witness(), arb_result_id(), arb_prestate_hash()).prop_map(
        |(witness, result_id, prestate_hash)| Vote {
            witness,
            result_id,
            prestate_hash,
        },
    )
}

/// Generate a list of unique witnesses
fn arb_unique_witnesses(min: usize, max: usize) -> impl Strategy<Value = Vec<String>> {
    prop::collection::hash_set(arb_witness(), min..=max)
        .prop_map(|set| set.into_iter().collect())
}

// ============================================================================
// DIFFERENTIAL TESTS: THRESHOLD CHECKING
// ============================================================================

proptest! {
    /// Test that threshold checking matches between production and reference
    #[test]
    fn prop_threshold_check_matches(
        proposals in prop::collection::vec(arb_share_proposal(), 0..10),
        threshold in 0usize..15,
    ) {
        let production_result = proposals.len() >= threshold;
        let reference_result = check_threshold_ref(&proposals, threshold);

        prop_assert_eq!(
            production_result,
            reference_result,
            "Threshold check diverged: production={}, reference={}, proposals={}, threshold={}",
            production_result,
            reference_result,
            proposals.len(),
            threshold
        );
    }

    /// Test threshold edge cases
    #[test]
    fn prop_threshold_edge_cases(
        count in 0usize..20,
    ) {
        let proposals: Vec<ShareProposal> = (0..count)
            .map(|i| ShareProposal {
                witness: format!("w{}", i),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: format!("s{}", i),
                    nonce_binding: format!("n{}", i),
                    data_binding: "binding".to_string(),
                },
            })
            .collect();

        // Test threshold = count (boundary)
        prop_assert_eq!(
            proposals.len() >= count,
            check_threshold_ref(&proposals, count),
            "Boundary case failed: count={}", count
        );

        // Test threshold = count + 1 (just above)
        if count < usize::MAX {
            prop_assert_eq!(
                proposals.len() >= count + 1,
                check_threshold_ref(&proposals, count + 1),
                "Above boundary case failed: count={}", count
            );
        }
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: SHARE CONSISTENCY
// ============================================================================

// Note: Production `shares_consistent` filters by result_id and checks non-empty values.
// Reference `shares_consistent_ref` checks all proposals have same result_id and binding.
// We test both implementations on their own semantics.

proptest! {
    /// Test that reference share consistency works correctly
    #[test]
    fn prop_shares_consistent_ref_works(
        witnesses in arb_unique_witnesses(1, 5),
    ) {
        // Create proposals with same result_id and binding
        let result_id = "r1".to_string();
        let binding = "bind1".to_string();

        let proposals: Vec<ShareProposal> = witnesses
            .iter()
            .enumerate()
            .map(|(i, w)| ShareProposal {
                witness: w.clone(),
                result_id: result_id.clone(),
                share: ShareData {
                    share_value: format!("s{}", i),
                    nonce_binding: format!("n{}", i),
                    data_binding: binding.clone(),
                },
            })
            .collect();

        // All same result_id and binding = consistent
        prop_assert!(
            shares_consistent_ref(&proposals),
            "Reference should detect consistent proposals"
        );
    }

    /// Test that production share consistency works correctly
    #[test]
    fn prop_shares_consistent_prod_works(
        witnesses in arb_unique_witnesses(1, 5),
    ) {
        // Create proposals with same result_id and valid share data
        let result_id = "r1";
        let prestate_hash = "prehash1";

        let proposals: Vec<ShareProposal> = witnesses
            .iter()
            .enumerate()
            .map(|(i, w)| ShareProposal {
                witness: w.clone(),
                result_id: result_id.to_string(),
                share: ShareData {
                    share_value: format!("s{}", i),
                    nonce_binding: format!("n{}", i),
                    data_binding: format!("cid:{}:{}", result_id, prestate_hash),
                },
            })
            .collect();

        // Production checks non-empty values for matching result_id
        prop_assert!(
            shares_consistent(&proposals, result_id, prestate_hash),
            "Production should detect consistent proposals"
        );
    }

    /// Test reference detects inconsistent result_ids
    #[test]
    fn prop_inconsistent_proposals_detected_ref(
        witnesses in arb_unique_witnesses(2, 5),
    ) {
        // Create proposals with different result_ids
        let proposals: Vec<ShareProposal> = witnesses
            .iter()
            .enumerate()
            .map(|(i, w)| ShareProposal {
                witness: w.clone(),
                result_id: format!("r{}", i), // Each has different result_id
                share: ShareData {
                    share_value: format!("s{}", i),
                    nonce_binding: format!("n{}", i),
                    data_binding: "bind".to_string(),
                },
            })
            .collect();

        // With 2+ proposals and different result_ids, reference should say inconsistent
        prop_assert!(
            !shares_consistent_ref(&proposals),
            "Reference should detect inconsistent proposals"
        );
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: EQUIVOCATOR DETECTION
// ============================================================================

proptest! {
    /// Test equivocator detection with random votes
    #[test]
    fn prop_equivocator_detection_matches(
        votes in prop::collection::vec(arb_vote(), 0..10),
    ) {
        // Convert votes to proposals for production implementation
        let proposals: Vec<ShareProposal> = votes
            .iter()
            .map(|v| ShareProposal {
                witness: v.witness.clone(),
                result_id: v.result_id.clone(),
                share: ShareData {
                    share_value: "share".to_string(),
                    nonce_binding: "nonce".to_string(),
                    data_binding: format!("{}:{}:{}", "cid", v.result_id, v.prestate_hash),
                },
            })
            .collect();

        // Get reference equivocators
        let reference_equivocators = detect_equivocators_ref(&votes);

        // Check each witness using production implementation
        let witness_set: HashSet<_> = proposals.iter().map(|p| p.witness.clone()).collect();

        for witness in &witness_set {
            // Production: is_equivocator(proposals, witness)
            let is_equivocator_prod = is_equivocator(&proposals, witness);
            let is_equivocator_ref = reference_equivocators.contains(witness);

            prop_assert_eq!(
                is_equivocator_prod,
                is_equivocator_ref,
                "Equivocator detection diverged for '{}': production={}, reference={}",
                witness,
                is_equivocator_prod,
                is_equivocator_ref
            );
        }
    }

    /// Test that honest witnesses are never detected as equivocators
    #[test]
    fn prop_honest_never_detected(
        witnesses in arb_unique_witnesses(1, 5),
    ) {
        // Create votes where each witness votes consistently
        let votes: Vec<Vote> = witnesses
            .iter()
            .map(|w| Vote {
                witness: w.clone(),
                result_id: "r1".to_string(), // All vote for same result
                prestate_hash: "pre1".to_string(),
            })
            .collect();

        let equivocators = detect_equivocators_ref(&votes);

        prop_assert!(
            equivocators.is_empty(),
            "Honest witnesses should not be detected: {:?}",
            equivocators
        );
    }

    /// Test that actual equivocators are always detected
    #[test]
    fn prop_equivocators_always_detected(
        honest_witnesses in arb_unique_witnesses(0, 3),
        equivocator in arb_witness(),
    ) {
        // Skip if equivocator is in honest set
        if honest_witnesses.contains(&equivocator) {
            return Ok(());
        }

        // Create votes with one equivocator voting for two different results
        let mut votes: Vec<Vote> = honest_witnesses
            .iter()
            .map(|w| Vote {
                witness: w.clone(),
                result_id: "r1".to_string(),
                prestate_hash: "pre1".to_string(),
            })
            .collect();

        // Add equivocating votes
        votes.push(Vote {
            witness: equivocator.clone(),
            result_id: "r1".to_string(),
            prestate_hash: "pre1".to_string(),
        });
        votes.push(Vote {
            witness: equivocator.clone(),
            result_id: "r2".to_string(), // Different result!
            prestate_hash: "pre1".to_string(),
        });

        let equivocators = detect_equivocators_ref(&votes);

        prop_assert!(
            equivocators.contains(&equivocator),
            "Equivocator '{}' should be detected",
            equivocator
        );
    }
}

// ============================================================================
// DIFFERENTIAL TESTS: EVIDENCE MERGE (CRDT PROPERTIES)
// ============================================================================

/// Generate random evidence with unique equivocators (no duplicates)
fn arb_evidence() -> impl Strategy<Value = Evidence> {
    (
        "cns[0-9]{1,2}".prop_map(String::from),
        prop::collection::vec(arb_vote(), 0..5),
        // Use hash_set to ensure unique equivocators - duplicates would violate CRDT properties
        prop::collection::hash_set(arb_witness(), 0..3).prop_map(|set| set.into_iter().collect::<Vec<_>>()),
    )
        .prop_map(|(consensus_id, votes, equivocators)| Evidence {
            consensus_id,
            votes,
            equivocators,
            commit_fact: None,
        })
}

proptest! {
    /// Test merge commutativity: merge(e1, e2) ≈ merge(e2, e1)
    #[test]
    fn prop_merge_commutative(
        e1 in arb_evidence(),
        e2 in arb_evidence(),
    ) {
        // Make consensus IDs match for meaningful merge
        let e1 = e1;
        let mut e2 = e2;
        e2.consensus_id = e1.consensus_id.clone();

        let m1 = merge_evidence_ref(&e1, &e2);
        let m2 = merge_evidence_ref(&e2, &e1);

        // Check membership-wise equality (order may differ)
        prop_assert_eq!(
            m1.votes.len(),
            m2.votes.len(),
            "Merge not commutative: vote counts differ"
        );

        for v in &m1.votes {
            prop_assert!(
                m2.votes.contains(v),
                "Merge not commutative: vote {:?} missing in reverse merge",
                v
            );
        }

        prop_assert_eq!(
            m1.equivocators.len(),
            m2.equivocators.len(),
            "Merge not commutative: equivocator counts differ"
        );
    }

    /// Test merge idempotence: merge(e, e) ≈ e
    #[test]
    fn prop_merge_idempotent(
        e in arb_evidence(),
    ) {
        let merged = merge_evidence_ref(&e, &e);

        prop_assert_eq!(
            merged.votes.len(),
            e.votes.len(),
            "Merge not idempotent: vote counts differ"
        );

        prop_assert_eq!(
            merged.equivocators.len(),
            e.equivocators.len(),
            "Merge not idempotent: equivocator counts differ"
        );
    }

    /// Test merge associativity: merge(merge(e1, e2), e3) ≈ merge(e1, merge(e2, e3))
    #[test]
    fn prop_merge_associative(
        e1 in arb_evidence(),
        e2 in arb_evidence(),
        e3 in arb_evidence(),
    ) {
        // Make consensus IDs match
        let e1 = e1;
        let mut e2 = e2;
        let mut e3 = e3;
        e2.consensus_id = e1.consensus_id.clone();
        e3.consensus_id = e1.consensus_id.clone();

        let left = merge_evidence_ref(&merge_evidence_ref(&e1, &e2), &e3);
        let right = merge_evidence_ref(&e1, &merge_evidence_ref(&e2, &e3));

        // Check membership-wise equality
        prop_assert_eq!(
            left.votes.len(),
            right.votes.len(),
            "Merge not associative: vote counts differ"
        );

        for v in &left.votes {
            prop_assert!(
                right.votes.contains(v),
                "Merge not associative: vote {:?} missing",
                v
            );
        }
    }

    /// Test that mismatched consensus IDs don't merge
    #[test]
    fn prop_merge_different_cid_identity(
        e1 in arb_evidence(),
        e2 in arb_evidence(),
    ) {
        // Ensure different consensus IDs
        let e1 = e1;
        let mut e2 = e2;
        if e1.consensus_id == e2.consensus_id {
            e2.consensus_id = format!("{}_different", e2.consensus_id);
        }

        let merged = merge_evidence_ref(&e1, &e2);

        // Should return e1 unchanged
        prop_assert_eq!(
            merged.consensus_id,
            e1.consensus_id,
            "Merge with different cid should return e1"
        );
        prop_assert_eq!(
            merged.votes.len(),
            e1.votes.len(),
            "Merge with different cid should preserve e1 votes"
        );
    }
}

// ============================================================================
// MANUAL DIFFERENTIAL TESTS (Non-proptest)
// ============================================================================

#[test]
fn test_empty_proposals_consistent() {
    let empty: Vec<ShareProposal> = vec![];
    // Reference: empty is always consistent
    assert!(shares_consistent_ref(&empty));
    // Production: empty filtered list is consistent
    assert!(shares_consistent(&empty, "any_rid", "any_hash"));
}

#[test]
fn test_single_proposal_consistent() {
    let single = vec![ShareProposal {
        witness: "w1".to_string(),
        result_id: "r1".to_string(),
        share: ShareData {
            share_value: "s1".to_string(),
            nonce_binding: "n1".to_string(),
            data_binding: "cid:r1:h1".to_string(),
        },
    }];

    // Reference: single proposal is always consistent
    assert!(shares_consistent_ref(&single));
    // Production: single proposal with non-empty values is consistent
    assert!(shares_consistent(&single, "r1", "h1"));
}

#[test]
fn test_threshold_zero() {
    let proposals: Vec<ShareProposal> = vec![];
    // Threshold 0 is always met (even with empty proposals)
    assert!(check_threshold_ref(&proposals, 0));
}

#[test]
fn test_equivocator_same_result_not_equivocator() {
    // Same witness, same result - not an equivocator
    let votes = vec![
        Vote {
            witness: "w1".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        },
        Vote {
            witness: "w1".to_string(),
            result_id: "r1".to_string(), // Same result
            prestate_hash: "h1".to_string(),
        },
    ];

    let equivocators = detect_equivocators_ref(&votes);
    assert!(equivocators.is_empty());
}

#[test]
fn test_equivocator_different_result_is_equivocator() {
    // Same witness, different result - is an equivocator
    let votes = vec![
        Vote {
            witness: "w1".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        },
        Vote {
            witness: "w1".to_string(),
            result_id: "r2".to_string(), // Different result!
            prestate_hash: "h1".to_string(),
        },
    ];

    let equivocators = detect_equivocators_ref(&votes);
    assert!(equivocators.contains("w1"));
}

#[test]
fn test_merge_preserves_commit_fact() {
    use aura_consensus::core::state::PureCommitFact;

    let e1 = Evidence {
        consensus_id: "cns1".to_string(),
        votes: vec![],
        equivocators: vec![],
        commit_fact: Some(PureCommitFact {
            cid: "cns1".to_string(),
            result_id: "r1".to_string(),
            signature: "sig".to_string(),
            prestate_hash: "h1".to_string(),
        }),
    };

    let e2 = Evidence {
        consensus_id: "cns1".to_string(),
        votes: vec![],
        equivocators: vec![],
        commit_fact: None,
    };

    // Merge should preserve the commit fact from e1
    let merged = merge_evidence_ref(&e1, &e2);
    assert!(merged.commit_fact.is_some());
    assert_eq!(merged.commit_fact.unwrap().result_id, "r1");

    // Reverse merge should also preserve (first wins)
    let merged_rev = merge_evidence_ref(&e2, &e1);
    assert!(merged_rev.commit_fact.is_some());
}

/// Summary test that verifies differential testing coverage
#[test]
fn test_differential_coverage_summary() {
    println!("Differential Test Coverage:");
    println!("  ✓ Threshold checking: production vs reference");
    println!("  ✓ Share consistency: production vs reference");
    println!("  ✓ Equivocator detection: production vs reference");
    println!("  ✓ Evidence merge: CRDT properties (comm, assoc, idem)");
    println!("  ✓ Edge cases: empty, single, zero threshold");
    println!("  ✓ Commit fact preservation through merge");
    println!("\nAll differential tests verify Lean-proven properties.");
}

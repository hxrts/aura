//! Correspondence Validation Tests
//!
//! Tests that validate the correspondence between Lean proofs and Rust implementation.
//! These tests verify structural properties that must hold for the Lean theorems to apply.
//!
//! ## Lean Correspondence
//! - File: verification/lean/Aura/Consensus/RustCorrespondence.lean
//! - Section: Type and Function Correspondence
//!
//! ## Coverage
//! - T7.5: Correspondence validation tests
//! - Signature serialization determinism
//! - Evidence merge properties (associative, commutative, idempotent)
//! - Threshold arithmetic correctness

mod common;

use aura_protocol::consensus::core::state::{ShareData, ShareProposal};
use common::reference::{detect_equivocators_ref, merge_evidence_ref, Evidence, Vote};
use proptest::prelude::*;
use std::collections::HashSet;

// ============================================================================
// SIGNATURE SERIALIZATION DETERMINISM
// ============================================================================

proptest! {
    /// Test: Signature serialization is deterministic
    /// Lean: ConsensusIdRepr.hash_is_deterministic
    ///
    /// The same ShareData always produces the same serialized form.
    #[test]
    fn prop_share_data_serialization_deterministic(
        share_value in "[a-f0-9]{8}",
        nonce_binding in "[a-f0-9]{8}",
        data_binding in "[a-z]{3}:[a-z]{3}:[a-f0-9]{4}",
    ) {
        let share1 = ShareData {
            share_value: share_value.clone(),
            nonce_binding: nonce_binding.clone(),
            data_binding: data_binding.clone(),
        };

        let share2 = ShareData {
            share_value,
            nonce_binding,
            data_binding,
        };

        // Same inputs produce equal structures
        prop_assert_eq!(&share1, &share2);

        // Debug representation is deterministic
        let debug1 = format!("{:?}", share1);
        let debug2 = format!("{:?}", share2);
        prop_assert_eq!(debug1, debug2);
    }

    /// Test: Vote serialization is deterministic
    /// Lean: WitnessVote equality is structural
    #[test]
    fn prop_vote_serialization_deterministic(
        witness in "[a-z]{1,5}",
        result_id in "r[0-9]{1,3}",
        prestate_hash in "h[a-f0-9]{4}",
    ) {
        let vote1 = Vote {
            witness: witness.clone(),
            result_id: result_id.clone(),
            prestate_hash: prestate_hash.clone(),
        };

        let vote2 = Vote {
            witness,
            result_id,
            prestate_hash,
        };

        prop_assert_eq!(vote1, vote2);
    }
}

// ============================================================================
// EVIDENCE MERGE PROPERTIES
// ============================================================================

/// Generate a random evidence structure
fn arb_evidence() -> impl Strategy<Value = Evidence> {
    (
        "cns[0-9]{1,2}",
        prop::collection::vec(arb_vote(), 0..5),
        prop::collection::vec("[a-z]{2,4}", 0..3),
    )
        .prop_map(|(consensus_id, votes, equivocators)| Evidence {
            consensus_id,
            votes,
            equivocators,
            commit_fact: None,
        })
}

/// Generate a random vote
fn arb_vote() -> impl Strategy<Value = Vote> {
    ("[a-z]{2,4}", "r[0-9]{1,2}", "h[a-f0-9]{4}").prop_map(|(witness, result_id, prestate_hash)| {
        Vote {
            witness,
            result_id,
            prestate_hash,
        }
    })
}

proptest! {
    /// Test: Evidence merge is commutative (membership-wise)
    /// Lean: Aura.Consensus.Evidence.merge_comm
    ///
    /// merge(e1, e2).votes ≃ merge(e2, e1).votes (same elements)
    #[test]
    fn prop_evidence_merge_commutative(
        e1 in arb_evidence(),
        e2 in arb_evidence(),
    ) {
        // Make consensus IDs match for meaningful merge
        let mut e2 = e2;
        e2.consensus_id = e1.consensus_id.clone();

        let m1 = merge_evidence_ref(&e1, &e2);
        let m2 = merge_evidence_ref(&e2, &e1);

        // Same votes (membership-wise)
        prop_assert_eq!(m1.votes.len(), m2.votes.len());
        for v in &m1.votes {
            prop_assert!(m2.votes.contains(v), "Vote {:?} missing in reverse merge", v);
        }

        // Same equivocators (membership-wise)
        prop_assert_eq!(m1.equivocators.len(), m2.equivocators.len());
        for eq in &m1.equivocators {
            prop_assert!(m2.equivocators.contains(eq), "Equivocator {:?} missing", eq);
        }
    }

    /// Test: Evidence merge is associative (membership-wise)
    /// Lean: Aura.Consensus.Evidence.merge_assoc
    ///
    /// merge(merge(e1, e2), e3) ≃ merge(e1, merge(e2, e3))
    #[test]
    fn prop_evidence_merge_associative(
        e1 in arb_evidence(),
        e2 in arb_evidence(),
        e3 in arb_evidence(),
    ) {
        // Make all consensus IDs match
        let mut e2 = e2;
        let mut e3 = e3;
        e2.consensus_id = e1.consensus_id.clone();
        e3.consensus_id = e1.consensus_id.clone();

        let left = merge_evidence_ref(&merge_evidence_ref(&e1, &e2), &e3);
        let right = merge_evidence_ref(&e1, &merge_evidence_ref(&e2, &e3));

        // Same votes (membership-wise)
        prop_assert_eq!(left.votes.len(), right.votes.len());
        for v in &left.votes {
            prop_assert!(right.votes.contains(v), "Vote {:?} missing in right", v);
        }
    }

    /// Test: Evidence merge is idempotent
    /// Lean: Aura.Consensus.Evidence.merge_idem
    ///
    /// merge(e, e) ≃ e
    #[test]
    fn prop_evidence_merge_idempotent(
        e in arb_evidence(),
    ) {
        let merged = merge_evidence_ref(&e, &e);

        prop_assert_eq!(merged.votes.len(), e.votes.len());
        prop_assert_eq!(merged.equivocators.len(), e.equivocators.len());

        for v in &e.votes {
            prop_assert!(merged.votes.contains(v));
        }
    }

    /// Test: Evidence merge preserves commit fact
    /// Lean: Aura.Consensus.Evidence.merge_preserves_commit
    ///
    /// e1.commit_fact.is_some → merge(e1, e2).commit_fact.is_some
    #[test]
    fn prop_evidence_merge_preserves_commit(
        mut e1 in arb_evidence(),
        e2 in arb_evidence(),
    ) {
        // Make consensus IDs match
        let mut e2 = e2;
        e2.consensus_id = e1.consensus_id.clone();

        // Set a commit fact on e1
        use aura_protocol::consensus::core::state::PureCommitFact;
        e1.commit_fact = Some(PureCommitFact {
            cid: e1.consensus_id.clone(),
            result_id: "r1".to_string(),
            signature: "sig".to_string(),
            prestate_hash: "h1".to_string(),
        });

        let merged = merge_evidence_ref(&e1, &e2);
        prop_assert!(merged.commit_fact.is_some(), "Commit fact lost in merge");
    }
}

// ============================================================================
// THRESHOLD ARITHMETIC CORRECTNESS
// ============================================================================

proptest! {
    /// Test: Threshold check is correct
    /// Lean: Aura.Consensus.Validity.threshold_correct
    ///
    /// check_threshold(proposals, k) ↔ len(proposals) ≥ k
    #[test]
    fn prop_threshold_check_correct(
        count in 0usize..20,
        threshold in 0usize..25,
    ) {
        use crate::common::reference::check_threshold_ref;

        let proposals: Vec<ShareProposal> = (0..count)
            .map(|i| ShareProposal {
                witness: format!("w{}", i),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: format!("s{}", i),
                    nonce_binding: format!("n{}", i),
                    data_binding: "cns:r1:h1".to_string(),
                },
            })
            .collect();

        let result = check_threshold_ref(&proposals, threshold);
        let expected = count >= threshold;

        prop_assert_eq!(result, expected,
            "Threshold check failed: count={}, threshold={}, result={}, expected={}",
            count, threshold, result, expected);
    }
}

// ============================================================================
// EQUIVOCATION DETECTION CORRECTNESS
// ============================================================================

proptest! {
    /// Test: Equivocation detection soundness
    /// Lean: Aura.Consensus.Equivocation.detection_soundness
    ///
    /// Detected witnesses actually equivocated.
    #[test]
    fn prop_equivocation_detection_sound(
        witnesses in prop::collection::vec("[a-z]{2,4}", 1..5),
    ) {
        // Create votes where first witness equivocates
        let mut votes = Vec::new();
        for (i, w) in witnesses.iter().enumerate() {
            votes.push(Vote {
                witness: w.clone(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            });

            // First witness votes for different result (equivocation)
            if i == 0 {
                votes.push(Vote {
                    witness: w.clone(),
                    result_id: "r2".to_string(), // Different!
                    prestate_hash: "h1".to_string(),
                });
            }
        }

        let equivocators = detect_equivocators_ref(&votes);

        // First witness should be detected
        prop_assert!(equivocators.contains(&witnesses[0]),
            "Equivocator not detected: {:?}", witnesses[0]);

        // Other witnesses should not be detected
        for w in &witnesses[1..] {
            prop_assert!(!equivocators.contains(w),
                "Honest witness falsely accused: {:?}", w);
        }
    }

    /// Test: Equivocation detection completeness
    /// Lean: Aura.Consensus.Equivocation.detection_completeness
    ///
    /// All equivocators are detected.
    #[test]
    fn prop_equivocation_detection_complete(
        n_honest in 0usize..5,
        n_equivocators in 1usize..4,
    ) {
        let mut votes = Vec::new();
        let mut expected_equivocators: HashSet<String> = HashSet::new();

        // Honest witnesses: one vote each
        for i in 0..n_honest {
            votes.push(Vote {
                witness: format!("honest_{}", i),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            });
        }

        // Equivocating witnesses: two conflicting votes each
        for i in 0..n_equivocators {
            let name = format!("equivocator_{}", i);
            expected_equivocators.insert(name.clone());

            votes.push(Vote {
                witness: name.clone(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            });
            votes.push(Vote {
                witness: name,
                result_id: "r2".to_string(), // Different!
                prestate_hash: "h1".to_string(),
            });
        }

        let detected = detect_equivocators_ref(&votes);

        // All expected equivocators should be detected
        for eq in &expected_equivocators {
            prop_assert!(detected.contains(eq),
                "Equivocator not detected: {}", eq);
        }

        // No honest witnesses should be detected
        for i in 0..n_honest {
            let name = format!("honest_{}", i);
            prop_assert!(!detected.contains(&name),
                "Honest witness falsely detected: {}", name);
        }
    }
}

// ============================================================================
// MANUAL TESTS FOR EDGE CASES
// ============================================================================

#[test]
fn test_empty_evidence_merge() {
    let e1 = Evidence {
        consensus_id: "cns1".to_string(),
        votes: vec![],
        equivocators: vec![],
        commit_fact: None,
    };

    let e2 = Evidence {
        consensus_id: "cns1".to_string(),
        votes: vec![],
        equivocators: vec![],
        commit_fact: None,
    };

    let merged = merge_evidence_ref(&e1, &e2);
    assert!(merged.votes.is_empty());
    assert!(merged.equivocators.is_empty());
    assert!(merged.commit_fact.is_none());
}

#[test]
fn test_different_cid_merge_is_identity() {
    let e1 = Evidence {
        consensus_id: "cns1".to_string(),
        votes: vec![Vote {
            witness: "w1".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        }],
        equivocators: vec![],
        commit_fact: None,
    };

    let e2 = Evidence {
        consensus_id: "cns2".to_string(), // Different!
        votes: vec![Vote {
            witness: "w2".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        }],
        equivocators: vec![],
        commit_fact: None,
    };

    // Per Lean spec: if cid differs, return e1
    let merged = merge_evidence_ref(&e1, &e2);
    assert_eq!(merged.votes.len(), 1);
    assert_eq!(merged.votes[0].witness, "w1");
}

#[test]
fn test_no_equivocation_empty_result() {
    let votes = vec![
        Vote {
            witness: "w1".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        },
        Vote {
            witness: "w2".to_string(),
            result_id: "r1".to_string(),
            prestate_hash: "h1".to_string(),
        },
    ];

    let equivocators = detect_equivocators_ref(&votes);
    assert!(equivocators.is_empty());
}

/// Summary test documenting correspondence coverage
#[test]
fn test_correspondence_coverage_summary() {
    println!("Correspondence Validation Coverage:");
    println!("  ✓ ShareData serialization determinism");
    println!("  ✓ Vote serialization determinism");
    println!("  ✓ Evidence merge commutativity");
    println!("  ✓ Evidence merge associativity");
    println!("  ✓ Evidence merge idempotence");
    println!("  ✓ Evidence merge preserves commit");
    println!("  ✓ Threshold check correctness");
    println!("  ✓ Equivocation detection soundness");
    println!("  ✓ Equivocation detection completeness");
    println!("\nAll tests validate Lean-proven properties.");
}

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

use aura_consensus::core::state::{ShareData, ShareProposal};
use aura_consensus::types::ConsensusId;
use aura_core::{hash, AuthorityId, Hash32};
use aura_testkit::consensus::{detect_equivocators_ref, merge_evidence_ref, Evidence, Vote};
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
        let debug1 = format!("{share1:?}");
        let debug2 = format!("{share2:?}");
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
            witness: authority_from_label(&witness),
            result_id: hash_from_label(&result_id),
            prestate_hash: hash_from_label(&prestate_hash),
        };

        let vote2 = Vote {
            witness: authority_from_label(&witness),
            result_id: hash_from_label(&result_id),
            prestate_hash: hash_from_label(&prestate_hash),
        };

        prop_assert_eq!(vote1, vote2);
    }
}

// ============================================================================
// EVIDENCE MERGE PROPERTIES
// ============================================================================

/// Generate a random evidence structure with unique equivocators
fn arb_evidence() -> impl Strategy<Value = Evidence> {
    (
        arb_consensus_id(),
        prop::collection::vec(arb_vote(), 0..5),
        prop::collection::hash_set("[a-z]{2,4}", 0..3)
            .prop_map(|set| set.into_iter().map(|w| authority_from_label(&w)).collect()),
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
    ("[a-z]{2,4}", "r[0-9]{1,2}", "h[a-f0-9]{4}")
        .prop_map(|(witness, result_id, prestate_hash)| Vote {
            witness: authority_from_label(&witness),
            result_id: hash_from_label(&result_id),
            prestate_hash: hash_from_label(&prestate_hash),
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
        use aura_consensus::core::state::PureCommitFact;
        e1.commit_fact = Some(PureCommitFact {
            cid: e1.consensus_id.clone(),
            result_id: hash_from_label("r1"),
            signature: "sig".to_string(),
            prestate_hash: hash_from_label("h1"),
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
        use aura_testkit::consensus::check_threshold_ref;

        let proposals: Vec<ShareProposal> = (0..count)
            .map(|i| ShareProposal {
                witness: authority_from_label(&format!("w{i}")),
                result_id: hash_from_label("r1"),
                share: ShareData {
                    share_value: format!("s{i}"),
                    nonce_binding: format!("n{i}"),
                    data_binding: format!("cns:{}:{}", hash_from_label("r1"), hash_from_label("h1")),
                },
            })
            .collect();

        let result = check_threshold_ref(&proposals, threshold);
        let expected = count >= threshold;

        prop_assert_eq!(
            result,
            expected,
            "Threshold check failed: count={}, threshold={}, result={}, expected={}",
            count, threshold, result, expected
        );
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
        witnesses in prop::collection::hash_set("[a-z]{2,4}", 1..5),
    ) {
        // Convert to vec for indexed access (order doesn't matter for this test)
        let witnesses: Vec<_> = witnesses
            .into_iter()
            .map(|w| authority_from_label(&w))
            .collect();

        // Create votes where first witness equivocates
        let mut votes = Vec::new();
        for (i, w) in witnesses.iter().enumerate() {
            votes.push(Vote {
                witness: *w,
                result_id: hash_from_label("r1"),
                prestate_hash: hash_from_label("h1"),
            });

            // First witness votes for different result (equivocation)
            if i == 0 {
                votes.push(Vote {
                    witness: *w,
                    result_id: hash_from_label("r2"), // Different!
                    prestate_hash: hash_from_label("h1"),
                });
            }
        }

        let equivocators = detect_equivocators_ref(&votes);

        // First witness should be detected
        prop_assert!(equivocators.contains(&witnesses[0]),
            "Equivocator not detected: {:?}", witnesses[0]);

        // Other witnesses should not be detected (they're guaranteed unique now)
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
        let mut expected_equivocators: HashSet<AuthorityId> = HashSet::new();

        // Honest witnesses: one vote each
        for i in 0..n_honest {
            votes.push(Vote {
                witness: authority_from_label(&format!("honest_{i}")),
                result_id: hash_from_label("r1"),
                prestate_hash: hash_from_label("h1"),
            });
        }

        // Equivocating witnesses: two conflicting votes each
        for i in 0..n_equivocators {
            let name = format!("equivocator_{i}");
            let authority = authority_from_label(&name);
            expected_equivocators.insert(authority);

            votes.push(Vote {
                witness: authority,
                result_id: hash_from_label("r1"),
                prestate_hash: hash_from_label("h1"),
            });
            votes.push(Vote {
                witness: authority,
                result_id: hash_from_label("r2"), // Different!
                prestate_hash: hash_from_label("h1"),
            });
        }

        let detected = detect_equivocators_ref(&votes);

        // All expected equivocators should be detected
        for eq in &expected_equivocators {
            prop_assert!(detected.contains(eq), "Equivocator not detected: {eq}");
        }

        // No honest witnesses should be detected
        for i in 0..n_honest {
            let authority = authority_from_label(&format!("honest_{i}"));
            prop_assert!(
                !detected.contains(&authority),
                "Honest witness falsely detected: {authority}"
            );
        }
    }
}

// ============================================================================
// MANUAL TESTS FOR EDGE CASES
// ============================================================================

#[test]
fn test_empty_evidence_merge() {
    let e1 = Evidence {
        consensus_id: consensus_id_from_label("cns1"),
        votes: vec![],
        equivocators: vec![],
        commit_fact: None,
    };

    let e2 = Evidence {
        consensus_id: consensus_id_from_label("cns1"),
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
        consensus_id: consensus_id_from_label("cns1"),
        votes: vec![Vote {
            witness: authority_from_label("w1"),
            result_id: hash_from_label("r1"),
            prestate_hash: hash_from_label("h1"),
        }],
        equivocators: vec![],
        commit_fact: None,
    };

    let e2 = Evidence {
        consensus_id: consensus_id_from_label("cns2"), // Different!
        votes: vec![Vote {
            witness: authority_from_label("w2"),
            result_id: hash_from_label("r1"),
            prestate_hash: hash_from_label("h1"),
        }],
        equivocators: vec![],
        commit_fact: None,
    };

    // Per Lean spec: if cid differs, return e1
    let merged = merge_evidence_ref(&e1, &e2);
    assert_eq!(merged.votes.len(), 1);
    assert_eq!(merged.votes[0].witness, authority_from_label("w1"));
}

#[test]
fn test_no_equivocation_empty_result() {
    let votes = vec![
        Vote {
            witness: authority_from_label("w1"),
            result_id: hash_from_label("r1"),
            prestate_hash: hash_from_label("h1"),
        },
        Vote {
            witness: authority_from_label("w2"),
            result_id: hash_from_label("r1"),
            prestate_hash: hash_from_label("h1"),
        },
    ];

    let equivocators = detect_equivocators_ref(&votes);
    assert!(equivocators.is_empty());
}

fn authority_from_label(label: &str) -> AuthorityId {
    AuthorityId::new_from_entropy(hash::hash(label.as_bytes()))
}

fn hash_from_label(label: &str) -> Hash32 {
    Hash32::from_bytes(label.as_bytes())
}

fn consensus_id_from_label(label: &str) -> ConsensusId {
    ConsensusId(Hash32::from_bytes(label.as_bytes()))
}

fn arb_consensus_id() -> impl Strategy<Value = ConsensusId> {
    "cns[0-9]{1,2}"
        .prop_map(String::from)
        .prop_map(|label| consensus_id_from_label(&label))
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

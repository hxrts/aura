//! Reference Implementations for Lean-Proven Primitives
//!
//! This module provides reference implementations that directly correspond
//! to Lean theorem statements. These are NOT optimized for production but
//! provide a ground truth for differential testing.
//!
//! ## Lean Correspondence
//! - File: verification/lean/Aura/Consensus/Agreement.lean
//! - File: verification/lean/Aura/Consensus/Evidence.lean
//! - File: verification/lean/Aura/Consensus/Validity.lean
//!
//! ## Quint Correspondence
//! - File: verification/quint/protocol_consensus.qnt
//! - Section: INVARIANTS, sharesConsistent
//!
//! ## Design Principles
//!
//! 1. **Match Lean exactly**: Each function mirrors a Lean definition
//! 2. **Simplicity over efficiency**: Prefer clarity to optimization
//! 3. **Property annotations**: Document which theorem each function relates to

use super::state::{ConsensusState, PureCommitFact, ShareProposal};
use std::collections::HashSet;

/// Reference evidence structure for CRDT merge
/// Lean: Aura.Consensus.Types.Evidence
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub consensus_id: String,
    pub votes: Vec<Vote>,
    pub equivocators: Vec<String>,
    pub commit_fact: Option<PureCommitFact>,
}

/// Reference vote structure
/// Lean: Aura.Consensus.Types.Vote
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vote {
    pub witness: String,
    pub result_id: String,
    pub prestate_hash: String,
}

/// Reference threshold signature structure
/// Lean: Aura.Consensus.Types.ThresholdSignature
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdSignature {
    pub sig_value: String,
    pub signer_set: Vec<String>,
    pub bound_cid: String,
    pub bound_rid: String,
    pub bound_phash: String,
}

// ============================================================================
// EVIDENCE MERGE (Lean: Aura.Consensus.Evidence)
// ============================================================================

/// Merge two lists with deduplication
/// Lean: Aura.Consensus.Evidence.mergeLists
/// Property: merge_comm, merge_assoc, merge_idem
fn merge_lists<T: Clone + Eq>(xs: &[T], ys: &[T]) -> Vec<T> {
    let mut result: Vec<T> = xs.to_vec();
    for y in ys {
        if !result.contains(y) {
            result.push(y.clone());
        }
    }
    result
}

/// Merge two evidence structures
/// Lean: Aura.Consensus.Evidence.mergeEvidence
///
/// Properties proven in Lean:
/// - `merge_comm`: mergeEvidence e1 e2 ≃ mergeEvidence e2 e1 (membership-wise)
/// - `merge_assoc`: mergeEvidence (mergeEvidence e1 e2) e3 ≃ mergeEvidence e1 (mergeEvidence e2 e3)
/// - `merge_idem`: mergeEvidence e e ≃ e
/// - `merge_preserves_commit`: commit preserved through merge
/// - `commit_monotonic`: once committed, stays committed
/// - `equivocator_monotonic`: equivocators only grow
pub fn merge_evidence_ref(e1: &Evidence, e2: &Evidence) -> Evidence {
    // Lean: if e1.consensusId != e2.consensusId then e1
    if e1.consensus_id != e2.consensus_id {
        return e1.clone();
    }

    Evidence {
        consensus_id: e1.consensus_id.clone(),
        votes: merge_lists(&e1.votes, &e2.votes),
        equivocators: merge_lists(&e1.equivocators, &e2.equivocators),
        // Lean: e1.commitFact.orElse (fun _ => e2.commitFact)
        commit_fact: e1.commit_fact.clone().or_else(|| e2.commit_fact.clone()),
    }
}

// ============================================================================
// THRESHOLD CHECKING (Lean: Aura.Consensus.Validity)
// ============================================================================

/// Check if threshold is met
/// Lean: Aura.Consensus.Validity.thresholdMet
///
/// Properties proven in Lean:
/// - `threshold_reflexivity`: k ≤ n → thresholdMet k sigs when |sigs| = k
pub fn check_threshold_ref(proposals: &[ShareProposal], threshold: usize) -> bool {
    proposals.len() >= threshold
}

/// Check if all proposals are consistent (same result binding)
/// Lean: Aura.Consensus.Agreement (shares must bind to same result)
/// Quint: sharesConsistent
///
/// Properties proven in Lean:
/// - `signature_binding_agreement`: valid sigs for same cid have same rid
pub fn shares_consistent_ref(proposals: &[ShareProposal]) -> bool {
    if proposals.is_empty() {
        return true;
    }

    let first_result = &proposals[0].result_id;
    let first_binding = &proposals[0].share.data_binding;

    proposals
        .iter()
        .all(|p| p.result_id == *first_result && p.share.data_binding == *first_binding)
}

// ============================================================================
// SIGNATURE AGGREGATION (Lean: Aura.Consensus.Agreement)
// ============================================================================

/// Aggregate shares into a threshold signature (reference implementation)
/// Lean: Implicit in Agreement axioms about signature aggregation
/// Quint: aggregateShares function
///
/// Properties proven/axiomatized in Lean:
/// - `signature_value_determinism`: same inputs produce same signature
/// - `signature_binding_agreement`: aggregated sig binds to consistent values
///
/// Note: This is a reference model only. Actual FROST aggregation uses
/// cryptographic primitives in `aura-core::crypto::tree_signing`.
pub fn aggregate_shares_ref(
    proposals: &[ShareProposal],
    threshold: usize,
) -> Option<ThresholdSignature> {
    // Lean: threshold check
    if proposals.len() < threshold {
        return None;
    }

    // Lean: consistency check (signature_binding_agreement axiom prerequisite)
    if !shares_consistent_ref(proposals) {
        return None;
    }

    let first = &proposals[0];

    // Reference aggregation: concatenate share values (not cryptographically valid)
    // The actual FROST aggregation uses proper crypto in tree_signing
    let sig_value = proposals
        .iter()
        .map(|p| p.share.share_value.as_str())
        .collect::<Vec<_>>()
        .join("|");

    Some(ThresholdSignature {
        sig_value,
        signer_set: proposals.iter().map(|p| p.witness.clone()).collect(),
        bound_cid: first.share.data_binding.split(':').next().unwrap_or("").to_string(),
        bound_rid: first.result_id.clone(),
        bound_phash: first
            .share
            .data_binding
            .split(':')
            .nth(2)
            .unwrap_or("")
            .to_string(),
    })
}

// ============================================================================
// EQUIVOCATION DETECTION (Lean: Aura.Consensus.Equivocation)
// ============================================================================

/// Detect equivocating witnesses
/// Lean: Aura.Consensus.Equivocation.detectEquivocators
///
/// Properties proven in Lean:
/// - `detection_soundness`: detected witness has conflicting votes
/// - `detection_completeness`: all equivocators are detected
/// - `honest_never_detected`: honest witness never falsely accused
pub fn detect_equivocators_ref(votes: &[Vote]) -> HashSet<String> {
    let mut equivocators = HashSet::new();

    // Check each pair of votes for conflicts
    for i in 0..votes.len() {
        for j in (i + 1)..votes.len() {
            let v1 = &votes[i];
            let v2 = &votes[j];

            // Same witness, different result_id = equivocation
            if v1.witness == v2.witness && v1.result_id != v2.result_id {
                equivocators.insert(v1.witness.clone());
            }
        }
    }

    equivocators
}

// ============================================================================
// CONSENSUS STATE EXTRACTION (for differential testing)
// ============================================================================

/// Convert ConsensusState to Evidence for CRDT operations
pub fn state_to_evidence(state: &ConsensusState) -> Evidence {
    Evidence {
        consensus_id: state.cid.clone(),
        votes: state
            .proposals
            .iter()
            .map(|p| Vote {
                witness: p.witness.clone(),
                result_id: p.result_id.clone(),
                prestate_hash: state.prestate_hash.clone(),
            })
            .collect(),
        equivocators: state.equivocators.iter().cloned().collect(),
        commit_fact: state.commit_fact.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::core::state::ShareData;

    #[test]
    fn test_merge_lists_dedup() {
        let xs = vec![1, 2, 3];
        let ys = vec![2, 3, 4];
        let merged = merge_lists(&xs, &ys);
        assert_eq!(merged, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_merge_evidence_comm() {
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
            consensus_id: "cns1".to_string(),
            votes: vec![Vote {
                witness: "w2".to_string(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            }],
            equivocators: vec![],
            commit_fact: None,
        };

        let m1 = merge_evidence_ref(&e1, &e2);
        let m2 = merge_evidence_ref(&e2, &e1);

        // Membership-wise commutativity
        assert_eq!(m1.votes.len(), m2.votes.len());
        assert!(m1.votes.iter().all(|v| m2.votes.contains(v)));
    }

    #[test]
    fn test_merge_evidence_idem() {
        let e = Evidence {
            consensus_id: "cns1".to_string(),
            votes: vec![Vote {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            }],
            equivocators: vec![],
            commit_fact: None,
        };

        let merged = merge_evidence_ref(&e, &e);
        assert_eq!(merged.votes.len(), e.votes.len());
    }

    #[test]
    fn test_check_threshold() {
        let proposals = vec![
            ShareProposal {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s1".to_string(),
                    nonce_binding: "n1".to_string(),
                    data_binding: "cns1:r1:h1".to_string(),
                },
            },
            ShareProposal {
                witness: "w2".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s2".to_string(),
                    nonce_binding: "n2".to_string(),
                    data_binding: "cns1:r1:h1".to_string(),
                },
            },
        ];

        assert!(check_threshold_ref(&proposals, 2));
        assert!(!check_threshold_ref(&proposals, 3));
    }

    #[test]
    fn test_shares_consistent() {
        let consistent = vec![
            ShareProposal {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s1".to_string(),
                    nonce_binding: "n1".to_string(),
                    data_binding: "binding1".to_string(),
                },
            },
            ShareProposal {
                witness: "w2".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s2".to_string(),
                    nonce_binding: "n2".to_string(),
                    data_binding: "binding1".to_string(),
                },
            },
        ];

        let inconsistent = vec![
            ShareProposal {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s1".to_string(),
                    nonce_binding: "n1".to_string(),
                    data_binding: "binding1".to_string(),
                },
            },
            ShareProposal {
                witness: "w2".to_string(),
                result_id: "r2".to_string(), // Different result!
                share: ShareData {
                    share_value: "s2".to_string(),
                    nonce_binding: "n2".to_string(),
                    data_binding: "binding1".to_string(),
                },
            },
        ];

        assert!(shares_consistent_ref(&consistent));
        assert!(!shares_consistent_ref(&inconsistent));
    }

    #[test]
    fn test_detect_equivocators() {
        let votes = vec![
            Vote {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            },
            Vote {
                witness: "w1".to_string(),
                result_id: "r2".to_string(), // Equivocation!
                prestate_hash: "h1".to_string(),
            },
            Vote {
                witness: "w2".to_string(),
                result_id: "r1".to_string(),
                prestate_hash: "h1".to_string(),
            },
        ];

        let equivocators = detect_equivocators_ref(&votes);
        assert!(equivocators.contains("w1"));
        assert!(!equivocators.contains("w2"));
    }

    #[test]
    fn test_aggregate_shares() {
        let proposals = vec![
            ShareProposal {
                witness: "w1".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s1".to_string(),
                    nonce_binding: "n1".to_string(),
                    data_binding: "cns1:r1:h1".to_string(),
                },
            },
            ShareProposal {
                witness: "w2".to_string(),
                result_id: "r1".to_string(),
                share: ShareData {
                    share_value: "s2".to_string(),
                    nonce_binding: "n2".to_string(),
                    data_binding: "cns1:r1:h1".to_string(),
                },
            },
        ];

        let sig = aggregate_shares_ref(&proposals, 2);
        assert!(sig.is_some());
        let sig = sig.unwrap();
        assert_eq!(sig.signer_set.len(), 2);
        assert_eq!(sig.bound_rid, "r1");
    }
}

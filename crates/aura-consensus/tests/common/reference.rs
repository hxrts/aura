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

use aura_consensus::core::{ConsensusPhase, ConsensusState, ShareProposal};
use aura_consensus::core::state::PureCommitFact;
use std::collections::BTreeSet;

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
pub fn detect_equivocators_ref(votes: &[Vote]) -> BTreeSet<String> {
    let mut equivocators = BTreeSet::new();

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
// TRANSITION REFERENCE IMPLEMENTATIONS
// ============================================================================
//
// These mirror the production transitions in transitions.rs but are written
// for maximum clarity rather than efficiency. They serve as an oracle for
// differential testing.

/// Reference result type for transitions
/// Mirrors TransitionResult but simplified
#[derive(Debug, Clone)]
pub enum TransitionResultRef {
    /// Transition succeeded
    Ok(ConsensusState),
    /// Transition was not enabled
    NotEnabled(String),
}

impl TransitionResultRef {
    /// Check if transition succeeded
    pub fn is_ok(&self) -> bool {
        matches!(self, TransitionResultRef::Ok(_))
    }

    /// Get the new state if transition succeeded
    pub fn state(self) -> Option<ConsensusState> {
        match self {
            TransitionResultRef::Ok(s) => Some(s),
            TransitionResultRef::NotEnabled(_) => None,
        }
    }

    /// Get the error message if transition failed
    pub fn error(&self) -> Option<&str> {
        match self {
            TransitionResultRef::Ok(_) => None,
            TransitionResultRef::NotEnabled(msg) => Some(msg),
        }
    }
}

/// Reference implementation of apply_share
///
/// Quint: `submitWitnessShare(cid, witness, rid, share)`
/// Lean: Follows from Agreement.lean share submission
///
/// This implementation prioritizes clarity:
/// 1. Check all preconditions explicitly
/// 2. Apply changes in obvious order
/// 3. Check threshold and commit if met
pub fn apply_share_ref(state: &ConsensusState, proposal: ShareProposal) -> TransitionResultRef {
    // Precondition 1: witness must be in witness set
    if !state.witnesses.contains(&proposal.witness) {
        return TransitionResultRef::NotEnabled(format!(
            "witness {} not in witness set",
            proposal.witness
        ));
    }

    // Precondition 2: witness must not have already voted
    let has_voted = state.proposals.iter().any(|p| p.witness == proposal.witness);
    if has_voted {
        return TransitionResultRef::NotEnabled(format!(
            "witness {} already voted",
            proposal.witness
        ));
    }

    // Precondition 3: consensus must be active
    let is_active = matches!(
        state.phase,
        ConsensusPhase::FastPathActive | ConsensusPhase::FallbackActive
    );
    if !is_active {
        return TransitionResultRef::NotEnabled("consensus not active".to_string());
    }

    // Precondition 4: witness must not be known equivocator
    if state.equivocators.contains(&proposal.witness) {
        return TransitionResultRef::NotEnabled(format!(
            "witness {} is known equivocator",
            proposal.witness
        ));
    }

    // Create new state
    let mut new_state = state.clone();

    // Check for equivocation (same witness, different result)
    // Note: This shouldn't happen since we checked has_voted above,
    // but this is the reference check for completeness
    let is_equivocating = state
        .proposals
        .iter()
        .any(|p| p.witness == proposal.witness && p.result_id != proposal.result_id);

    if is_equivocating {
        new_state.equivocators.insert(proposal.witness.clone());
    } else {
        new_state.proposals.push(proposal.clone());
    }

    // Check if threshold is met after adding proposal
    // Count proposals for each result
    let mut result_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for p in &new_state.proposals {
        *result_counts.entry(&p.result_id).or_insert(0) += 1;
    }

    // Check if any result has threshold
    let threshold_met = result_counts.values().any(|&count| count >= new_state.threshold);

    if threshold_met {
        new_state.phase = ConsensusPhase::Committed;

        // Find the winning result
        let winning_result = result_counts
            .iter()
            .find(|(_, &count)| count >= new_state.threshold)
            .map(|(&rid, _)| rid.to_string());

        if let Some(rid) = winning_result {
            new_state.commit_fact = Some(PureCommitFact {
                cid: new_state.cid.clone(),
                result_id: rid,
                signature: "ref_agg_sig".to_string(),
                prestate_hash: new_state.prestate_hash.clone(),
            });
        }
    }

    TransitionResultRef::Ok(new_state)
}

/// Reference implementation of trigger_fallback
///
/// Quint: `triggerFallback(cid)`
///
/// Simple: just change phase from FastPathActive to FallbackActive
pub fn trigger_fallback_ref(state: &ConsensusState) -> TransitionResultRef {
    // Precondition: must be in fast path
    if state.phase != ConsensusPhase::FastPathActive {
        return TransitionResultRef::NotEnabled(format!(
            "not in fast path: {:?}",
            state.phase
        ));
    }

    let mut new_state = state.clone();
    new_state.phase = ConsensusPhase::FallbackActive;
    new_state.fallback_timer_active = true;

    TransitionResultRef::Ok(new_state)
}

/// Reference implementation of fail_consensus
///
/// Quint: `failConsensus(cid)`
pub fn fail_consensus_ref(state: &ConsensusState) -> TransitionResultRef {
    // Cannot fail if already committed
    if state.phase == ConsensusPhase::Committed {
        return TransitionResultRef::NotEnabled("already committed".to_string());
    }

    // Cannot fail if already failed
    if state.phase == ConsensusPhase::Failed {
        return TransitionResultRef::NotEnabled("already failed".to_string());
    }

    let mut new_state = state.clone();
    new_state.phase = ConsensusPhase::Failed;

    TransitionResultRef::Ok(new_state)
}

/// Reference implementation of check_invariants
///
/// Quint: `WellFormedState(insts, committed, nonces, epoch)`
///
/// Returns None if valid, Some(error) if invalid
pub fn check_invariants_ref(state: &ConsensusState) -> Option<String> {
    // Invariant 1: threshold >= 1
    if state.threshold < 1 {
        return Some("threshold must be >= 1".to_string());
    }

    // Invariant 2: |witnesses| >= threshold
    if state.witnesses.len() < state.threshold {
        return Some(format!(
            "insufficient witnesses: {} < {}",
            state.witnesses.len(),
            state.threshold
        ));
    }

    // Invariant 3: all proposals from witnesses
    for proposal in &state.proposals {
        if !state.witnesses.contains(&proposal.witness) {
            return Some(format!("proposal from non-witness: {}", proposal.witness));
        }
    }

    // Invariant 4: equivocators subset of witnesses
    for eq in &state.equivocators {
        if !state.witnesses.contains(eq) {
            return Some(format!("equivocator not in witness set: {}", eq));
        }
    }

    // Invariant 5: committed phase requires commit fact
    if state.phase == ConsensusPhase::Committed && state.commit_fact.is_none() {
        return Some("committed phase but no commit fact".to_string());
    }

    // Invariant 6: no witness has multiple proposals (no duplicate entries)
    let mut seen_witnesses: BTreeSet<&str> = BTreeSet::new();
    for proposal in &state.proposals {
        if seen_witnesses.contains(proposal.witness.as_str()) {
            return Some(format!("duplicate proposal from witness: {}", proposal.witness));
        }
        seen_witnesses.insert(&proposal.witness);
    }

    None // All invariants hold
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
    use aura_consensus::core::state::ShareData;

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

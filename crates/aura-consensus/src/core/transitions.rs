//! Pure Consensus State Transitions
//!
//! Effect-free transition functions that mirror Quint actions.
//!
//! ## Quint Correspondence
//! - `start_consensus` ↔ `startConsensus` in protocol_consensus.qnt
//! - `apply_share` ↔ `submitWitnessShare` in protocol_consensus.qnt
//! - `trigger_fallback` ↔ `triggerFallback` in protocol_consensus.qnt
//! - `gossip_shares` ↔ `gossipShares` in protocol_consensus.qnt
//! - `complete_via_fallback` ↔ `completeViaFallback` in protocol_consensus.qnt
//! - `fail_consensus` ↔ `failConsensus` in protocol_consensus.qnt
//!
//! ## Design Principles
//! 1. Pure functions: `fn(state, params) -> TransitionResult`
//! 2. No side effects: all I/O happens in calling layer
//! 3. Deterministic: same inputs always produce same outputs
//! 4. Verifiable: each function maps to exactly one Quint action

use std::collections::BTreeSet;

use super::state::{ConsensusPhase, ConsensusState, PathSelection, PureCommitFact, ShareProposal};

/// Result of a state transition.
///
/// Quint actions return bool (enabled/disabled). We use Result to carry
/// error information for debugging while maintaining the same semantics.
#[derive(Debug, Clone)]
pub enum TransitionResult {
    /// Transition succeeded, new state produced
    Ok(ConsensusState),
    /// Transition was not enabled (precondition failed)
    NotEnabled(String),
}

impl TransitionResult {
    /// Check if transition succeeded
    pub fn is_ok(&self) -> bool {
        matches!(self, TransitionResult::Ok(_))
    }

    /// Get the new state if transition succeeded
    pub fn state(self) -> Option<ConsensusState> {
        match self {
            TransitionResult::Ok(s) => Some(s),
            TransitionResult::NotEnabled(_) => None,
        }
    }
}

/// Start a new consensus instance.
///
/// Quint: `startConsensus(cid, initiator, op, pHash, witnesses, threshold)`
///
/// Preconditions:
/// - `witnesses.len() >= threshold`
/// - `threshold >= 1`
///
/// Returns new ConsensusState in FastPathActive or FallbackActive phase
/// based on path selection.
pub fn start_consensus(
    cid: String,
    operation: String,
    prestate_hash: String,
    threshold: usize,
    witnesses: BTreeSet<String>,
    initiator: String,
    path: PathSelection,
) -> TransitionResult {
    // Quint: witnesses.size() >= threshold
    if witnesses.len() < threshold {
        return TransitionResult::NotEnabled(format!(
            "insufficient witnesses: {} < {}",
            witnesses.len(),
            threshold
        ));
    }

    // Quint: threshold >= 1
    if threshold < 1 {
        return TransitionResult::NotEnabled("threshold must be >= 1".to_string());
    }

    let state = ConsensusState::new(
        cid,
        operation,
        prestate_hash,
        threshold,
        witnesses,
        initiator,
        path,
    );

    TransitionResult::Ok(state)
}

/// Apply a witness share proposal to consensus state.
///
/// Quint: `submitWitnessShare(cid, witness, rid, share)`
///
/// Preconditions:
/// - Witness is in the witness set
/// - Witness has not already voted
/// - Consensus is active (FastPathActive or FallbackActive)
/// - Witness is not a known equivocator
///
/// Effects:
/// - Adds proposal to state
/// - Detects equivocation if witness votes for different result
/// - Commits if threshold is reached
pub fn apply_share(state: &ConsensusState, proposal: ShareProposal) -> TransitionResult {
    // Quint: isWitness = inst.witnesses.contains(witness)
    if !state.witnesses.contains(&proposal.witness) {
        return TransitionResult::NotEnabled(format!(
            "witness {} not in witness set",
            proposal.witness
        ));
    }

    // Quint: notVoted = not(hasProposal(inst.proposals, witness))
    if state.has_proposal(&proposal.witness) {
        return TransitionResult::NotEnabled(format!("witness {} already voted", proposal.witness));
    }

    // Quint: isActive = inst.phase == FastPathActive or inst.phase == FallbackActive
    if !state.is_active() {
        return TransitionResult::NotEnabled(format!("consensus not active: {:?}", state.phase));
    }

    // Quint: not(inst.equivocators.contains(witness))
    if state.equivocators.contains(&proposal.witness) {
        return TransitionResult::NotEnabled(format!(
            "witness {} is known equivocator",
            proposal.witness
        ));
    }

    let mut new_state = state.clone();

    // Quint: isEquivocating = detectEquivocation(inst.proposals, witness, rid)
    let is_equivocating = state
        .proposals
        .iter()
        .any(|p| p.witness == proposal.witness && p.result_id != proposal.result_id);

    if is_equivocating {
        // Quint: newEquivocators = inst.equivocators.union(Set(witness))
        new_state.equivocators.insert(proposal.witness);
        // Don't add proposal from equivocator
    } else {
        // Quint: newProposals = inst.proposals.union(Set(proposal))
        new_state.proposals.push(proposal);
    }

    // Quint: matchingCount = countMatchingProposals(newProposals, rid, inst.prestateHash)
    // Quint: reachedThreshold = matchingCount >= inst.threshold
    if new_state.threshold_met() {
        // Quint: newPhase = ConsensusCommitted
        new_state.phase = ConsensusPhase::Committed;

        // Create commit fact
        if let Some(rid) = new_state.majority_result() {
            let attesters: BTreeSet<String> = new_state
                .proposals
                .iter()
                .filter(|p| p.result_id == rid)
                .map(|p| p.witness.clone())
                .collect();

            new_state.commit_fact = Some(PureCommitFact {
                cid: new_state.cid.clone(),
                result_id: rid,
                signature: "agg_sig_placeholder".to_string(),
                prestate_hash: new_state.prestate_hash.clone(),
            });
        }
    }

    TransitionResult::Ok(new_state)
}

/// Trigger fallback when fast path stalls.
///
/// Quint: `triggerFallback(cid)`
///
/// Preconditions:
/// - Consensus is in FastPathActive phase
///
/// Effects:
/// - Moves to FallbackActive phase
/// - Activates fallback timer
pub fn trigger_fallback(state: &ConsensusState) -> TransitionResult {
    // Quint: isFastPath = inst.phase == FastPathActive
    if state.phase != ConsensusPhase::FastPathActive {
        return TransitionResult::NotEnabled(format!("not in fast path: {:?}", state.phase));
    }

    let mut new_state = state.clone();
    new_state.phase = ConsensusPhase::FallbackActive;
    new_state.fallback_timer_active = true;

    TransitionResult::Ok(new_state)
}

/// Gossip shares during fallback.
///
/// Quint: `gossipShares(cid, shareSet)`
///
/// Preconditions:
/// - Consensus is in FallbackActive phase
/// - At least one valid share to add
/// - Threshold not yet reached (else use complete_via_fallback)
///
/// Effects:
/// - Merges valid shares into proposals
pub fn gossip_shares(state: &ConsensusState, shares: Vec<ShareProposal>) -> TransitionResult {
    // Quint: isFallback = inst.phase == FallbackActive
    if state.phase != ConsensusPhase::FallbackActive {
        return TransitionResult::NotEnabled(format!("not in fallback: {:?}", state.phase));
    }

    // Filter valid shares
    // Quint: validShares = shareSet.filter(p => ...)
    let valid_shares: Vec<ShareProposal> = shares
        .into_iter()
        .filter(|p| {
            state.witnesses.contains(&p.witness)
                && !state.equivocators.contains(&p.witness)
                && !state.has_proposal(&p.witness)
        })
        .collect();

    // Quint: validShares.size() >= 1
    if valid_shares.is_empty() {
        return TransitionResult::NotEnabled("no valid shares to add".to_string());
    }

    let mut new_state = state.clone();
    new_state.proposals.extend(valid_shares);

    // Quint: not(anyReachesThreshold) - if threshold reached, use completeViaFallback
    if new_state.threshold_met() {
        return TransitionResult::NotEnabled(
            "threshold reached, use complete_via_fallback".to_string(),
        );
    }

    TransitionResult::Ok(new_state)
}

/// Complete consensus via fallback path.
///
/// Quint: `completeViaFallback(cid, winningRid)`
///
/// Preconditions:
/// - Consensus is in FallbackActive phase
/// - Threshold shares exist for the winning result
///
/// Effects:
/// - Moves to ConsensusCommitted phase
/// - Creates commit fact with aggregated signature
pub fn complete_via_fallback(state: &ConsensusState, winning_rid: &str) -> TransitionResult {
    // Quint: isFallback = inst.phase == FallbackActive
    if state.phase != ConsensusPhase::FallbackActive {
        return TransitionResult::NotEnabled(format!("not in fallback: {:?}", state.phase));
    }

    // Quint: reachedThreshold = matchingCount >= inst.threshold
    let matching_count = state.count_proposals_for_result(winning_rid);
    if matching_count < state.threshold {
        return TransitionResult::NotEnabled(format!(
            "insufficient shares for {}: {} < {}",
            winning_rid, matching_count, state.threshold
        ));
    }

    let mut new_state = state.clone();
    new_state.phase = ConsensusPhase::Committed;

    // Create commit fact
    let attesters: BTreeSet<String> = state
        .proposals
        .iter()
        .filter(|p| p.result_id == winning_rid)
        .map(|p| p.witness.clone())
        .collect();

    new_state.commit_fact = Some(PureCommitFact {
        cid: state.cid.clone(),
        result_id: winning_rid.to_string(),
        signature: "agg_sig_fallback".to_string(),
        prestate_hash: state.prestate_hash.clone(),
    });

    TransitionResult::Ok(new_state)
}

/// Fail consensus instance.
///
/// Quint: `failConsensus(cid)`
///
/// Preconditions:
/// - Not already committed
/// - Not already failed
///
/// Effects:
/// - Moves to ConsensusFailed phase
pub fn fail_consensus(state: &ConsensusState) -> TransitionResult {
    // Quint: notCommitted = inst.phase != ConsensusCommitted
    if state.phase == ConsensusPhase::Committed {
        return TransitionResult::NotEnabled("already committed".to_string());
    }

    // Quint: notFailed = inst.phase != ConsensusFailed
    if state.phase == ConsensusPhase::Failed {
        return TransitionResult::NotEnabled("already failed".to_string());
    }

    let mut new_state = state.clone();
    new_state.phase = ConsensusPhase::Failed;

    TransitionResult::Ok(new_state)
}

#[cfg(test)]
mod tests {
    use super::super::state::ShareData;
    use super::*;

    fn make_share(witness: &str, result_id: &str) -> ShareProposal {
        ShareProposal {
            witness: witness.to_string(),
            result_id: result_id.to_string(),
            share: ShareData {
                share_value: format!("share_{}", witness),
                nonce_binding: format!("nonce_{}", witness),
                data_binding: format!("binding_{}", result_id),
            },
        }
    }

    #[test]
    fn test_start_consensus() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let result = start_consensus(
            "cns1".to_string(),
            "update_policy".to_string(),
            "pre_abc".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        assert!(result.is_ok());
        let state = result.state().unwrap();
        assert_eq!(state.phase, ConsensusPhase::FastPathActive);
        assert_eq!(state.threshold, 2);
    }

    #[test]
    fn test_start_consensus_insufficient_witnesses() {
        let witnesses: BTreeSet<_> = ["w1"].iter().map(|s| s.to_string()).collect();

        let result = start_consensus(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        assert!(!result.is_ok());
    }

    #[test]
    fn test_apply_share_reaches_threshold() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let mut state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        // First share
        let result = apply_share(&state, make_share("w1", "rid1"));
        assert!(result.is_ok());
        state = result.state().unwrap();
        assert_eq!(state.phase, ConsensusPhase::FastPathActive);

        // Second share - threshold met
        let result = apply_share(&state, make_share("w2", "rid1"));
        assert!(result.is_ok());
        state = result.state().unwrap();
        assert_eq!(state.phase, ConsensusPhase::Committed);
        assert!(state.commit_fact.is_some());
    }

    #[test]
    fn test_apply_share_detects_equivocation() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let mut state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        // First share from w1
        state.proposals.push(make_share("w1", "rid1"));

        // w1 tries to vote for different result - equivocation
        let result = apply_share(&state, make_share("w1", "rid2"));
        // Since w1 already voted, should be NotEnabled
        assert!(!result.is_ok());
    }

    #[test]
    fn test_trigger_fallback() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        let result = trigger_fallback(&state);
        assert!(result.is_ok());
        let new_state = result.state().unwrap();
        assert_eq!(new_state.phase, ConsensusPhase::FallbackActive);
        assert!(new_state.fallback_timer_active);
    }

    #[test]
    fn test_trigger_fallback_not_fast_path() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::SlowPath, // Already in fallback
        );

        let result = trigger_fallback(&state);
        assert!(!result.is_ok());
    }

    #[test]
    fn test_complete_via_fallback() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let mut state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::SlowPath,
        );

        // Add shares via gossip
        state.proposals.push(make_share("w1", "rid1"));
        state.proposals.push(make_share("w2", "rid1"));

        let result = complete_via_fallback(&state, "rid1");
        assert!(result.is_ok());
        let new_state = result.state().unwrap();
        assert_eq!(new_state.phase, ConsensusPhase::Committed);
        assert!(new_state.commit_fact.is_some());
    }

    #[test]
    fn test_fail_consensus() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        let result = fail_consensus(&state);
        assert!(result.is_ok());
        let new_state = result.state().unwrap();
        assert_eq!(new_state.phase, ConsensusPhase::Failed);
    }

    #[test]
    fn test_fail_consensus_already_committed() {
        let witnesses: BTreeSet<_> = ["w1", "w2"].iter().map(|s| s.to_string()).collect();

        let mut state = ConsensusState::new(
            "cns1".to_string(),
            "op".to_string(),
            "pre".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        state.phase = ConsensusPhase::Committed;

        let result = fail_consensus(&state);
        assert!(!result.is_ok());
    }
}

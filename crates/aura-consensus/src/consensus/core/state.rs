//! Pure Consensus State Definitions
//!
//! Effect-free state structures that mirror the Quint specification.
//!
//! ## Quint Correspondence
//! - `ConsensusPhase` ↔ `ConsensusPhase` in protocol_consensus.qnt
//! - `ConsensusState` ↔ `ConsensusInstance` in protocol_consensus.qnt
//! - `ShareProposal` ↔ `ShareProposal` in protocol_consensus.qnt
//! - `ShareData` ↔ `ShareData` in protocol_consensus.qnt
//!
//! ## Lean Correspondence
//! - `ConsensusPhase` ↔ `ConsensusPhase` in Types.lean
//! - `ShareProposal` ↔ `WitnessVote` in Types.lean

// The pure consensus core uses BTreeSet for deterministic, reproducible state.
// This matches Quint's Set semantics (deterministic iteration order) and ensures
// consensus execution is fully reproducible across replicas and test runs.
// HashMap is only used for local counting operations within single functions.
use std::collections::{BTreeSet, HashMap};

/// Consensus phase matching Quint's ConsensusPhase sum type.
///
/// Quint: `type ConsensusPhase = ConsensusPending | FastPathActive | FallbackActive
///                              | ConsensusCommitted | ConsensusFailed`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConsensusPhase {
    /// Consensus not yet started
    /// Quint: ConsensusPending
    Pending,

    /// Fast path active (1 RTT with cached nonces)
    /// Quint: FastPathActive
    FastPathActive,

    /// Fallback path active (2 RTT)
    /// Quint: FallbackActive
    FallbackActive,

    /// Successfully committed
    /// Quint: ConsensusCommitted
    Committed,

    /// Failed (conflict, timeout, or insufficient participation)
    /// Quint: ConsensusFailed
    Failed,
}

/// Path selection for consensus.
///
/// Quint: `type PathSelection = FastPath | SlowPath`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSelection {
    /// Fast path (1 RTT) - all witnesses have valid cached nonces
    FastPath,
    /// Slow path (2 RTT) - fallback when nonces unavailable
    SlowPath,
}

/// Data bound to a signature share.
///
/// Quint: `type ShareData = { shareValue: ShareValue, nonceBinding: NonceCommitment, dataBinding: DataBinding }`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShareData {
    /// Abstract share value (in production, actual FROST share)
    pub share_value: String,
    /// Binding to nonce commitment
    pub nonce_binding: String,
    /// Binding to consensus data (cid, rid, prestate)
    pub data_binding: String,
}

/// A witness's share proposal.
///
/// Quint: `type ShareProposal = { witness: AuthorityId, resultId: ResultId, share: ShareData }`
/// Lean: `structure WitnessVote`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareProposal {
    /// The witness submitting the share
    pub witness: String,
    /// Result ID this share is for
    pub result_id: String,
    /// The share data
    pub share: ShareData,
}

/// Witness participation tracking for liveness analysis.
///
/// Quint: `type WitnessParticipation` in protocol_liveness_timing.qnt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitnessParticipation {
    /// Witness identifier
    pub witness: String,
    /// Whether witness is behaving honestly
    pub is_honest: bool,
    /// Whether witness is currently reachable
    pub is_online: bool,
    /// Last seen activity time
    pub last_seen: i64,
    /// Count of shares sent
    pub shares_sent: i64,
}

/// Cached nonce for pipelining optimization.
///
/// Quint: `type CachedNonce = { commitment: NonceCommitment, cachedAt: Epoch }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedNonce {
    /// Nonce commitment value
    pub commitment: String,
    /// Epoch when cached
    pub cached_at: u64,
}

/// Committed fact representing successful consensus.
///
/// Quint: `type CommitFact = { cid: ConsensusId, resultId: ResultId, signature: ThresholdSignature, prestateHash: PrestateHash }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PureCommitFact {
    /// Consensus instance ID
    pub cid: String,
    /// Result ID
    pub result_id: String,
    /// Threshold signature (abstract in pure core)
    pub signature: String,
    /// Prestate hash
    pub prestate_hash: String,
}

/// Pure consensus instance state.
///
/// This structure mirrors Quint's `ConsensusInstance` and contains all state
/// needed for consensus without any effects.
///
/// Quint: `type ConsensusInstance = { cid, operation, prestateHash, threshold, witnesses,
///                                    initiator, phase, proposals, commitFact, fallbackTimerActive, equivocators }`
#[derive(Debug, Clone)]
pub struct ConsensusState {
    /// Consensus instance identifier
    /// Quint: cid: ConsensusId
    pub cid: String,

    /// Operation being agreed upon
    /// Quint: operation: OperationData
    pub operation: String,

    /// Hash of prestate this operation is bound to
    /// Quint: prestateHash: PrestateHash
    pub prestate_hash: String,

    /// Required threshold for agreement
    /// Quint: threshold: int
    pub threshold: usize,

    /// Set of eligible witnesses
    /// Quint: witnesses: Set[AuthorityId]
    pub witnesses: BTreeSet<String>,

    /// Initiator of this consensus instance
    /// Quint: initiator: AuthorityId
    pub initiator: String,

    /// Current phase
    /// Quint: phase: ConsensusPhase
    pub phase: ConsensusPhase,

    /// Collected share proposals
    /// Quint: proposals: Set[ShareProposal]
    pub proposals: Vec<ShareProposal>,

    /// Commit fact if consensus succeeded
    /// Quint: commitFact: Option[CommitFact]
    pub commit_fact: Option<PureCommitFact>,

    /// Whether fallback timer is active
    /// Quint: fallbackTimerActive: bool
    pub fallback_timer_active: bool,

    /// Set of detected equivocators
    /// Quint: equivocators: Set[AuthorityId]
    pub equivocators: BTreeSet<String>,
}

impl ConsensusState {
    /// Create a new pending consensus instance.
    ///
    /// Quint: startConsensus action initialization
    pub fn new(
        cid: String,
        operation: String,
        prestate_hash: String,
        threshold: usize,
        witnesses: BTreeSet<String>,
        initiator: String,
        path: PathSelection,
    ) -> Self {
        let phase = match path {
            PathSelection::FastPath => ConsensusPhase::FastPathActive,
            PathSelection::SlowPath => ConsensusPhase::FallbackActive,
        };

        Self {
            cid,
            operation,
            prestate_hash,
            threshold,
            witnesses,
            initiator,
            phase,
            proposals: Vec::new(),
            commit_fact: None,
            fallback_timer_active: path == PathSelection::SlowPath,
            equivocators: BTreeSet::new(),
        }
    }

    /// Check if a witness has already submitted a proposal.
    ///
    /// Quint: hasProposal(proposals, witness)
    pub fn has_proposal(&self, witness: &str) -> bool {
        self.proposals.iter().any(|p| p.witness == witness)
    }

    /// Count proposals for a specific result ID.
    ///
    /// Quint: countProposalsForResult(proposals, rid)
    pub fn count_proposals_for_result(&self, result_id: &str) -> usize {
        self.proposals
            .iter()
            .filter(|p| p.result_id == result_id)
            .count()
    }

    /// Check if threshold is met for any result.
    ///
    /// Quint: part of canCommit predicate
    pub fn threshold_met(&self) -> bool {
        let mut result_counts: HashMap<&str, usize> = HashMap::new();
        for proposal in &self.proposals {
            *result_counts.entry(&proposal.result_id).or_insert(0) += 1;
        }
        result_counts.values().any(|&count| count >= self.threshold)
    }

    /// Get the result ID with the most proposals.
    pub fn majority_result(&self) -> Option<String> {
        let mut result_counts: HashMap<&str, usize> = HashMap::new();
        for proposal in &self.proposals {
            *result_counts.entry(&proposal.result_id).or_insert(0) += 1;
        }
        result_counts
            .into_iter()
            .filter(|&(_, count)| count >= self.threshold)
            .max_by_key(|&(_, count)| count)
            .map(|(rid, _)| rid.to_string())
    }

    /// Check if consensus is in a terminal state.
    ///
    /// Quint: isTerminated(inst) in protocol_liveness_properties.qnt
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            ConsensusPhase::Committed | ConsensusPhase::Failed
        )
    }

    /// Check if consensus is active (can make progress).
    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            ConsensusPhase::FastPathActive | ConsensusPhase::FallbackActive
        )
    }
}

/// Global consensus state tracking multiple instances.
///
/// Quint: `var instances: ConsensusId -> ConsensusInstance`
#[derive(Debug, Clone, Default)]
pub struct GlobalConsensusState {
    /// Active consensus instances
    pub instances: HashMap<String, ConsensusState>,

    /// Committed facts (immutable once added)
    pub committed_facts: HashMap<String, PureCommitFact>,

    /// Global set of witnesses
    pub global_witnesses: BTreeSet<String>,

    /// Current epoch for nonce validity
    pub current_epoch: u64,

    /// Cached nonces per witness
    pub witness_nonces: HashMap<String, Option<CachedNonce>>,
}

impl GlobalConsensusState {
    /// Create a new empty global state.
    pub fn new(witnesses: BTreeSet<String>, epoch: u64) -> Self {
        let witness_nonces = witnesses
            .iter()
            .map(|w| (w.clone(), None))
            .collect();

        Self {
            instances: HashMap::new(),
            committed_facts: HashMap::new(),
            global_witnesses: witnesses,
            current_epoch: epoch,
            witness_nonces,
        }
    }

    /// Check if a nonce is valid for the current epoch.
    ///
    /// Quint: isNonceValid(nonce, epoch, validityWindow)
    pub fn is_nonce_valid(&self, nonce: &Option<CachedNonce>, validity_window: u64) -> bool {
        match nonce {
            Some(n) => {
                self.current_epoch >= n.cached_at
                    && self.current_epoch - n.cached_at < validity_window
            }
            None => false,
        }
    }

    /// Select path based on nonce availability.
    ///
    /// Quint: selectPath(witnesses, nonces, epoch, validityWindow)
    pub fn select_path(&self, witnesses: &BTreeSet<String>, validity_window: u64) -> PathSelection {
        let all_valid = witnesses.iter().all(|w| {
            self.witness_nonces
                .get(w)
                .map(|n| self.is_nonce_valid(n, validity_window))
                .unwrap_or(false)
        });

        if all_valid {
            PathSelection::FastPath
        } else {
            PathSelection::SlowPath
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_phase_equality() {
        assert_eq!(ConsensusPhase::Pending, ConsensusPhase::Pending);
        assert_ne!(ConsensusPhase::Pending, ConsensusPhase::FastPathActive);
    }

    #[test]
    fn test_consensus_state_new() {
        let witnesses: BTreeSet<_> = ["w1", "w2", "w3"].iter().map(|s| s.to_string()).collect();

        let state = ConsensusState::new(
            "cns1".to_string(),
            "update_policy".to_string(),
            "pre_abc".to_string(),
            2,
            witnesses,
            "w1".to_string(),
            PathSelection::FastPath,
        );

        assert_eq!(state.phase, ConsensusPhase::FastPathActive);
        assert!(!state.fallback_timer_active);
        assert!(state.proposals.is_empty());
    }

    #[test]
    fn test_has_proposal() {
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

        assert!(!state.has_proposal("w1"));

        state.proposals.push(ShareProposal {
            witness: "w1".to_string(),
            result_id: "rid1".to_string(),
            share: ShareData {
                share_value: "share".to_string(),
                nonce_binding: "nonce".to_string(),
                data_binding: "data".to_string(),
            },
        });

        assert!(state.has_proposal("w1"));
        assert!(!state.has_proposal("w2"));
    }

    #[test]
    fn test_threshold_met() {
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

        assert!(!state.threshold_met());

        // Add first proposal
        state.proposals.push(ShareProposal {
            witness: "w1".to_string(),
            result_id: "rid1".to_string(),
            share: ShareData {
                share_value: "s1".to_string(),
                nonce_binding: "n1".to_string(),
                data_binding: "d1".to_string(),
            },
        });
        assert!(!state.threshold_met());

        // Add second proposal with same result_id - now threshold met
        state.proposals.push(ShareProposal {
            witness: "w2".to_string(),
            result_id: "rid1".to_string(),
            share: ShareData {
                share_value: "s2".to_string(),
                nonce_binding: "n2".to_string(),
                data_binding: "d2".to_string(),
            },
        });
        assert!(state.threshold_met());
    }

    #[test]
    fn test_is_terminal() {
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

        assert!(!state.is_terminal());

        state.phase = ConsensusPhase::Committed;
        assert!(state.is_terminal());

        state.phase = ConsensusPhase::Failed;
        assert!(state.is_terminal());
    }
}

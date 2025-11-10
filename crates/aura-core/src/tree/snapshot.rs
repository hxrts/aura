use super::{Epoch, LeafId, NodeIndex, Policy, TreeHash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Compact snapshot of tree state at a specific epoch
///
/// Snapshots allow pruning OpLog history while preserving the ability to merge
/// future operations. They capture the essential state needed to verify and
/// apply subsequent operations.
///
/// # Invariants
///
/// - All snapshots are immutable once created
/// - Snapshots can only be created at epoch boundaries
/// - Snapshots must include complete roster and policy information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Epoch at which snapshot was taken
    pub epoch: Epoch,

    /// Root tree commitment at snapshot epoch
    pub commitment: TreeHash32,

    /// Set of leaf identifiers in tree at snapshot
    pub roster: Vec<LeafId>,

    /// Policies for all nodes at snapshot
    pub policies: BTreeMap<NodeIndex, Policy>,

    /// Optional reference to full tree state blob
    ///
    /// If present, contains CID of serialized full TreeState.
    /// Used for faster restoration but not required for correctness.
    pub state_cid: Option<TreeHash32>,

    /// Snapshot creation timestamp (monotonic)
    pub timestamp: u64,

    /// Snapshot version for forward compatibility
    pub version: u8,
}

impl Snapshot {
    /// Create a new snapshot
    pub fn new(
        epoch: Epoch,
        commitment: TreeHash32,
        roster: Vec<LeafId>,
        policies: BTreeMap<NodeIndex, Policy>,
        timestamp: u64,
    ) -> Self {
        Self {
            epoch,
            commitment,
            roster,
            policies,
            state_cid: None,
            timestamp,
            version: 1,
        }
    }

    /// Create snapshot with optional state blob CID
    pub fn with_state_cid(mut self, state_cid: TreeHash32) -> Self {
        self.state_cid = Some(state_cid);
        self
    }

    /// Get roster size
    pub fn roster_size(&self) -> usize {
        self.roster.len()
    }

    /// Check if leaf is in snapshot roster
    pub fn contains_leaf(&self, leaf_id: &LeafId) -> bool {
        self.roster.contains(leaf_id)
    }

    /// Get policy for node
    pub fn get_policy(&self, node: &NodeIndex) -> Option<&Policy> {
        self.policies.get(node)
    }

    /// Verify snapshot is well-formed
    pub fn validate(&self) -> Result<(), SnapshotError> {
        // Check roster is non-empty
        if self.roster.is_empty() {
            return Err(SnapshotError::EmptyRoster);
        }

        // Check policies is non-empty
        if self.policies.is_empty() {
            return Err(SnapshotError::EmptyPolicies);
        }

        // Check version is supported
        if self.version == 0 || self.version > 1 {
            return Err(SnapshotError::UnsupportedVersion(self.version));
        }

        Ok(())
    }
}

/// Proposal for creating a snapshot at a specific cut point
///
/// The cut defines which operations should be included in the snapshot
/// and which should remain in the active OpLog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cut {
    /// Epoch at which to take snapshot
    pub epoch: Epoch,

    /// Commitment at cut point
    pub commitment: TreeHash32,

    /// Operations to include in snapshot (all ops up to this CID)
    pub cut_cid: TreeHash32,

    /// Proposer's device ID
    pub proposer: LeafId,

    /// Proposal timestamp
    pub timestamp: u64,
}

impl Cut {
    /// Create a new cut proposal
    pub fn new(
        epoch: Epoch,
        commitment: TreeHash32,
        cut_cid: TreeHash32,
        proposer: LeafId,
    ) -> Self {
        Self {
            epoch,
            commitment,
            cut_cid,
            proposer,
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp() as u64,
        }
    }
}

/// Unique identifier for a snapshot proposal
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProposalId(pub TreeHash32);

impl ProposalId {
    /// Create proposal ID from cut
    pub fn from_cut(cut: &Cut) -> Self {
        // In real implementation, hash the cut
        Self(cut.commitment)
    }

    /// Create a new random proposal ID (for testing)
    pub fn new_random() -> Self {
        Self([0u8; 32])
    }
}

/// Partial signature/approval for a snapshot proposal
///
/// Used in threshold approval ceremony for snapshot creation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partial {
    /// Proposal being approved
    pub proposal_id: ProposalId,

    /// Signer's leaf ID
    pub signer: LeafId,

    /// Partial signature (FROST signature share)
    pub signature: Vec<u8>,

    /// Timestamp of approval
    pub timestamp: u64,
}

impl Partial {
    /// Create a new partial approval
    pub fn new(proposal_id: ProposalId, signer: LeafId, signature: Vec<u8>) -> Self {
        Self {
            proposal_id,
            signer,
            signature,
            timestamp: time::OffsetDateTime::now_utc().unix_timestamp() as u64,
        }
    }
}

/// Errors that can occur during snapshot operations
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    /// Snapshot roster is empty
    #[error("Snapshot roster cannot be empty")]
    EmptyRoster,

    /// Snapshot policies is empty
    #[error("Snapshot policies cannot be empty")]
    EmptyPolicies,

    /// Unsupported snapshot version
    #[error("Unsupported snapshot version: {0}")]
    UnsupportedVersion(u8),

    /// Invalid cut point
    #[error("Invalid cut point: {0}")]
    InvalidCut(String),

    /// Insufficient approvals for snapshot
    #[error("Insufficient approvals: got {got}, need {need}")]
    InsufficientApprovals {
        /// Number of approvals received
        got: usize,
        /// Number of approvals needed
        need: usize,
    },

    /// Snapshot verification failed
    #[error("Snapshot verification failed: {0}")]
    VerificationFailed(String),

    /// Incompatible snapshot (different tree)
    #[error("Incompatible snapshot - cannot apply")]
    Incompatible,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_creation() {
        let snapshot = Snapshot::new(
            5,
            [1u8; 32],
            vec![LeafId(1), LeafId(2)],
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        );

        assert_eq!(snapshot.epoch, 5);
        assert_eq!(snapshot.roster_size(), 2);
        assert_eq!(snapshot.version, 1);
        assert!(snapshot.state_cid.is_none());
    }

    #[test]
    fn test_snapshot_with_state_cid() {
        let snapshot = Snapshot::new(
            5,
            [1u8; 32],
            vec![LeafId(1)],
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        )
        .with_state_cid([2u8; 32]);

        assert_eq!(snapshot.state_cid, Some([2u8; 32]));
    }

    #[test]
    fn test_snapshot_contains_leaf() {
        let snapshot = Snapshot::new(
            5,
            [1u8; 32],
            vec![LeafId(1), LeafId(2), LeafId(3)],
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        );

        assert!(snapshot.contains_leaf(&LeafId(1)));
        assert!(snapshot.contains_leaf(&LeafId(2)));
        assert!(!snapshot.contains_leaf(&LeafId(99)));
    }

    #[test]
    fn test_snapshot_get_policy() {
        let mut policies = BTreeMap::new();
        policies.insert(NodeIndex(0), Policy::Any);
        policies.insert(NodeIndex(1), Policy::Threshold { m: 2, n: 3 });

        let snapshot = Snapshot::new(5, [1u8; 32], vec![LeafId(1)], policies, 1000);

        assert_eq!(snapshot.get_policy(&NodeIndex(0)), Some(&Policy::Any));
        assert_eq!(
            snapshot.get_policy(&NodeIndex(1)),
            Some(&Policy::Threshold { m: 2, n: 3 })
        );
        assert_eq!(snapshot.get_policy(&NodeIndex(99)), None);
    }

    #[test]
    fn test_snapshot_validate_success() {
        let snapshot = Snapshot::new(
            5,
            [1u8; 32],
            vec![LeafId(1)],
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        );

        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn test_snapshot_validate_empty_roster() {
        let snapshot = Snapshot::new(
            5,
            [1u8; 32],
            vec![],
            BTreeMap::from([(NodeIndex(0), Policy::Any)]),
            1000,
        );

        assert_eq!(snapshot.validate(), Err(SnapshotError::EmptyRoster));
    }

    #[test]
    fn test_snapshot_validate_empty_policies() {
        let snapshot = Snapshot::new(5, [1u8; 32], vec![LeafId(1)], BTreeMap::new(), 1000);

        assert_eq!(snapshot.validate(), Err(SnapshotError::EmptyPolicies));
    }

    #[test]
    fn test_cut_creation() {
        let cut = Cut::new(10, [1u8; 32], [2u8; 32], LeafId(1));

        assert_eq!(cut.epoch, 10);
        assert_eq!(cut.commitment, [1u8; 32]);
        assert_eq!(cut.cut_cid, [2u8; 32]);
        assert_eq!(cut.proposer, LeafId(1));
    }

    #[test]
    fn test_proposal_id_from_cut() {
        let cut = Cut::new(10, [1u8; 32], [2u8; 32], LeafId(1));
        let proposal_id = ProposalId::from_cut(&cut);

        assert_eq!(proposal_id.0, [1u8; 32]);
    }

    #[test]
    fn test_partial_creation() {
        let proposal_id = ProposalId([1u8; 32]);
        let partial = Partial::new(proposal_id, LeafId(2), vec![1, 2, 3]);

        assert_eq!(partial.proposal_id, proposal_id);
        assert_eq!(partial.signer, LeafId(2));
        assert_eq!(partial.signature, vec![1, 2, 3]);
    }
}

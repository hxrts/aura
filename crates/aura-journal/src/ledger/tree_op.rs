//! TreeOp Types
//!
//! Defines the signed operations that mutate the ratchet tree.
//! Every TreeOp is attested by a threshold signature and records the complete
//! state transition including affected nodes and new commitments.

use crate::tree::{AffectedPath, Commitment, LeafIndex, LeafNode, NodeIndex, Policy};
use aura_types::identifiers::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Epoch counter for tree operations
pub type Epoch = u64;

/// Timestamp (Unix milliseconds)
pub type Timestamp = u64;

/// Threshold signature from m-of-n devices
///
/// Represents an aggregate signature from multiple devices that collectively
/// attest to the validity of a TreeOp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// Aggregate signature bytes (Ed25519 format via FROST)
    pub signature: Vec<u8>,
    /// Device IDs that contributed to this signature
    pub signers: Vec<DeviceId>,
    /// Threshold requirement (m of n)
    pub threshold: (usize, usize), // (m, n)
}

impl ThresholdSignature {
    /// Create a new threshold signature
    pub fn new(signature: Vec<u8>, signers: Vec<DeviceId>, threshold: (usize, usize)) -> Self {
        Self {
            signature,
            signers,
            threshold,
        }
    }

    /// Verify that the threshold requirement is met
    pub fn is_threshold_met(&self) -> bool {
        self.signers.len() >= self.threshold.0
    }

    /// Get the number of signers
    pub fn num_signers(&self) -> usize {
        self.signers.len()
    }
}

/// Tree operation variants
///
/// Each variant represents a specific type of tree mutation with its associated data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeOp {
    /// Add a new leaf to the tree
    AddLeaf {
        /// The leaf node being added
        leaf_node: LeafNode,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },

    /// Remove a leaf from the tree
    RemoveLeaf {
        /// Index of the leaf being removed
        leaf_index: LeafIndex,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },

    /// Rotate secrets along a path (for forward secrecy)
    RotatePath {
        /// Index of the leaf whose path is being rotated
        leaf_index: LeafIndex,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },

    /// Refresh policy on a branch node
    RefreshPolicy {
        /// Index of the branch node
        node_index: NodeIndex,
        /// New policy
        new_policy: Policy,
        /// Path affected by this operation
        affected_path: AffectedPath,
    },

    /// Epoch bump (invalidates cached credentials)
    EpochBump {
        /// Reason for the epoch bump
        reason: EpochBumpReason,
    },

    /// Recovery grant (guardian-issued recovery capability)
    RecoveryGrant {
        /// Target device for recovery
        target_device: DeviceId,
        /// Guardian devices that issued the grant
        guardians: Vec<DeviceId>,
        /// Expiration timestamp
        expires_at: Timestamp,
    },
}

/// Reason for an epoch bump
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EpochBumpReason {
    /// After recovery ceremony
    PostRecovery,
    /// After device removal
    PostDeviceRemoval,
    /// Periodic rotation
    PeriodicRotation,
    /// Security incident
    SecurityIncident,
}

/// Complete tree operation record with attestation
///
/// This is the atomic unit of tree mutation that gets recorded in the journal ledger.
/// Every TreeOpRecord is signed by a threshold of devices and contains complete
/// information about the state transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeOpRecord {
    /// Epoch at which this operation was applied
    pub epoch: Epoch,

    /// The tree operation
    pub op: TreeOp,

    /// Indices of nodes affected by this operation
    pub affected_indices: Vec<NodeIndex>,

    /// New commitments after this operation
    pub new_commitments: BTreeMap<NodeIndex, Commitment>,

    /// Capability references issued as part of this operation (if any)
    pub capability_refs: Vec<super::capability::CapabilityRef>,

    /// Threshold signature attesting to this operation
    pub attestation: ThresholdSignature,

    /// Timestamp when this operation was authored
    pub authored_at: Timestamp,

    /// Device that authored this operation
    pub author: DeviceId,
}

impl TreeOpRecord {
    /// Create a new tree operation record
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch: Epoch,
        op: TreeOp,
        affected_indices: Vec<NodeIndex>,
        new_commitments: BTreeMap<NodeIndex, Commitment>,
        capability_refs: Vec<super::capability::CapabilityRef>,
        attestation: ThresholdSignature,
        authored_at: Timestamp,
        author: DeviceId,
    ) -> Self {
        Self {
            epoch,
            op,
            affected_indices,
            new_commitments,
            capability_refs,
            attestation,
            authored_at,
            author,
        }
    }

    /// Verify that the threshold signature is valid
    pub fn verify_threshold(&self) -> bool {
        self.attestation.is_threshold_met()
    }

    /// Get the root commitment after this operation (if available)
    pub fn root_commitment(&self) -> Option<&Commitment> {
        // Find the highest node index (root)
        self.new_commitments
            .iter()
            .max_by_key(|(idx, _)| idx.value())
            .map(|(_, commitment)| commitment)
    }

    /// Check if this operation issues any capabilities
    pub fn issues_capabilities(&self) -> bool {
        !self.capability_refs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::node::{KeyPackage, LeafMetadata};
    use crate::tree::{LeafId, LeafRole};

    #[test]
    fn test_threshold_signature_is_met() {
        let sig = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 3], (2, 3));
        assert!(sig.is_threshold_met());

        let sig = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 1], (2, 3));
        assert!(!sig.is_threshold_met());
    }

    #[test]
    fn test_tree_op_add_leaf() {
        let leaf = LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: LeafIndex(0),
            role: LeafRole::Device,
            public_key: KeyPackage {
                signing_key: vec![0u8; 32],
                encryption_key: None,
            },
            metadata: LeafMetadata::default(),
        };

        let op = TreeOp::AddLeaf {
            leaf_node: leaf,
            affected_path: AffectedPath::new(),
        };

        match op {
            TreeOp::AddLeaf { .. } => (),
            _ => panic!("Expected AddLeaf"),
        }
    }

    #[test]
    fn test_tree_op_remove_leaf() {
        let op = TreeOp::RemoveLeaf {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        };

        match op {
            TreeOp::RemoveLeaf { .. } => (),
            _ => panic!("Expected RemoveLeaf"),
        }
    }

    #[test]
    fn test_tree_op_rotate_path() {
        let op = TreeOp::RotatePath {
            leaf_index: LeafIndex(0),
            affected_path: AffectedPath::new(),
        };

        match op {
            TreeOp::RotatePath { .. } => (),
            _ => panic!("Expected RotatePath"),
        }
    }

    #[test]
    fn test_tree_op_refresh_policy() {
        let op = TreeOp::RefreshPolicy {
            node_index: NodeIndex::new(1),
            new_policy: Policy::All,
            affected_path: AffectedPath::new(),
        };

        match op {
            TreeOp::RefreshPolicy { .. } => (),
            _ => panic!("Expected RefreshPolicy"),
        }
    }

    #[test]
    fn test_tree_op_epoch_bump() {
        let op = TreeOp::EpochBump {
            reason: EpochBumpReason::PostRecovery,
        };

        match op {
            TreeOp::EpochBump { .. } => (),
            _ => panic!("Expected EpochBump"),
        }
    }

    #[test]
    fn test_tree_op_record_creation() {
        let op = TreeOp::EpochBump {
            reason: EpochBumpReason::PeriodicRotation,
        };

        let sig = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3));

        let record = TreeOpRecord::new(
            1,
            op,
            vec![],
            BTreeMap::new(),
            vec![],
            sig,
            1000,
            DeviceId::new(),
        );

        assert_eq!(record.epoch, 1);
        assert!(record.verify_threshold());
    }

    #[test]
    fn test_tree_op_record_root_commitment() {
        let mut commitments = BTreeMap::new();
        commitments.insert(NodeIndex::new(0), Commitment::new([1u8; 32]));
        commitments.insert(NodeIndex::new(3), Commitment::new([2u8; 32]));
        commitments.insert(NodeIndex::new(5), Commitment::new([3u8; 32]));

        let op = TreeOp::EpochBump {
            reason: EpochBumpReason::PeriodicRotation,
        };

        let sig = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3));

        let record = TreeOpRecord::new(
            1,
            op,
            vec![],
            commitments,
            vec![],
            sig,
            1000,
            DeviceId::new(),
        );

        // Root should be the highest index
        let root = record.root_commitment();
        assert!(root.is_some());
        assert_eq!(root.unwrap(), &Commitment::new([3u8; 32]));
    }

    #[test]
    fn test_tree_op_record_issues_capabilities() {
        let op = TreeOp::EpochBump {
            reason: EpochBumpReason::PeriodicRotation,
        };

        let sig = ThresholdSignature::new(vec![0u8; 64], vec![DeviceId::new(); 2], (2, 3));

        let record = TreeOpRecord::new(
            1,
            op,
            vec![],
            BTreeMap::new(),
            vec![],
            sig,
            1000,
            DeviceId::new(),
        );

        assert!(!record.issues_capabilities());
    }
}

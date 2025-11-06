//! Tree Node Types
//!
//! Defines the fundamental node types for the ratchet tree:
//! - LeafNode: Devices and Guardians
//! - BranchNode: Interior nodes carrying policies

use aura_types::identifiers::{DeviceId, GuardianId};
use crate::tree::commitment::Commitment;
use crate::tree::indexing::{LeafIndex, NodeIndex};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Leaf node role - distinguishes devices from guardians
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LeafRole {
    /// Device (phone, laptop, tablet)
    Device,
    /// Guardian (trusted contact for recovery)
    Guardian,
}

impl fmt::Display for LeafRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeafRole::Device => write!(f, "device"),
            LeafRole::Guardian => write!(f, "guardian"),
        }
    }
}

/// Metadata for a leaf node
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafMetadata {
    /// Display name (e.g., "Alice's Laptop")
    pub display_name: String,
    /// Platform hint (e.g., "iOS", "Android", "Linux")
    pub platform: Option<String>,
    /// Additional metadata
    pub extra: std::collections::BTreeMap<String, String>,
}

impl Default for LeafMetadata {
    fn default() -> Self {
        Self {
            display_name: String::from("Unknown Device"),
            platform: None,
            extra: std::collections::BTreeMap::new(),
        }
    }
}

/// Unique identifier for a leaf node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LeafId(pub uuid::Uuid);

impl LeafId {
    /// Create a new random leaf ID
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }

    /// Create a leaf ID from a UUID
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for LeafId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for LeafId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "leaf-{}", self.0)
    }
}

/// Public key package for a leaf node
///
/// This represents the cryptographic identity of a device or guardian.
/// For devices, this is typically an Ed25519 signing key.
/// For guardians, this may include additional key material for recovery protocols.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyPackage {
    /// Public signing key (Ed25519)
    pub signing_key: Vec<u8>, // 32 bytes for Ed25519
    /// Optional encryption key (for E2E messaging)
    pub encryption_key: Option<Vec<u8>>,
}

/// Leaf node in the ratchet tree
///
/// Represents a device or guardian with fixed semantics.
/// Leaves inherit policy from their ancestor branch path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafNode {
    /// Stable identifier assigned via LBBT rules
    pub leaf_id: LeafId,
    /// Leaf index in the tree (determines NodeIndex)
    pub leaf_index: LeafIndex,
    /// Role (Device or Guardian)
    pub role: LeafRole,
    /// Public key package
    pub public_key: KeyPackage,
    /// Display name and metadata
    pub metadata: LeafMetadata,
}

impl LeafNode {
    /// Create a new device leaf node
    pub fn new_device(
        leaf_id: LeafId,
        leaf_index: LeafIndex,
        public_key: KeyPackage,
        metadata: LeafMetadata,
    ) -> Self {
        Self {
            leaf_id,
            leaf_index,
            role: LeafRole::Device,
            public_key,
            metadata,
        }
    }

    /// Create a new guardian leaf node
    pub fn new_guardian(
        leaf_id: LeafId,
        leaf_index: LeafIndex,
        public_key: KeyPackage,
        metadata: LeafMetadata,
    ) -> Self {
        Self {
            leaf_id,
            leaf_index,
            role: LeafRole::Guardian,
            public_key,
            metadata,
        }
    }

    /// Get the device ID if this is a device leaf
    pub fn device_id(&self) -> Option<DeviceId> {
        match self.role {
            LeafRole::Device => Some(DeviceId(self.leaf_id.0)),
            LeafRole::Guardian => None,
        }
    }

    /// Get the guardian ID if this is a guardian leaf
    pub fn guardian_id(&self) -> Option<GuardianId> {
        match self.role {
            LeafRole::Guardian => Some(GuardianId(self.leaf_id.0)),
            LeafRole::Device => None,
        }
    }
}

/// Policy for a branch node
///
/// Defines the threshold requirement for operations involving this branch's subtree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Policy {
    /// All children must participate
    #[default]
    All,
    /// Any single child can participate
    Any,
    /// Threshold: m of n children must participate
    Threshold {
        /// Minimum number of participants (m)
        m: usize,
        /// Total number of participants (n)
        n: usize,
    },
}

impl Policy {
    /// Create a threshold policy
    pub fn threshold(m: usize, n: usize) -> Self {
        assert!(m > 0, "Threshold m must be positive");
        assert!(m <= n, "Threshold m must not exceed n");
        Self::Threshold { m, n }
    }

    /// Get the minimum number of participants required
    pub fn required_participants(&self, total_children: usize) -> usize {
        match self {
            Policy::All => total_children,
            Policy::Any => 1,
            Policy::Threshold { m, .. } => *m,
        }
    }

    /// Check if a policy is satisfied by the given number of participants
    pub fn is_satisfied(&self, participants: usize, total_children: usize) -> bool {
        participants >= self.required_participants(total_children)
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Policy::All => write!(f, "All"),
            Policy::Any => write!(f, "Any"),
            Policy::Threshold { m, n } => write!(f, "Threshold({}/{})", m, n),
        }
    }
}

/// Branch node in the ratchet tree
///
/// Interior nodes that carry policies and commitments.
/// Branches define threshold requirements for their subtree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchNode {
    /// Node index (implicit from tree position, stored for verification)
    pub node_index: NodeIndex,
    /// Policy for this branch
    pub policy: Policy,
    /// Commitment hash binding structure and content
    pub commitment: Commitment,
}

impl BranchNode {
    /// Create a new branch node
    pub fn new(node_index: NodeIndex, policy: Policy, commitment: Commitment) -> Self {
        Self {
            node_index,
            policy,
            commitment,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_role_display() {
        assert_eq!(LeafRole::Device.to_string(), "device");
        assert_eq!(LeafRole::Guardian.to_string(), "guardian");
    }

    #[test]
    fn test_policy_threshold() {
        let policy = Policy::threshold(2, 3);
        assert_eq!(policy.required_participants(3), 2);
        assert!(policy.is_satisfied(2, 3));
        assert!(policy.is_satisfied(3, 3));
        assert!(!policy.is_satisfied(1, 3));
    }

    #[test]
    fn test_policy_all() {
        let policy = Policy::All;
        assert_eq!(policy.required_participants(5), 5);
        assert!(policy.is_satisfied(5, 5));
        assert!(!policy.is_satisfied(4, 5));
    }

    #[test]
    fn test_policy_any() {
        let policy = Policy::Any;
        assert_eq!(policy.required_participants(5), 1);
        assert!(policy.is_satisfied(1, 5));
        assert!(policy.is_satisfied(5, 5));
    }

    #[test]
    #[should_panic(expected = "Threshold m must be positive")]
    fn test_policy_threshold_zero() {
        Policy::threshold(0, 3);
    }

    #[test]
    #[should_panic(expected = "Threshold m must not exceed n")]
    fn test_policy_threshold_invalid() {
        Policy::threshold(4, 3);
    }

    #[test]
    fn test_leaf_node_device() {
        let leaf_id = LeafId::new();
        let leaf_index = LeafIndex(0);
        let key_package = KeyPackage {
            signing_key: vec![0u8; 32],
            encryption_key: None,
        };
        let metadata = LeafMetadata::default();

        let leaf = LeafNode::new_device(leaf_id, leaf_index, key_package, metadata);
        assert_eq!(leaf.role, LeafRole::Device);
        assert!(leaf.device_id().is_some());
        assert!(leaf.guardian_id().is_none());
    }

    #[test]
    fn test_leaf_node_guardian() {
        let leaf_id = LeafId::new();
        let leaf_index = LeafIndex(0);
        let key_package = KeyPackage {
            signing_key: vec![0u8; 32],
            encryption_key: None,
        };
        let metadata = LeafMetadata::default();

        let leaf = LeafNode::new_guardian(leaf_id, leaf_index, key_package, metadata);
        assert_eq!(leaf.role, LeafRole::Guardian);
        assert!(leaf.guardian_id().is_some());
        assert!(leaf.device_id().is_none());
    }
}

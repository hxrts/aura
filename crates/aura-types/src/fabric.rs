//! KeyFabric Core Types
//!
//! This module defines the core types for KeyFabric's unified threshold and membership system.
//!
//! KeyFabric unifies threshold cryptography and group membership into a single CRDT-based
//! graph structure where threshold policies are structural properties of the graph itself.
//!
//! ## Core Concepts
//!
//! - **Structure = Security**: If graph topology is valid, cryptographic state is valid
//! - **Policy as Topology**: Threshold policies are node properties, not external config
//! - **CRDT-Native**: All operations merge deterministically via Automerge
//! - **Composable**: Identity, group, recovery use same primitives
//!
//! ## Architecture
//!
//! ```text
//! Graph Node = {
//!     identity: NodeId
//!     kind: Device | Identity | Group | Guardian
//!     policy: All | Any | Threshold{m,n}
//!     encrypted_secret: AEAD(node_secret, parent_KEK)
//!     share_headers: Vec<ShareCommitment>
//!     epoch: u64  // rotation counter
//!     messaging_key: Option<AEAD(group_messaging_key)>  // for Groups only
//! }
//!
//! Graph Edge = {
//!     from: NodeId → to: NodeId
//!     kind: Contains | GrantsCapability
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Unique identifier for a fabric node
pub type NodeId = crate::identifiers::DeviceId; // Reuse DeviceId infrastructure

/// Unique identifier for a fabric edge
pub type EdgeId = uuid::Uuid;

/// Reference to a fabric resource for capability binding
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceRef {
    /// Reference to a node
    Node(NodeId),
    /// Reference to an edge
    Edge(EdgeId),
    /// Reference to entire fabric
    Fabric,
}

impl ResourceRef {
    /// Get the resource type as a string
    pub fn resource_type(&self) -> String {
        match self {
            ResourceRef::Node(id) => format!("fabric://node/{}", id),
            ResourceRef::Edge(id) => format!("fabric://edge/{}", id),
            ResourceRef::Fabric => "fabric://".to_string(),
        }
    }
}

/// Node kinds determine semantics and capabilities
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    /// Physical device with private key
    Device,

    /// Threshold identity (M-of-N devices)
    Identity,

    /// Private group with messaging capabilities
    Group,

    /// Guardian for social recovery
    Guardian,
}

/// Node policies define how the node's secret can be unwrapped
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodePolicy {
    /// All children must participate (AND)
    All,

    /// Any one child can participate (OR)
    Any,

    /// M-of-N threshold requirement
    Threshold {
        /// Number of required participants (m)
        m: u8,
        /// Total number of participants (n)
        n: u8,
    },
}

impl NodePolicy {
    /// Check if this policy is valid (m <= n for threshold)
    pub fn is_valid(&self) -> bool {
        match self {
            NodePolicy::Threshold { m, n } => m <= n && *m > 0 && *n > 0,
            _ => true,
        }
    }

    /// Get the minimum number of participants required
    pub fn min_participants(&self) -> u8 {
        match self {
            NodePolicy::All => 1, // Will be set to actual child count during validation
            NodePolicy::Any => 1,
            NodePolicy::Threshold { m, .. } => *m,
        }
    }
}

/// Cryptographic backend identifier (versioned to prevent compatibility issues)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CryptoBackendId {
    /// Ed25519 with Blake3 hashing (MVP implementation)
    Ed25519V1,
    // Future backends deferred to Phase 8:
    // BLS12_381V1,
    // PallasVestaV1,
    // Curve25519V1,
}

impl Default for CryptoBackendId {
    fn default() -> Self {
        Self::Ed25519V1
    }
}

/// Hash function identifier (versioned)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashFunctionId {
    /// Blake3 hashing (MVP implementation)
    Blake3V1,
    // Future hash functions deferred to Phase 8:
    // SHA256V1,
    // PoseidonV1,
}

impl Default for HashFunctionId {
    fn default() -> Self {
        Self::Blake3V1
    }
}

/// Per-child share metadata for threshold unwrap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareHeader {
    /// Which child node this share is for
    pub child_id: NodeId,

    /// Share index (1..=n)
    pub index: u8,

    /// Commitment to the share (for verification)
    /// Serialized opaque bytes - interpretation depends on crypto_backend
    pub commitment: Vec<u8>,

    /// Public verification data
    /// Serialized opaque bytes - interpretation depends on crypto_backend
    pub proof: Vec<u8>,
}

/// Node commitment (32-byte hash)
/// Used for Merkle-ish tree structure with policy-aware derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeCommitment(pub [u8; 32]);

impl NodeCommitment {
    /// Create zero commitment (for testing)
    pub fn zero() -> Self {
        Self([0u8; 32])
    }

    /// Create commitment from hash bytes
    pub fn from_hash(hash: [u8; 32]) -> Self {
        Self(hash)
    }
}

/// A node in the KeyFabric graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyNode {
    /// Unique node identifier (UUID)
    pub id: NodeId,

    /// Type of node (determines semantics)
    pub kind: NodeKind,

    /// Policy for deriving/unwrapping this node's secret
    pub policy: NodePolicy,

    /// AEAD-encrypted node secret (KEK-wrapped)
    /// Unwrapped when policy conditions met
    pub enc_secret: Vec<u8>,

    /// Per-child share metadata for threshold unwrap
    /// (index, commitment, etc.)
    pub share_headers: Vec<ShareHeader>,

    /// AEAD-encrypted messaging key (Groups only)
    /// Used for private group messaging encryption
    pub enc_messaging_key: Option<Vec<u8>>,

    /// Rotation counter (prevents replay attacks)
    pub epoch: u64,

    /// Cryptographic backend for this subtree (versioned enum)
    /// All descendants must use the same backend
    pub crypto_backend: CryptoBackendId,

    /// Hash function for commitment derivation (versioned enum)
    pub hash_function: HashFunctionId,

    /// Non-sensitive metadata (display name, created_at, etc.)
    pub meta: BTreeMap<String, String>,
}

impl KeyNode {
    /// Create a new key node with default values
    pub fn new(id: NodeId, kind: NodeKind, policy: NodePolicy) -> Self {
        Self {
            id,
            kind,
            policy,
            enc_secret: Vec::new(),
            share_headers: Vec::new(),
            enc_messaging_key: None,
            epoch: 0,
            crypto_backend: CryptoBackendId::default(),
            hash_function: HashFunctionId::default(),
            meta: BTreeMap::new(),
        }
    }

    /// Check if this node is a leaf (no children expected)
    pub fn is_leaf(&self) -> bool {
        matches!(self.kind, NodeKind::Device | NodeKind::Guardian)
    }

    /// Check if this node can have a messaging key
    pub fn supports_messaging(&self) -> bool {
        matches!(self.kind, NodeKind::Group)
    }

    /// Get display name from metadata
    pub fn display_name(&self) -> Option<&str> {
        self.meta.get("display_name").map(|s| s.as_str())
    }

    /// Set display name in metadata
    pub fn set_display_name(&mut self, name: String) {
        self.meta.insert("display_name".to_string(), name);
    }

    /// Compute node commitment (Merkle-ish, policy-aware)
    ///
    /// C(node) = H(
    ///   tag = "NODE",
    ///   kind,
    ///   policy,
    ///   epoch,
    ///   children = sort_by_id([C(child_1), ..., C(child_k)])
    /// )
    ///
    /// This commitment is:
    /// - Independent of crypto backend specifics (uses hash_function)
    /// - Deterministic across replicas (sorted children)
    /// - Stable for equality checks
    /// - Suitable for ZK proofs and cross-domain verification
    pub fn compute_commitment(&self, _child_commitments: &[NodeCommitment]) -> NodeCommitment {
        // Implementation deferred to Phase 2
        // For now, return a placeholder based on node ID
        let mut hash = [0u8; 32];
        let id_bytes = self.id.0.as_bytes();
        hash[..id_bytes.len().min(32)].copy_from_slice(&id_bytes[..id_bytes.len().min(32)]);
        NodeCommitment(hash)
    }
}

/// Edge kinds define relationship semantics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Parent-child containment (acyclic)
    /// Contributes to key derivation upward
    /// Example: Identity Contains Device, Identity Contains Guardian
    ///
    /// INVARIANT: Contains edges must form a DAG (enforced at apply time)
    Contains,

    /// OCAP binding (capability token → resource)
    /// Example: Guardian token GrantsCapability to Recovery subtree
    GrantsCapability,
    // References removed - deferred to Phase 8 with Group/Link support
}

/// An edge in the KeyFabric graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEdge {
    /// Unique edge identifier
    pub id: EdgeId,

    /// Source node
    pub from: NodeId,

    /// Target node
    pub to: NodeId,

    /// Edge semantics
    pub kind: EdgeKind,
}

impl KeyEdge {
    /// Create a new edge
    pub fn new(from: NodeId, to: NodeId, kind: EdgeKind) -> Self {
        Self {
            id: EdgeId::new_v4(),
            from,
            to,
            kind,
        }
    }

    /// Check if this edge participates in key derivation
    pub fn participates_in_derivation(&self) -> bool {
        matches!(self.kind, EdgeKind::Contains)
    }
}

/// Capability types specific to KeyFabric operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FabricCapability {
    /// Can perform threshold unwrapping operations
    ThresholdUnwrap,
    /// Can contribute shares to threshold operations
    ShareContribution,
    /// Can participate in node rotation
    NodeRotation,
    /// Can initiate FROST signing operations
    FrostSigning,
    /// Can verify fabric credentials
    CredentialVerification,
    /// Administrative capabilities over fabric
    FabricAdmin,
}

impl fmt::Display for FabricCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FabricCapability::ThresholdUnwrap => write!(f, "threshold_unwrap"),
            FabricCapability::ShareContribution => write!(f, "share_contribution"),
            FabricCapability::NodeRotation => write!(f, "node_rotation"),
            FabricCapability::FrostSigning => write!(f, "frost_signing"),
            FabricCapability::CredentialVerification => write!(f, "credential_verification"),
            FabricCapability::FabricAdmin => write!(f, "fabric_admin"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_policy_validation() {
        assert!(NodePolicy::All.is_valid());
        assert!(NodePolicy::Any.is_valid());
        assert!(NodePolicy::Threshold { m: 2, n: 3 }.is_valid());
        assert!(NodePolicy::Threshold { m: 3, n: 3 }.is_valid());

        // Invalid threshold policies
        assert!(!NodePolicy::Threshold { m: 4, n: 3 }.is_valid()); // m > n
        assert!(!NodePolicy::Threshold { m: 0, n: 3 }.is_valid()); // m = 0
        assert!(!NodePolicy::Threshold { m: 2, n: 0 }.is_valid()); // n = 0
    }

    #[test]
    fn test_node_creation() {
        let device_id = NodeId::new_v4();
        let node = KeyNode::new(device_id, NodeKind::Device, NodePolicy::Any);

        assert_eq!(node.id, device_id);
        assert_eq!(node.kind, NodeKind::Device);
        assert_eq!(node.policy, NodePolicy::Any);
        assert_eq!(node.epoch, 0);
        assert!(node.is_leaf());
        assert!(!node.supports_messaging());
    }

    #[test]
    fn test_edge_creation() {
        let from_id = NodeId::new_v4();
        let to_id = NodeId::new_v4();
        let edge = KeyEdge::new(from_id, to_id, EdgeKind::Contains);

        assert_eq!(edge.from, from_id);
        assert_eq!(edge.to, to_id);
        assert_eq!(edge.kind, EdgeKind::Contains);
        assert!(edge.participates_in_derivation());
    }

    #[test]
    fn test_commitment_deterministic() {
        let node_id = NodeId::new_v4();
        let node1 = KeyNode::new(
            node_id,
            NodeKind::Identity,
            NodePolicy::Threshold { m: 2, n: 3 },
        );
        let node2 = KeyNode::new(
            node_id,
            NodeKind::Identity,
            NodePolicy::Threshold { m: 2, n: 3 },
        );

        let commitment1 = node1.compute_commitment(&[]);
        let commitment2 = node2.compute_commitment(&[]);

        assert_eq!(commitment1, commitment2);
    }
}

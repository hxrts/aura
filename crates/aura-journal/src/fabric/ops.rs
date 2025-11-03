//! KeyFabric Operations - Maps to Automerge native operations
//!
//! This module defines the operation set for KeyFabric and maps them to
//! native Automerge CRDT operations without custom reducers.

use aura_types::{fabric::*, AuraError};
use serde::{Deserialize, Serialize};

/// KeyFabric operation enumeration
///
/// These operations map directly to Automerge native operations,
/// leveraging the built-in CRDT merge semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FabricOp {
    /// Add a new node to the fabric
    AddNode {
        /// The node to add to the fabric graph
        node: KeyNode,
    },

    /// Update a node's threshold policy
    /// Triggers rewrapping of encrypted secrets
    UpdateNodePolicy {
        /// The ID of the node whose policy is being updated
        node: NodeId,
        /// The new threshold policy to apply to the node
        policy: NodePolicy,
    },

    /// Rotate a node's secret (increments epoch)
    /// Invalidates all previous shares
    RotateNode {
        /// The ID of the node whose secret is being rotated
        node: NodeId,
        /// The new encrypted secret wrapped with the new KEK
        new_secret: Vec<u8>,
        /// Optional new messaging key for group nodes only
        new_messaging_key: Option<Vec<u8>>,
    },

    /// Add an edge between nodes
    AddEdge {
        /// The edge to add connecting two nodes in the fabric
        edge: KeyEdge,
    },

    /// Remove an edge (soft delete with tombstone)
    RemoveEdge {
        /// The ID of the edge to remove from the fabric
        edge: EdgeId,
    },

    /// Contribute a share for threshold unwrapping
    /// Multiple devices contribute to reach m-of-n
    ContributeShare {
        /// The ID of the parent node being unwrapped
        node: NodeId,
        /// The ID of the child node receiving the unwrapped secret
        child: NodeId,
        /// The encrypted share data for threshold reconstruction
        share_data: Vec<u8>,
        /// Cryptographic commitment to the share for verification
        commitment: Vec<u8>,
        /// Zero-knowledge proof of correct share generation
        proof: Vec<u8>,
        /// The epoch of the secret being unwrapped
        epoch: u64,
    },

    /// Send encrypted message to group
    SendGroupMessage {
        /// The ID of the group node receiving the message
        group: NodeId,
        /// The encrypted message content
        encrypted_content: Vec<u8>,
        /// Proof that the sender is a valid member of the group
        sender_proof: Vec<u8>,
        /// The epoch of the group's messaging key used for encryption
        epoch: u64,
    },

    /// Bind capability token to fabric resource
    GrantCapability {
        /// The unique identifier for the capability token being granted
        token_id: String,
        /// The fabric resource that this capability grants access to
        target: ResourceRef,
    },

    /// Revoke capability token
    RevokeCapability {
        /// The unique identifier for the capability token being revoked
        token_id: String,
    },
}

impl FabricOp {
    /// Get the operation type for logging/metrics
    pub fn op_type(&self) -> &'static str {
        match self {
            FabricOp::AddNode { .. } => "add_node",
            FabricOp::UpdateNodePolicy { .. } => "update_node_policy",
            FabricOp::RotateNode { .. } => "rotate_node",
            FabricOp::AddEdge { .. } => "add_edge",
            FabricOp::RemoveEdge { .. } => "remove_edge",
            FabricOp::ContributeShare { .. } => "contribute_share",
            FabricOp::SendGroupMessage { .. } => "send_group_message",
            FabricOp::GrantCapability { .. } => "grant_capability",
            FabricOp::RevokeCapability { .. } => "revoke_capability",
        }
    }

    /// Check if this operation requires capability verification
    pub fn requires_capability(&self) -> bool {
        match self {
            FabricOp::AddNode { .. } => true,
            FabricOp::UpdateNodePolicy { .. } => true,
            FabricOp::RotateNode { .. } => true,
            FabricOp::AddEdge { .. } => true,
            FabricOp::RemoveEdge { .. } => true,
            FabricOp::ContributeShare { .. } => true,
            FabricOp::SendGroupMessage { .. } => true,
            FabricOp::GrantCapability { .. } => true,
            FabricOp::RevokeCapability { .. } => true,
        }
    }

    /// Get the resource this operation targets (for capability checking)
    pub fn target_resource(&self) -> Option<String> {
        match self {
            FabricOp::AddNode { node } => Some(format!("fabric://node/{}", node.id)),
            FabricOp::UpdateNodePolicy { node, .. } => Some(format!("fabric://node/{}", node)),
            FabricOp::RotateNode { node, .. } => Some(format!("fabric://node/{}", node)),
            FabricOp::AddEdge { edge } => Some(format!("fabric://edge/{}", edge.id)),
            FabricOp::RemoveEdge { edge } => Some(format!("fabric://edge/{}", edge)),
            FabricOp::ContributeShare { node, .. } => Some(format!("fabric://node/{}", node)),
            FabricOp::SendGroupMessage { group, .. } => Some(format!("fabric://group/{}", group)),
            FabricOp::GrantCapability { target, .. } => Some(target.resource_type()),
            FabricOp::RevokeCapability { token_id } => Some(format!("fabric://token/{}", token_id)),
        }
    }
}

/// Result of applying a fabric operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricOpResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Error message if operation failed
    pub error: Option<String>,

    /// Optional result data (e.g., generated IDs)
    pub data: Option<FabricOpData>,

    /// Metrics about the operation
    pub metrics: FabricOpMetrics,
}

impl FabricOpResult {
    /// Create a successful result
    pub fn success(data: Option<FabricOpData>) -> Self {
        Self {
            success: true,
            error: None,
            data,
            metrics: FabricOpMetrics::default(),
        }
    }

    /// Create a failed result
    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            error: Some(error),
            data: None,
            metrics: FabricOpMetrics::default(),
        }
    }

    /// Create result from AuraError
    pub fn from_error(error: AuraError) -> Self {
        Self::failure(error.to_string())
    }
}

/// Optional data returned from fabric operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FabricOpData {
    /// Node was created/updated
    NodeData {
        /// The ID of the node that was created or updated
        node_id: NodeId,
        /// Cryptographic commitment to the node's current state
        commitment: NodeCommitment,
    },

    /// Edge was created
    EdgeData {
        /// The ID of the edge that was created
        edge_id: EdgeId,
    },

    /// Secret was reconstructed
    SecretData {
        /// The ID of the node whose secret was reconstructed
        node_id: NodeId,
        /// The epoch of the reconstructed secret
        epoch: u64,
    },

    /// Message was sent
    MessageData {
        /// The unique identifier for the sent message
        message_id: String,
        /// The timestamp when the message was sent
        timestamp: u64,
    },

    /// Capability was granted/revoked
    CapabilityData {
        /// The unique identifier for the capability token
        token_id: String,
        /// The resource that the capability grants access to
        resource: String,
    },
}

/// Metrics collected during operation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FabricOpMetrics {
    /// Time taken to execute the operation (milliseconds)
    pub duration_ms: u64,

    /// Number of nodes affected
    pub nodes_affected: u32,

    /// Number of edges affected
    pub edges_affected: u32,

    /// Size of data processed (bytes)
    pub data_size_bytes: u64,

    /// Whether operation required network communication
    pub network_required: bool,

    /// Whether operation required threshold participation
    pub threshold_required: bool,
}

impl Default for FabricOpMetrics {
    fn default() -> Self {
        Self {
            duration_ms: 0,
            nodes_affected: 0,
            edges_affected: 0,
            data_size_bytes: 0,
            network_required: false,
            threshold_required: false,
        }
    }
}

/// Automerge operation mapping utilities
pub struct AutomergeOperations;

impl AutomergeOperations {
    /// Map a fabric operation to Automerge operations
    ///
    /// This function determines how to represent each fabric operation
    /// as native Automerge CRDT operations (Map insertions, Set additions, etc.)
    pub fn map_to_automerge(op: &FabricOp) -> Vec<AutomergeOp> {
        match op {
            FabricOp::AddNode { node } => {
                vec![
                    AutomergeOp::MapInsert {
                        path: vec!["fabric".to_string(), "nodes".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Node(node.clone()),
                    },
                    AutomergeOp::MapInsert {
                        path: vec!["fabric".to_string(), "incoming_edges".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Set(vec![]),
                    },
                    AutomergeOp::MapInsert {
                        path: vec!["fabric".to_string(), "outgoing_edges".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Set(vec![]),
                    },
                ]
            }

            FabricOp::AddEdge { edge } => {
                vec![
                    AutomergeOp::MapInsert {
                        path: vec!["fabric".to_string(), "edges".to_string()],
                        key: edge.id.to_string(),
                        value: AutomergeValue::Edge(edge.clone()),
                    },
                    AutomergeOp::SetAdd {
                        path: vec![
                            "fabric".to_string(),
                            "outgoing_edges".to_string(),
                            edge.from.to_string(),
                        ],
                        value: edge.id.to_string(),
                    },
                    AutomergeOp::SetAdd {
                        path: vec![
                            "fabric".to_string(),
                            "incoming_edges".to_string(),
                            edge.to.to_string(),
                        ],
                        value: edge.id.to_string(),
                    },
                ]
            }

            FabricOp::RemoveEdge { edge } => {
                // Soft delete - add to tombstones set
                vec![AutomergeOp::SetAdd {
                    path: vec!["fabric".to_string(), "tombstones".to_string()],
                    value: edge.to_string(),
                }]
            }

            FabricOp::UpdateNodePolicy { node, policy } => {
                vec![AutomergeOp::MapUpdate {
                    path: vec!["fabric".to_string(), "nodes".to_string(), node.to_string()],
                    field: "policy".to_string(),
                    value: AutomergeValue::Policy(policy.clone()),
                }]
            }

            FabricOp::RotateNode {
                node,
                new_secret,
                new_messaging_key,
            } => {
                let mut ops = vec![
                    AutomergeOp::MapUpdate {
                        path: vec!["fabric".to_string(), "nodes".to_string(), node.to_string()],
                        field: "enc_secret".to_string(),
                        value: AutomergeValue::Bytes(new_secret.clone()),
                    },
                    AutomergeOp::MapUpdate {
                        path: vec!["fabric".to_string(), "nodes".to_string(), node.to_string()],
                        field: "epoch".to_string(),
                        value: AutomergeValue::Counter(1), // Increment epoch
                    },
                ];

                if let Some(messaging_key) = new_messaging_key {
                    ops.push(AutomergeOp::MapUpdate {
                        path: vec!["fabric".to_string(), "nodes".to_string(), node.to_string()],
                        field: "enc_messaging_key".to_string(),
                        value: AutomergeValue::Bytes(messaging_key.clone()),
                    });
                }

                ops
            }

            FabricOp::ContributeShare {
                node,
                child,
                share_data,
                commitment,
                proof,
                epoch,
            } => {
                vec![AutomergeOp::MapInsert {
                    path: vec![
                        "fabric".to_string(),
                        "accumulated_shares".to_string(),
                        format!("{}:{}", node, child),
                    ],
                    key: format!("share_{}", uuid::Uuid::new_v4()),
                    value: AutomergeValue::Share {
                        data: share_data.clone(),
                        commitment: commitment.clone(),
                        proof: proof.clone(),
                        epoch: *epoch,
                    },
                }]
            }

            _ => {
                // For operations not yet implemented, return empty operations
                vec![]
            }
        }
    }
}

/// Automerge operation representation
#[derive(Debug, Clone)]
pub enum AutomergeOp {
    /// Insert a new key-value pair into an Automerge map
    MapInsert {
        /// The path to the map in the Automerge document
        path: Vec<String>,
        /// The key to insert in the map
        key: String,
        /// The value to associate with the key
        value: AutomergeValue,
    },
    /// Update an existing field in an Automerge map
    MapUpdate {
        /// The path to the map in the Automerge document
        path: Vec<String>,
        /// The field name to update
        field: String,
        /// The new value for the field
        value: AutomergeValue,
    },
    /// Add a value to an Automerge set
    SetAdd {
        /// The path to the set in the Automerge document
        path: Vec<String>,
        /// The value to add to the set
        value: String,
    },
    /// Remove a value from an Automerge set
    SetRemove {
        /// The path to the set in the Automerge document
        path: Vec<String>,
        /// The value to remove from the set
        value: String,
    },
}

/// Automerge value types for KeyFabric data
#[derive(Debug, Clone)]
pub enum AutomergeValue {
    /// A complete key node in the fabric
    Node(KeyNode),
    /// An edge connecting two nodes in the fabric
    Edge(KeyEdge),
    /// A threshold policy for a node
    Policy(NodePolicy),
    /// Raw byte data (encrypted secrets, keys, etc.)
    Bytes(Vec<u8>),
    /// A counter value for epochs or versioning
    Counter(i64),
    /// A set of string values
    Set(Vec<String>),
    /// A threshold share with cryptographic proofs
    Share {
        /// The encrypted share data
        data: Vec<u8>,
        /// Cryptographic commitment to the share
        commitment: Vec<u8>,
        /// Zero-knowledge proof of correct share generation
        proof: Vec<u8>,
        /// The epoch of the secret this share belongs to
        epoch: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::fabric::{NodeKind, NodePolicy};

    #[test]
    fn test_fabric_op_type() {
        let node = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let op = FabricOp::AddNode { node };

        assert_eq!(op.op_type(), "add_node");
        assert!(op.requires_capability());
        assert!(op.target_resource().is_some());
    }

    #[test]
    fn test_op_result_creation() {
        let result = FabricOpResult::success(None);
        assert!(result.success);
        assert!(result.error.is_none());

        let error_result = FabricOpResult::failure("Test error".to_string());
        assert!(!error_result.success);
        assert!(error_result.error.is_some());
    }

    #[test]
    fn test_automerge_mapping() {
        let node = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let op = FabricOp::AddNode { node: node.clone() };

        let automerge_ops = AutomergeOperations::map_to_automerge(&op);
        assert_eq!(automerge_ops.len(), 3); // node + incoming + outgoing edges

        // Check that we have the right operation types
        match &automerge_ops[0] {
            AutomergeOp::MapInsert { path, key, .. } => {
                assert_eq!(path, &vec!["fabric".to_string(), "nodes".to_string()]);
                assert_eq!(key, &node.id.to_string());
            }
            _ => panic!("Expected MapInsert operation"),
        }
    }

    #[test]
    fn test_edge_operations() {
        let from_id = NodeId::new_v4();
        let to_id = NodeId::new_v4();
        let edge = KeyEdge::new(from_id, to_id, EdgeKind::Contains);
        let op = FabricOp::AddEdge { edge: edge.clone() };

        let automerge_ops = AutomergeOperations::map_to_automerge(&op);
        assert_eq!(automerge_ops.len(), 3); // edge + outgoing + incoming updates
    }
}

//! KeyJournal Operations - Maps to Automerge native operations
//!
//! This module defines the operation set for KeyJournal and maps them to
//! native Automerge CRDT operations without custom reducers.

use crate::journal::*;
use aura_core::AuraError;
use serde::{Deserialize, Serialize};

/// KeyJournal operation enumeration
///
/// These operations map directly to Automerge native operations,
/// leveraging the built-in CRDT merge semantics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalOp {
    /// Add a new node to the journal
    AddNode {
        /// The node to add to the journal graph
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
        /// The edge to add connecting two nodes in the journal
        edge: KeyEdge,
    },

    /// Remove an edge (soft delete with tombstone)
    RemoveEdge {
        /// The ID of the edge to remove from the journal
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

    /// Bind capability token to journal resource
    GrantCapability {
        /// The unique identifier for the capability token being granted
        token_id: String,
        /// The journal resource that this capability grants access to
        target: ResourceRef,
    },

    /// Revoke capability token
    RevokeCapability {
        /// The unique identifier for the capability token being revoked
        token_id: String,
    },
}

impl JournalOp {
    /// Get the operation type for logging/metrics
    pub fn op_type(&self) -> &'static str {
        match self {
            JournalOp::AddNode { .. } => "add_node",
            JournalOp::UpdateNodePolicy { .. } => "update_node_policy",
            JournalOp::RotateNode { .. } => "rotate_node",
            JournalOp::AddEdge { .. } => "add_edge",
            JournalOp::RemoveEdge { .. } => "remove_edge",
            JournalOp::ContributeShare { .. } => "contribute_share",
            JournalOp::SendGroupMessage { .. } => "send_group_message",
            JournalOp::GrantCapability { .. } => "grant_capability",
            JournalOp::RevokeCapability { .. } => "revoke_capability",
        }
    }

    /// Check if this operation requires capability verification
    pub fn requires_capability(&self) -> bool {
        match self {
            JournalOp::AddNode { .. } => true,
            JournalOp::UpdateNodePolicy { .. } => true,
            JournalOp::RotateNode { .. } => true,
            JournalOp::AddEdge { .. } => true,
            JournalOp::RemoveEdge { .. } => true,
            JournalOp::ContributeShare { .. } => true,
            JournalOp::SendGroupMessage { .. } => true,
            JournalOp::GrantCapability { .. } => true,
            JournalOp::RevokeCapability { .. } => true,
        }
    }

    /// Get the resource this operation targets (for capability checking)
    pub fn target_resource(&self) -> Option<String> {
        match self {
            JournalOp::AddNode { node } => Some(format!("journal://node/{}", node.id)),
            JournalOp::UpdateNodePolicy { node, .. } => Some(format!("journal://node/{}", node)),
            JournalOp::RotateNode { node, .. } => Some(format!("journal://node/{}", node)),
            JournalOp::AddEdge { edge } => Some(format!("journal://edge/{}", edge.id)),
            JournalOp::RemoveEdge { edge } => Some(format!("journal://edge/{}", edge)),
            JournalOp::ContributeShare { node, .. } => Some(format!("journal://node/{}", node)),
            JournalOp::SendGroupMessage { group, .. } => Some(format!("journal://group/{}", group)),
            JournalOp::GrantCapability { target, .. } => Some(target.resource_type()),
            JournalOp::RevokeCapability { token_id } => {
                Some(format!("journal://token/{}", token_id))
            }
        }
    }
}

/// Result of applying a journal operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalOpResult {
    /// Whether the operation succeeded
    pub success: bool,

    /// Error message if operation failed
    pub error: Option<String>,

    /// Optional result data (e.g., generated IDs)
    pub data: Option<JournalOpData>,

    /// Metrics about the operation
    pub metrics: JournalOpMetrics,
}

impl JournalOpResult {
    /// Create a successful result
    pub fn success(data: Option<JournalOpData>) -> Self {
        Self {
            success: true,
            error: None,
            data,
            metrics: JournalOpMetrics::default(),
        }
    }

    /// Create a failed result
    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            error: Some(error),
            data: None,
            metrics: JournalOpMetrics::default(),
        }
    }

    /// Create result from AuraError
    pub fn from_error(error: AuraError) -> Self {
        Self::failure(error.to_string())
    }
}

/// Optional data returned from journal operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalOpData {
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
pub struct JournalOpMetrics {
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

impl Default for JournalOpMetrics {
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
    /// Map a journal operation to Automerge operations
    ///
    /// This function determines how to represent each journal operation
    /// as native Automerge CRDT operations (Map insertions, Set additions, etc.)
    pub fn map_to_automerge(op: &JournalOp) -> Vec<AutomergeOp> {
        match op {
            JournalOp::AddNode { node } => {
                vec![
                    AutomergeOp::MapInsert {
                        path: vec!["journal".to_string(), "nodes".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Node(node.clone()),
                    },
                    AutomergeOp::MapInsert {
                        path: vec!["journal".to_string(), "incoming_edges".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Set(vec![]),
                    },
                    AutomergeOp::MapInsert {
                        path: vec!["journal".to_string(), "outgoing_edges".to_string()],
                        key: node.id.to_string(),
                        value: AutomergeValue::Set(vec![]),
                    },
                ]
            }

            JournalOp::AddEdge { edge } => {
                vec![
                    AutomergeOp::MapInsert {
                        path: vec!["journal".to_string(), "edges".to_string()],
                        key: edge.id.to_string(),
                        value: AutomergeValue::Edge(edge.clone()),
                    },
                    AutomergeOp::SetAdd {
                        path: vec![
                            "journal".to_string(),
                            "outgoing_edges".to_string(),
                            edge.from.to_string(),
                        ],
                        value: edge.id.to_string(),
                    },
                    AutomergeOp::SetAdd {
                        path: vec![
                            "journal".to_string(),
                            "incoming_edges".to_string(),
                            edge.to.to_string(),
                        ],
                        value: edge.id.to_string(),
                    },
                ]
            }

            JournalOp::RemoveEdge { edge } => {
                // Soft delete - add to tombstones set
                vec![AutomergeOp::SetAdd {
                    path: vec!["journal".to_string(), "tombstones".to_string()],
                    value: edge.to_string(),
                }]
            }

            JournalOp::UpdateNodePolicy { node, policy } => {
                vec![AutomergeOp::MapUpdate {
                    path: vec!["journal".to_string(), "nodes".to_string(), node.to_string()],
                    field: "policy".to_string(),
                    value: AutomergeValue::Policy(policy.clone()),
                }]
            }

            JournalOp::RotateNode {
                node,
                new_secret,
                new_messaging_key,
            } => {
                let mut ops = vec![
                    AutomergeOp::MapUpdate {
                        path: vec!["journal".to_string(), "nodes".to_string(), node.to_string()],
                        field: "enc_secret".to_string(),
                        value: AutomergeValue::Bytes(new_secret.clone()),
                    },
                    AutomergeOp::MapUpdate {
                        path: vec!["journal".to_string(), "nodes".to_string(), node.to_string()],
                        field: "epoch".to_string(),
                        value: AutomergeValue::Counter(1), // Increment epoch
                    },
                ];

                if let Some(messaging_key) = new_messaging_key {
                    ops.push(AutomergeOp::MapUpdate {
                        path: vec!["journal".to_string(), "nodes".to_string(), node.to_string()],
                        field: "enc_messaging_key".to_string(),
                        value: AutomergeValue::Bytes(messaging_key.clone()),
                    });
                }

                ops
            }

            JournalOp::ContributeShare {
                node,
                child,
                share_data,
                commitment,
                proof,
                epoch,
            } => {
                // Use deterministic key based on node, child, and epoch
                // This ensures CRDT convergence without requiring random UUIDs
                let share_key = format!("share_{}:{}:{}", node, child, epoch);

                vec![AutomergeOp::MapInsert {
                    path: vec![
                        "journal".to_string(),
                        "accumulated_shares".to_string(),
                        format!("{}:{}", node, child),
                    ],
                    key: share_key,
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

/// Automerge value types for KeyJournal data
#[derive(Debug, Clone)]
pub enum AutomergeValue {
    /// A complete key node in the journal
    Node(KeyNode),
    /// An edge connecting two nodes in the journal
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
    use crate::journal::{NodeKind, NodePolicy};

    #[test]
    fn test_journal_op_type() {
        let node = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node };

        assert_eq!(op.op_type(), "add_node");
        assert!(op.requires_capability());
        assert!(op.target_resource().is_some());
    }

    #[test]
    fn test_op_result_creation() {
        let result = JournalOpResult::success(None);
        assert!(result.success);
        assert!(result.error.is_none());

        let error_result = JournalOpResult::failure("Test error".to_string());
        assert!(!error_result.success);
        assert!(error_result.error.is_some());
    }

    #[test]
    fn test_automerge_mapping() {
        let node = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node: node.clone() };

        let automerge_ops = AutomergeOperations::map_to_automerge(&op);
        assert_eq!(automerge_ops.len(), 3); // node + incoming + outgoing edges

        // Check that we have the right operation types
        match &automerge_ops[0] {
            AutomergeOp::MapInsert { path, key, .. } => {
                assert_eq!(path, &vec!["journal".to_string(), "nodes".to_string()]);
                assert_eq!(key, &node.id.to_string());
            }
            _ => panic!("Expected MapInsert operation"),
        }
    }

    #[test]
    fn test_edge_operations() {
        let from_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());
        let to_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());
        let edge = KeyEdge::new(from_id, to_id, EdgeKind::Contains);
        let op = JournalOp::AddEdge { edge: edge.clone() };

        let automerge_ops = AutomergeOperations::map_to_automerge(&op);
        assert_eq!(automerge_ops.len(), 3); // edge + outgoing + incoming updates
    }
}

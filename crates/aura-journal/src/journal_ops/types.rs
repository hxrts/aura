//! KeyJournal State Types and Core Structures
//!
//! This module defines the core data structures for KeyJournal state management,
//! including the main KeyJournal graph structure and its integration with Automerge.

use crate::journal::*;
use aura_core::{AuraError, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The main KeyJournal structure - a CRDT-based graph of nodes and edges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyJournal {
    /// Map of node ID to node data
    pub nodes: BTreeMap<NodeId, KeyNode>,

    /// Map of edge ID to edge data
    pub edges: BTreeMap<EdgeId, KeyEdge>,

    /// Index: node ID to set of incoming edge IDs (for efficient parent lookup)
    pub incoming_edges: BTreeMap<NodeId, BTreeSet<EdgeId>>,

    /// Index: node ID to set of outgoing edge IDs (for efficient child lookup)
    pub outgoing_edges: BTreeMap<NodeId, BTreeSet<EdgeId>>,

    /// Journal-wide metadata
    pub meta: BTreeMap<String, String>,
}

impl KeyJournal {
    /// Create a new empty journal
    pub fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            incoming_edges: BTreeMap::new(),
            outgoing_edges: BTreeMap::new(),
            meta: BTreeMap::new(),
        }
    }

    /// Add a node to the journal
    pub fn add_node(&mut self, node: KeyNode) -> Result<(), AuraError> {
        let node_id = node.id;

        // Validate node policy
        if !node.policy.is_valid() {
            return Err(AuraError::invalid(format!(
                "Invalid node policy for Node ID: {}",
                node_id
            )));
        }

        // Insert node
        self.nodes.insert(node_id, node);

        // Initialize edge indices
        self.incoming_edges.entry(node_id).or_default();
        self.outgoing_edges.entry(node_id).or_default();

        Ok(())
    }

    /// Add an edge to the journal
    pub fn add_edge(&mut self, edge: KeyEdge) -> Result<(), AuraError> {
        // Validate nodes exist
        if !self.nodes.contains_key(&edge.from) {
            return Err(AuraError::internal(format!(
                "Source node does not exist: Node ID: {}",
                edge.from
            )));
        }
        if !self.nodes.contains_key(&edge.to) {
            return Err(AuraError::internal(format!(
                "Target node does not exist: Node ID: {}",
                edge.to
            )));
        }

        // For Contains edges, check for cycles (basic check - full validation in graph module)
        if edge.kind == EdgeKind::Contains && edge.from == edge.to {
            return Err(AuraError::internal(format!(
                "Self-referential Contains edge not allowed: Edge: {} -> {}",
                edge.from, edge.to
            )));
        }

        let edge_id = edge.id;
        let from_id = edge.from;
        let to_id = edge.to;

        // Insert edge
        self.edges.insert(edge_id, edge);

        // Update indices
        self.outgoing_edges
            .entry(from_id)
            .or_default()
            .insert(edge_id);
        self.incoming_edges
            .entry(to_id)
            .or_default()
            .insert(edge_id);

        Ok(())
    }

    /// Remove an edge from the journal
    pub fn remove_edge(&mut self, edge_id: EdgeId) -> Result<(), AuraError> {
        if let Some(edge) = self.edges.remove(&edge_id) {
            // Update indices
            if let Some(outgoing) = self.outgoing_edges.get_mut(&edge.from) {
                outgoing.remove(&edge_id);
            }
            if let Some(incoming) = self.incoming_edges.get_mut(&edge.to) {
                incoming.remove(&edge_id);
            }
            Ok(())
        } else {
            Err(AuraError::internal(format!(
                "Edge does not exist: Edge ID: {}",
                edge_id
            )))
        }
    }

    /// Get a node by ID
    pub fn get_node(&self, node_id: &NodeId) -> Option<&KeyNode> {
        self.nodes.get(node_id)
    }

    /// Get an edge by ID
    pub fn get_edge(&self, edge_id: &EdgeId) -> Option<&KeyEdge> {
        self.edges.get(edge_id)
    }

    /// Get all children of a node (via Contains edges)
    pub fn get_children(&self, node_id: &NodeId) -> Vec<NodeId> {
        self.outgoing_edges
            .get(node_id)
            .unwrap_or(&BTreeSet::new())
            .iter()
            .filter_map(|edge_id| {
                self.edges.get(edge_id).and_then(|edge| {
                    if edge.kind == EdgeKind::Contains {
                        Some(edge.to)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Get parent of a node (via Contains edges) - assumes single parent
    pub fn get_parent(&self, node_id: &NodeId) -> Option<NodeId> {
        self.incoming_edges
            .get(node_id)
            .unwrap_or(&BTreeSet::new())
            .iter()
            .find_map(|edge_id| {
                self.edges.get(edge_id).and_then(|edge| {
                    if edge.kind == EdgeKind::Contains {
                        Some(edge.from)
                    } else {
                        None
                    }
                })
            })
    }

    /// Find all root nodes (nodes with no Contains parents)
    pub fn get_roots(&self) -> Vec<NodeId> {
        self.nodes
            .keys()
            .filter(|node_id| self.get_parent(node_id).is_none())
            .cloned()
            .collect()
    }

    /// Check if the journal is structurally valid
    pub fn validate(&self) -> Result<(), AuraError> {
        // Basic validation - more thorough validation in graph module
        for node in self.nodes.values() {
            if !node.policy.is_valid() {
                return Err(AuraError::internal(format!(
                    "Node has invalid policy: Node ID: {}",
                    node.id
                )));
            }
        }

        // Check edge consistency
        for edge in self.edges.values() {
            if !self.nodes.contains_key(&edge.from) {
                return Err(AuraError::internal(format!(
                    "Edge references non-existent source node: Edge ID: {}",
                    edge.id
                )));
            }
            if !self.nodes.contains_key(&edge.to) {
                return Err(AuraError::internal(format!(
                    "Edge references non-existent target node: Edge ID: {}",
                    edge.id
                )));
            }
        }

        Ok(())
    }
}

impl Default for KeyJournal {
    fn default() -> Self {
        Self::new()
    }
}

/// Journal state that integrates with the journal's account state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalState {
    /// The main journal graph
    pub journal: KeyJournal,

    /// Per-node accumulated shares for threshold operations
    /// Map: (NodeId, ChildId) -> accumulated shares
    pub accumulated_shares: BTreeMap<(NodeId, NodeId), Vec<ContributedShare>>,

    /// Per-node unwrapped secrets cache
    /// Map: NodeId -> (secret, epoch) - cleared on rotation
    pub unwrapped_secrets: BTreeMap<NodeId, (Vec<u8>, u64)>,

    /// Capability token bindings
    /// Map: capability token ID -> granted resource
    pub capability_bindings: BTreeMap<String, crate::journal::ResourceRef>,

    /// Last modification timestamp for conflict resolution
    pub last_modified: u64,
}

impl JournalState {
    /// Create new journal state
    pub fn new() -> Self {
        Self {
            journal: KeyJournal::new(),
            accumulated_shares: BTreeMap::new(),
            unwrapped_secrets: BTreeMap::new(),
            capability_bindings: BTreeMap::new(),
            last_modified: 0,
        }
    }

    /// Clear unwrapped secrets for a node (called on rotation)
    pub fn clear_secrets(&mut self, node_id: &NodeId) {
        self.unwrapped_secrets.remove(node_id);

        // Also clear any shares for this node
        self.accumulated_shares
            .retain(|(parent_id, _), _| parent_id != node_id);
    }

    /// Update last modified timestamp
    pub fn touch(&mut self, timestamp: u64) {
        self.last_modified = timestamp;
    }
}

impl Default for JournalState {
    fn default() -> Self {
        Self::new()
    }
}

/// A contributed share for threshold reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributedShare {
    /// The device that contributed this share
    pub contributor: DeviceId,

    /// The actual share data (opaque bytes)
    pub share_data: Vec<u8>,

    /// Commitment/proof for verification
    pub commitment: Vec<u8>,

    /// Epoch this share is valid for
    pub epoch: u64,

    /// Timestamp when share was contributed
    pub timestamp: u64,
}

// ResourceRef is defined in journal.rs as an enum - use that instead

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{NodeKind, NodePolicy};

    #[test]
    fn test_journal_creation() {
        let journal = KeyJournal::new();
        assert!(journal.nodes.is_empty());
        assert!(journal.edges.is_empty());
        assert!(journal.validate().is_ok());
    }

    #[test]
    fn test_node_addition() {
        let mut journal = KeyJournal::new();
        let node_id = aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([1u8; 16]));
        let node = KeyNode::new(node_id, NodeKind::Device, NodePolicy::Any);

        assert!(journal.add_node(node).is_ok());
        assert!(journal.nodes.contains_key(&node_id));
        assert!(journal.validate().is_ok());
    }

    #[test]
    fn test_edge_addition() {
        let mut journal = KeyJournal::new();

        // Add two nodes
        let parent_id = aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([2u8; 16]));
        let child_id = aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([3u8; 16]));
        let parent = KeyNode::new(
            parent_id,
            NodeKind::Identity,
            NodePolicy::Threshold { m: 1, n: 1 },
        );
        let child = KeyNode::new(child_id, NodeKind::Device, NodePolicy::Any);

        journal.add_node(parent).unwrap();
        journal.add_node(child).unwrap();

        // Add edge
        let edge = KeyEdge::new(parent_id, child_id, EdgeKind::Contains);
        assert!(journal.add_edge(edge).is_ok());

        // Check relationships
        let children = journal.get_children(&parent_id);
        assert_eq!(children, vec![child_id]);

        let parent = journal.get_parent(&child_id);
        assert_eq!(parent, Some(parent_id));

        assert!(journal.validate().is_ok());
    }

    #[test]
    fn test_invalid_edge() {
        let mut journal = KeyJournal::new();
        let node_id = aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([4u8; 16]));

        // Try to add edge to non-existent node
        let edge = KeyEdge::new(
            node_id,
            aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([5u8; 16])),
            EdgeKind::Contains,
        );
        assert!(journal.add_edge(edge).is_err());
    }

    #[test]
    fn test_self_referential_edge() {
        let mut journal = KeyJournal::new();
        let node_id = aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes([6u8; 16]));
        let node = KeyNode::new(node_id, NodeKind::Device, NodePolicy::Any);

        journal.add_node(node).unwrap();

        // Try to add self-referential edge
        let edge = KeyEdge::new(node_id, node_id, EdgeKind::Contains);
        assert!(journal.add_edge(edge).is_err());
    }
}

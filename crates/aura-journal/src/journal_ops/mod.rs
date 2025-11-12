//! KeyJournal Implementation - Phase 1, 2 & 3 MVP
//!
//! This module implements the core KeyJournal CRDT-based threshold identity system.
//! Phase 1 provides foundation types, basic operations, and TODO fix - Simplified effects.
//! Phase 2 adds policy-aware derivation and DKD integration.
//! Phase 3 adds threshold unwrapping with M-of-N secret reconstruction.

use crate::journal::{EdgeId, EdgeKind, KeyEdge, KeyNode, NodeId, NodeKind};
use aura_core::{AuraError, Hash32};

// Public modules
pub mod graph;
pub mod ops;
pub mod types;

// Phase 2 modules - Local operations only
pub mod derivation;
pub mod views;

// Phase 3: Distributed operations moved to aura-choreography crate
// Use choreographic coordination for threshold, share contribution, and rotation

// Re-exports
pub use graph::*;
pub use ops::*;
pub use types::*;

// Phase 2 re-exports - Local operations only
pub use derivation::*;
pub use views::*;

// Phase 3: Distributed operations available via aura-choreography crate
// Use KeyJournalThresholdChoreography, KeyJournalShareContributionChoreography, etc.

/// KeyJournal Interface
pub struct KeyJournal {
    /// Current journal state containing nodes and edges
    state: JournalState,
}

impl KeyJournal {
    /// Create a new KeyJournal instance
    pub fn new() -> Self {
        Self {
            state: JournalState::new(),
        }
    }

    /// Get immutable reference to the journal state
    pub fn state(&self) -> &JournalState {
        &self.state
    }

    /// Get mutable reference to the journal state
    pub fn state_mut(&mut self) -> &mut JournalState {
        &mut self.state
    }

    /// Add a node to the journal
    pub fn add_node(&mut self, node: KeyNode) -> Result<(), AuraError> {
        // Basic validation
        if self.state.journal.nodes.contains_key(&node.id) {
            return Err(AuraError::internal(format!(
                "Node already exists: Node ID: {}",
                node.id
            )));
        }

        // Apply operation
        self.state.journal.nodes.insert(node.id, node);
        Ok(())
    }

    /// Add an edge between nodes
    pub fn add_edge(&mut self, edge: KeyEdge) -> Result<(), AuraError> {
        // Validate nodes exist
        if !self.state.journal.nodes.contains_key(&edge.from) {
            return Err(AuraError::internal(format!(
                "Source node does not exist: Node ID: {}",
                edge.from
            )));
        }

        if !self.state.journal.nodes.contains_key(&edge.to) {
            return Err(AuraError::internal(format!(
                "Target node does not exist: Node ID: {}",
                edge.to
            )));
        }

        // Apply operation
        self.state.journal.edges.insert(edge.id, edge);
        Ok(())
    }

    /// Get journal statistics
    pub fn get_stats(&self) -> JournalStats {
        JournalStats {
            node_count: self.state.journal.nodes.len(),
            edge_count: self.state.journal.edges.len(),
            device_count: self
                .state
                .journal
                .nodes
                .values()
                .filter(|n| matches!(n.kind, NodeKind::Device))
                .count(),
            guardian_count: self
                .state
                .journal
                .nodes
                .values()
                .filter(|n| matches!(n.kind, NodeKind::Guardian))
                .count(),
        }
    }
}

/// Basic journal statistics
#[derive(Debug, Clone)]
pub struct JournalStats {
    /// Total number of nodes in the journal
    pub node_count: usize,
    /// Total number of edges in the journal
    pub edge_count: usize,
    /// Number of device nodes
    pub device_count: usize,
    /// Number of guardian nodes
    pub guardian_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{NodeKind, NodePolicy};

    #[test]
    fn test_journal() {
        let mut journal = KeyJournal::new();

        // Test adding nodes
        let node1 = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Guardian, NodePolicy::Any);

        journal.add_node(node1.clone()).unwrap();
        journal.add_node(node2.clone()).unwrap();

        // Test adding edge
        #[allow(clippy::disallowed_methods)]
        let edge = KeyEdge::with_id(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        journal.add_edge(edge).unwrap();

        // Test statistics
        let stats = journal.get_stats();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.device_count, 1);
        assert_eq!(stats.guardian_count, 1);
    }

    #[test]
    fn test_node_validation() {
        let mut journal = KeyJournal::new();

        let node1 = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Guardian, NodePolicy::Any);
        let node3 = KeyNode::new(aura_core::identifiers::DeviceId(uuid::Uuid::new_v4()), NodeKind::Device, NodePolicy::Any);

        journal.add_node(node1.clone()).unwrap();
        journal.add_node(node2.clone()).unwrap();

        // Try to add edge from non-existent node (should fail)
        #[allow(clippy::disallowed_methods)]
        let edge = KeyEdge::with_id(EdgeId::new_v4(), node3.id, node1.id, EdgeKind::Contains);
        let result = journal.add_edge(edge);
        assert!(result.is_err());
    }
}

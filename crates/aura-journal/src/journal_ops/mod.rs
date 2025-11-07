//! KeyJournal Implementation - Phase 1, 2 & 3 MVP
//!
//! This module implements the core KeyJournal CRDT-based threshold identity system.
//! Phase 1 provides foundation types, basic operations, and simplified effects.
//! Phase 2 adds policy-aware derivation and DKD integration.
//! Phase 3 adds threshold unwrapping with M-of-N secret reconstruction.

use crate::journal::*;
use aura_types::AuraError;

// Public modules
pub mod effects_simple;
pub mod graph;
pub mod ops;
pub mod types;

// Phase 2 modules - Local operations only
pub mod derivation;
pub mod views;

// Phase 3: Distributed operations moved to aura-choreography crate
// Use choreographic coordination for threshold, share contribution, and rotation

// Re-exports for MVP
pub use effects_simple::*;
pub use graph::*;
pub use ops::*;
pub use types::*;

// Phase 2 re-exports - Local operations only
pub use derivation::*;
pub use views::*;

// Phase 3: Distributed operations available via aura-choreography crate
// Use KeyJournalThresholdChoreography, KeyJournalShareContributionChoreography, etc.

// TODO: Full re-exports when modules are implemented
// pub use effects::*;
// pub use handlers::*;
// pub use middleware::*;

/// KeyJournal Phase 1 MVP Interface
pub struct KeyJournalMvp {
    /// Current journal state containing nodes and edges
    state: JournalState,
    /// Injectable effects for external dependencies
    effects: Box<dyn SimpleJournalEffects>,
}

impl KeyJournalMvp {
    /// Create a new KeyJournal MVP instance with the provided effects
    pub fn new(effects: Box<dyn SimpleJournalEffects>) -> Self {
        Self {
            state: JournalState::new(),
            effects,
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
    pub async fn add_node(&mut self, node: KeyNode) -> Result<(), AuraError> {
        // Basic validation
        if self.state.journal.nodes.contains_key(&node.id) {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Node already exists".to_string(),
                    context: format!("Node ID: {}", node.id),
                },
            ));
        }

        // Apply operation
        self.state.journal.nodes.insert(node.id, node);
        Ok(())
    }

    /// Add an edge between nodes
    pub async fn add_edge(&mut self, edge: KeyEdge) -> Result<(), AuraError> {
        // Validate nodes exist
        if !self.state.journal.nodes.contains_key(&edge.from) {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Source node does not exist".to_string(),
                    context: format!("Node ID: {}", edge.from),
                },
            ));
        }

        if !self.state.journal.nodes.contains_key(&edge.to) {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Target node does not exist".to_string(),
                    context: format!("Node ID: {}", edge.to),
                },
            ));
        }

        // Check for cycles
        let existing_edges: Vec<(NodeId, NodeId)> = self
            .state
            .journal
            .edges
            .values()
            .map(|e| (e.from, e.to))
            .collect();

        let new_edge_tuple = (edge.from, edge.to);
        if self
            .effects
            .would_create_cycle(&existing_edges, new_edge_tuple)
            .await?
        {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Edge would create cycle".to_string(),
                    context: format!("Edge: {} -> {}", edge.from, edge.to),
                },
            ));
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

    #[tokio::test]
    async fn test_journal_mvp() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Box::new(SimpleJournalEffectsAdapter::new(device_id));
        let mut journal = KeyJournalMvp::new(effects);

        // Test adding nodes
        let node1 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(NodeId::new_v4(), NodeKind::Guardian, NodePolicy::Any);

        journal.add_node(node1.clone()).await.unwrap();
        journal.add_node(node2.clone()).await.unwrap();

        // Test adding edge
        #[allow(clippy::disallowed_methods)]
        let edge = KeyEdge::with_id(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        journal.add_edge(edge).await.unwrap();

        // Test statistics
        let stats = journal.get_stats();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.device_count, 1);
        assert_eq!(stats.guardian_count, 1);
    }

    #[tokio::test]
    async fn test_cycle_prevention() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Box::new(SimpleJournalEffectsAdapter::new(device_id));
        let mut journal = KeyJournalMvp::new(effects);

        let node1 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(NodeId::new_v4(), NodeKind::Guardian, NodePolicy::Any);

        journal.add_node(node1.clone()).await.unwrap();
        journal.add_node(node2.clone()).await.unwrap();

        // Add edge 1 -> 2
        #[allow(clippy::disallowed_methods)]
        let edge1 = KeyEdge::with_id(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        journal.add_edge(edge1).await.unwrap();

        // Try to add edge 2 -> 1 (should fail - creates cycle)
        #[allow(clippy::disallowed_methods)]
        let edge2 = KeyEdge::with_id(EdgeId::new_v4(), node2.id, node1.id, EdgeKind::Contains);
        let result = journal.add_edge(edge2).await;
        assert!(result.is_err());
    }
}

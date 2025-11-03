//! KeyFabric Implementation - Phase 1, 2 & 3 MVP
//!
//! This module implements the core KeyFabric CRDT-based threshold identity system.
//! Phase 1 provides foundation types, basic operations, and simplified effects.
//! Phase 2 adds policy-aware derivation and DKD integration.
//! Phase 3 adds threshold unwrapping with M-of-N secret reconstruction.

use aura_types::{fabric::*, AuraError};

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

// TODO: Complex modules (to be completed in future phase)
// pub mod effects;
// pub mod handlers;
// pub mod middleware;

// Re-exports for MVP
pub use effects_simple::*;
pub use graph::*;
pub use ops::*;
pub use types::*;

// Phase 2 re-exports - Local operations only
pub use derivation::*;
pub use views::*;

// Phase 3: Distributed operations available via aura-choreography crate
// Use KeyFabricThresholdChoreography, KeyFabricShareContributionChoreography, etc.

// TODO: Full re-exports when modules are implemented
// pub use effects::*;
// pub use handlers::*;
// pub use middleware::*;

/// KeyFabric Phase 1 MVP Interface
pub struct KeyFabricMvp {
    /// Current fabric state containing nodes and edges
    state: FabricState,
    /// Injectable effects for external dependencies
    effects: Box<dyn SimpleFabricEffects>,
}

impl KeyFabricMvp {
    /// Create a new KeyFabric MVP instance with the provided effects
    pub fn new(effects: Box<dyn SimpleFabricEffects>) -> Self {
        Self {
            state: FabricState::new(),
            effects,
        }
    }

    /// Get immutable reference to the fabric state
    pub fn state(&self) -> &FabricState {
        &self.state
    }

    /// Get mutable reference to the fabric state
    pub fn state_mut(&mut self) -> &mut FabricState {
        &mut self.state
    }

    /// Add a node to the fabric
    pub async fn add_node(&mut self, node: KeyNode) -> Result<(), AuraError> {
        // Basic validation
        if self.state.fabric.nodes.contains_key(&node.id) {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Node already exists".to_string(),
                    context: format!("Node ID: {}", node.id),
                },
            ));
        }

        // Apply operation
        self.state.fabric.nodes.insert(node.id, node);
        Ok(())
    }

    /// Add an edge between nodes
    pub async fn add_edge(&mut self, edge: KeyEdge) -> Result<(), AuraError> {
        // Validate nodes exist
        if !self.state.fabric.nodes.contains_key(&edge.from) {
            return Err(AuraError::Data(
                aura_types::errors::DataError::LedgerOperationFailed {
                    message: "Source node does not exist".to_string(),
                    context: format!("Node ID: {}", edge.from),
                },
            ));
        }

        if !self.state.fabric.nodes.contains_key(&edge.to) {
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
            .fabric
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
        self.state.fabric.edges.insert(edge.id, edge);
        Ok(())
    }

    /// Get fabric statistics
    pub fn get_stats(&self) -> FabricStats {
        FabricStats {
            node_count: self.state.fabric.nodes.len(),
            edge_count: self.state.fabric.edges.len(),
            device_count: self
                .state
                .fabric
                .nodes
                .values()
                .filter(|n| matches!(n.kind, NodeKind::Device))
                .count(),
            guardian_count: self
                .state
                .fabric
                .nodes
                .values()
                .filter(|n| matches!(n.kind, NodeKind::Guardian))
                .count(),
        }
    }
}

/// Basic fabric statistics
#[derive(Debug, Clone)]
pub struct FabricStats {
    /// Total number of nodes in the fabric
    pub node_count: usize,
    /// Total number of edges in the fabric
    pub edge_count: usize,
    /// Number of device nodes
    pub device_count: usize,
    /// Number of guardian nodes
    pub guardian_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::fabric::{NodeKind, NodePolicy};

    #[tokio::test]
    async fn test_fabric_mvp() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Box::new(SimpleFabricEffectsAdapter::new(device_id));
        let mut fabric = KeyFabricMvp::new(effects);

        // Test adding nodes
        let node1 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(NodeId::new_v4(), NodeKind::Guardian, NodePolicy::Any);

        fabric.add_node(node1.clone()).await.unwrap();
        fabric.add_node(node2.clone()).await.unwrap();

        // Test adding edge
        let edge = KeyEdge::new(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        fabric.add_edge(edge).await.unwrap();

        // Test statistics
        let stats = fabric.get_stats();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.device_count, 1);
        assert_eq!(stats.guardian_count, 1);
    }

    #[tokio::test]
    async fn test_cycle_prevention() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Box::new(SimpleFabricEffectsAdapter::new(device_id));
        let mut fabric = KeyFabricMvp::new(effects);

        let node1 = KeyNode::new(NodeId::new_v4(), NodeKind::Device, NodePolicy::Any);
        let node2 = KeyNode::new(NodeId::new_v4(), NodeKind::Guardian, NodePolicy::Any);

        fabric.add_node(node1.clone()).await.unwrap();
        fabric.add_node(node2.clone()).await.unwrap();

        // Add edge 1 -> 2
        let edge1 = KeyEdge::new(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        fabric.add_edge(edge1).await.unwrap();

        // Try to add edge 2 -> 1 (should fail - creates cycle)
        let edge2 = KeyEdge::new(EdgeId::new_v4(), node2.id, node1.id, EdgeKind::Contains);
        let result = fabric.add_edge(edge2).await;
        assert!(result.is_err());
    }
}

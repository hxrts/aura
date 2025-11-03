//! KeyFabric Graph Algorithms and Validation
//!
//! This module provides graph algorithms and validation for the KeyFabric
//! using petgraph for efficient cycle detection and graph traversal.

use super::types::KeyFabric;
use aura_types::fabric::*;
use petgraph::Graph;
use std::collections::{BTreeMap, BTreeSet};

/// Graph algorithms and validation for KeyFabric
pub struct FabricGraph;

impl FabricGraph {
    /// Check if adding an edge would create a cycle in the Contains subgraph
    pub fn would_create_cycle(fabric: &KeyFabric, new_edge: &KeyEdge) -> Result<bool, GraphError> {
        if new_edge.kind != EdgeKind::Contains {
            // Only Contains edges participate in cycle detection
            return Ok(false);
        }

        // Build directed graph of Contains edges
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();

        // Add all nodes that participate in Contains relationships
        let mut participating_nodes = BTreeSet::new();
        for edge in fabric.edges.values() {
            if edge.kind == EdgeKind::Contains {
                participating_nodes.insert(edge.from);
                participating_nodes.insert(edge.to);
            }
        }
        participating_nodes.insert(new_edge.from);
        participating_nodes.insert(new_edge.to);

        // Add nodes to graph
        for &node_id in &participating_nodes {
            let idx = graph.add_node(node_id);
            node_indices.insert(node_id, idx);
        }

        // Add existing Contains edges
        for edge in fabric.edges.values() {
            if edge.kind == EdgeKind::Contains {
                if let (Some(&from_idx), Some(&to_idx)) =
                    (node_indices.get(&edge.from), node_indices.get(&edge.to))
                {
                    graph.add_edge(from_idx, to_idx, ());
                }
            }
        }

        // Add the new edge
        if let (Some(&from_idx), Some(&to_idx)) = (
            node_indices.get(&new_edge.from),
            node_indices.get(&new_edge.to),
        ) {
            graph.add_edge(from_idx, to_idx, ());
        }

        // Check for cycles using DFS
        Ok(petgraph::algo::is_cyclic_directed(&graph))
    }

    /// Validate the entire fabric structure
    pub fn validate_fabric(fabric: &KeyFabric) -> Result<(), GraphError> {
        // Check basic fabric validity
        fabric
            .validate()
            .map_err(|e| GraphError::ValidationFailed(format!("Basic validation failed: {}", e)))?;

        // Check for cycles in Contains subgraph
        let contains_edges: Vec<(NodeId, NodeId)> = fabric
            .edges
            .values()
            .filter(|edge| edge.kind == EdgeKind::Contains)
            .map(|edge| (edge.from, edge.to))
            .collect();

        if Self::has_cycles(&contains_edges)? {
            return Err(GraphError::CycleDetected(
                "Cycle detected in Contains edges".to_string(),
            ));
        }

        // Check that each node has at most one Contains parent
        let mut child_parent_count = BTreeMap::new();
        for edge in fabric.edges.values() {
            if edge.kind == EdgeKind::Contains {
                let count = child_parent_count.entry(edge.to).or_insert(0);
                *count += 1;
                if *count > 1 {
                    return Err(GraphError::ValidationFailed(format!(
                        "Node {} has multiple Contains parents",
                        edge.to
                    )));
                }
            }
        }

        // Validate policy consistency
        for node in fabric.nodes.values() {
            let children = fabric.get_children(&node.id);
            match &node.policy {
                NodePolicy::Threshold { m, n } => {
                    if children.len() != *n as usize {
                        return Err(GraphError::ValidationFailed(format!(
                            "Node {} has threshold policy {}-of-{} but {} children",
                            node.id,
                            m,
                            n,
                            children.len()
                        )));
                    }
                }
                NodePolicy::All => {
                    if children.is_empty() && !node.is_leaf() {
                        return Err(GraphError::ValidationFailed(format!(
                            "Non-leaf node {} has All policy but no children",
                            node.id
                        )));
                    }
                }
                NodePolicy::Any => {
                    if children.is_empty() && !node.is_leaf() {
                        return Err(GraphError::ValidationFailed(format!(
                            "Non-leaf node {} has Any policy but no children",
                            node.id
                        )));
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a set of edges contains cycles
    fn has_cycles(edges: &[(NodeId, NodeId)]) -> Result<bool, GraphError> {
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();

        // Add all nodes
        for (from, to) in edges {
            if !node_indices.contains_key(from) {
                let idx = graph.add_node(*from);
                node_indices.insert(*from, idx);
            }
            if !node_indices.contains_key(to) {
                let idx = graph.add_node(*to);
                node_indices.insert(*to, idx);
            }
        }

        // Add edges
        for (from, to) in edges {
            if let (Some(&from_idx), Some(&to_idx)) = (node_indices.get(from), node_indices.get(to))
            {
                graph.add_edge(from_idx, to_idx, ());
            }
        }

        Ok(petgraph::algo::is_cyclic_directed(&graph))
    }

    /// Find all root nodes (nodes with no Contains parents)
    pub fn find_roots(fabric: &KeyFabric) -> Vec<NodeId> {
        let mut roots = Vec::new();

        for node_id in fabric.nodes.keys() {
            if fabric.get_parent(node_id).is_none() {
                roots.push(*node_id);
            }
        }

        roots
    }

    /// Get topological ordering of nodes for derivation
    pub fn topological_order(fabric: &KeyFabric, root: NodeId) -> Result<Vec<NodeId>, GraphError> {
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();
        let mut visited = BTreeSet::new();

        // Build subgraph starting from root
        let mut to_visit = vec![root];

        while let Some(node_id) = to_visit.pop() {
            if visited.contains(&node_id) {
                continue;
            }
            visited.insert(node_id);

            // Add node to graph
            if !node_indices.contains_key(&node_id) {
                let idx = graph.add_node(node_id);
                node_indices.insert(node_id, idx);
            }

            // Add children
            let children = fabric.get_children(&node_id);
            for child_id in children {
                if !node_indices.contains_key(&child_id) {
                    let idx = graph.add_node(child_id);
                    node_indices.insert(child_id, idx);
                }

                // Add edge from child to parent (reverse for topological sort)
                if let (Some(&child_idx), Some(&parent_idx)) =
                    (node_indices.get(&child_id), node_indices.get(&node_id))
                {
                    graph.add_edge(child_idx, parent_idx, ());
                }

                to_visit.push(child_id);
            }
        }

        // Perform topological sort
        let sorted = petgraph::algo::toposort(&graph, None)
            .map_err(|_| GraphError::CycleDetected("Cycle in derivation tree".to_string()))?;

        let result = sorted.iter().map(|&idx| graph[idx]).collect();

        Ok(result)
    }

    /// Find strongly connected components
    pub fn find_components(fabric: &KeyFabric) -> Result<Vec<Vec<NodeId>>, GraphError> {
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();

        // Build graph with Contains edges
        for node_id in fabric.nodes.keys() {
            let idx = graph.add_node(*node_id);
            node_indices.insert(*node_id, idx);
        }

        for edge in fabric.edges.values() {
            if edge.kind == EdgeKind::Contains {
                if let (Some(&from_idx), Some(&to_idx)) =
                    (node_indices.get(&edge.from), node_indices.get(&edge.to))
                {
                    graph.add_edge(from_idx, to_idx, ());
                }
            }
        }

        // Find SCCs
        let sccs = petgraph::algo::kosaraju_scc(&graph);
        let result = sccs
            .into_iter()
            .map(|scc| scc.into_iter().map(|idx| graph[idx]).collect())
            .collect();

        Ok(result)
    }

    /// Check if a path exists between two nodes
    pub fn has_path(fabric: &KeyFabric, from: NodeId, to: NodeId) -> Result<bool, GraphError> {
        if from == to {
            return Ok(true);
        }

        let mut visited = BTreeSet::new();
        let mut to_visit = vec![from];

        while let Some(current) = to_visit.pop() {
            if visited.contains(&current) {
                continue;
            }
            visited.insert(current);

            if current == to {
                return Ok(true);
            }

            // Add children to visit list
            let children = fabric.get_children(&current);
            to_visit.extend(children);
        }

        Ok(false)
    }

    /// Calculate the depth of a node from the root
    pub fn node_depth(fabric: &KeyFabric, node_id: NodeId) -> Option<usize> {
        let mut depth = 0;
        let mut current = node_id;

        loop {
            if let Some(parent) = fabric.get_parent(&current) {
                depth += 1;
                current = parent;

                // Prevent infinite loops (shouldn't happen with valid fabric)
                if depth > 100 {
                    return None;
                }
            } else {
                // Reached root
                break;
            }
        }

        Some(depth)
    }
}

/// Errors that can occur during graph operations
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// A cycle was detected in the graph
    #[error("Cycle detected: {0}")]
    CycleDetected(String),

    /// Graph validation failed
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// Graph algorithm error
    #[error("Algorithm error: {0}")]
    AlgorithmError(String),

    /// Node not found
    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::fabric::{NodeKind, NodePolicy};

    fn create_test_fabric() -> KeyFabric {
        let mut fabric = KeyFabric::new();

        // Create identity with 2 devices
        let identity_id = NodeId::new_v4();
        let device1_id = NodeId::new_v4();
        let device2_id = NodeId::new_v4();

        let identity = KeyNode::new(
            identity_id,
            NodeKind::Identity,
            NodePolicy::Threshold { m: 2, n: 2 },
        );
        let device1 = KeyNode::new(device1_id, NodeKind::Device, NodePolicy::Any);
        let device2 = KeyNode::new(device2_id, NodeKind::Device, NodePolicy::Any);

        fabric.add_node(identity).unwrap();
        fabric.add_node(device1).unwrap();
        fabric.add_node(device2).unwrap();

        let edge1 = KeyEdge::new(identity_id, device1_id, EdgeKind::Contains);
        let edge2 = KeyEdge::new(identity_id, device2_id, EdgeKind::Contains);

        fabric.add_edge(edge1).unwrap();
        fabric.add_edge(edge2).unwrap();

        fabric
    }

    #[test]
    fn test_fabric_validation() {
        let fabric = create_test_fabric();
        assert!(FabricGraph::validate_fabric(&fabric).is_ok());
    }

    #[test]
    fn test_cycle_detection() {
        let mut fabric = create_test_fabric();

        // Get existing nodes
        let identity_id = FabricGraph::find_roots(&fabric)[0];
        let device_ids = fabric.get_children(&identity_id);
        let device1_id = device_ids[0];

        // Try to create a cycle: device1 -> identity
        let cycle_edge = KeyEdge::new(device1_id, identity_id, EdgeKind::Contains);

        assert!(FabricGraph::would_create_cycle(&fabric, &cycle_edge).unwrap());
    }

    #[test]
    fn test_no_cycle_detection() {
        let fabric = create_test_fabric();

        // Create a new disconnected node
        let new_node_id = NodeId::new_v4();
        let new_device_id = NodeId::new_v4();

        let edge = KeyEdge::new(new_node_id, new_device_id, EdgeKind::Contains);

        assert!(!FabricGraph::would_create_cycle(&fabric, &edge).unwrap());
    }

    #[test]
    fn test_topological_order() {
        let fabric = create_test_fabric();
        let roots = FabricGraph::find_roots(&fabric);
        let root = roots[0];

        let order = FabricGraph::topological_order(&fabric, root).unwrap();

        // Should have 3 nodes: identity + 2 devices
        assert_eq!(order.len(), 3);

        // Root should be last (topological order for derivation)
        assert_eq!(order[2], root);
    }

    #[test]
    fn test_node_depth() {
        let fabric = create_test_fabric();
        let roots = FabricGraph::find_roots(&fabric);
        let root = roots[0];
        let children = fabric.get_children(&root);

        // Root has depth 0
        assert_eq!(FabricGraph::node_depth(&fabric, root), Some(0));

        // Children have depth 1
        for child in children {
            assert_eq!(FabricGraph::node_depth(&fabric, child), Some(1));
        }
    }

    #[test]
    fn test_path_exists() {
        let fabric = create_test_fabric();
        let roots = FabricGraph::find_roots(&fabric);
        let root = roots[0];
        let children = fabric.get_children(&root);
        let child = children[0];

        // Path from root to child should exist
        assert!(FabricGraph::has_path(&fabric, root, child).unwrap());

        // Path from child to root should not exist (directed graph)
        assert!(!FabricGraph::has_path(&fabric, child, root).unwrap());
    }
}

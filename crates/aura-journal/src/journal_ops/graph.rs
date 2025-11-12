//! KeyJournal Graph Algorithms and Validation
//!
//! This module provides graph algorithms and validation for the KeyJournal
//! using petgraph for efficient cycle detection and graph traversal.

use super::types::KeyJournal;
use crate::journal::*;
use petgraph::Graph;
use std::collections::{BTreeMap, BTreeSet};

/// Graph algorithms and validation for KeyJournal
pub struct JournalGraph;

impl JournalGraph {
    /// Check if adding an edge would create a cycle in the Contains subgraph
    pub fn would_create_cycle(
        journal: &KeyJournal,
        new_edge: &KeyEdge,
    ) -> Result<bool, GraphError> {
        if new_edge.kind != EdgeKind::Contains {
            // Only Contains edges participate in cycle detection
            return Ok(false);
        }

        // Build directed graph of Contains edges
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();

        // Add all nodes that participate in Contains relationships
        let mut participating_nodes = BTreeSet::new();
        for edge in journal.edges.values() {
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
        for edge in journal.edges.values() {
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

    /// Validate the entire journal structure
    pub fn validate_journal(journal: &KeyJournal) -> Result<(), GraphError> {
        // Check basic journal validity
        journal
            .validate()
            .map_err(|e| GraphError::ValidationFailed(format!("Basic validation failed: {}", e)))?;

        // Check for cycles in Contains subgraph
        let contains_edges: Vec<(NodeId, NodeId)> = journal
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
        for edge in journal.edges.values() {
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
        for node in journal.nodes.values() {
            let children = journal.get_children(&node.id);
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
    pub fn find_roots(journal: &KeyJournal) -> Vec<NodeId> {
        let mut roots = Vec::new();

        for node_id in journal.nodes.keys() {
            if journal.get_parent(node_id).is_none() {
                roots.push(*node_id);
            }
        }

        roots
    }

    /// Get topological ordering of nodes for derivation
    pub fn topological_order(
        journal: &KeyJournal,
        root: NodeId,
    ) -> Result<Vec<NodeId>, GraphError> {
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
            let children = journal.get_children(&node_id);
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
    pub fn find_components(journal: &KeyJournal) -> Result<Vec<Vec<NodeId>>, GraphError> {
        let mut graph = Graph::new();
        let mut node_indices = BTreeMap::new();

        // Build graph with Contains edges
        for node_id in journal.nodes.keys() {
            let idx = graph.add_node(*node_id);
            node_indices.insert(*node_id, idx);
        }

        for edge in journal.edges.values() {
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
    pub fn has_path(journal: &KeyJournal, from: NodeId, to: NodeId) -> Result<bool, GraphError> {
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
            let children = journal.get_children(&current);
            to_visit.extend(children);
        }

        Ok(false)
    }

    /// Calculate the depth of a node from the root
    pub fn node_depth(journal: &KeyJournal, node_id: NodeId) -> Option<usize> {
        let mut depth = 0;
        let mut current = node_id;

        loop {
            if let Some(parent) = journal.get_parent(&current) {
                depth += 1;
                current = parent;

                // Prevent infinite loops (shouldn't happen with valid journal)
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
    use crate::journal::{NodeKind, NodePolicy};

    fn create_test_journal() -> KeyJournal {
        let mut journal = KeyJournal::new();

        // Create identity with 2 devices
        let identity_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());
        let device1_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());
        let device2_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());

        let identity = KeyNode::new(
            identity_id,
            NodeKind::Identity,
            NodePolicy::Threshold { m: 2, n: 2 },
        );
        let device1 = KeyNode::new(device1_id, NodeKind::Device, NodePolicy::Any);
        let device2 = KeyNode::new(device2_id, NodeKind::Device, NodePolicy::Any);

        journal.add_node(identity).unwrap();
        journal.add_node(device1).unwrap();
        journal.add_node(device2).unwrap();

        let edge1 = KeyEdge::new(identity_id, device1_id, EdgeKind::Contains);
        let edge2 = KeyEdge::new(identity_id, device2_id, EdgeKind::Contains);

        journal.add_edge(edge1).unwrap();
        journal.add_edge(edge2).unwrap();

        journal
    }

    #[test]
    fn test_journal_validation() {
        let journal = create_test_journal();
        assert!(JournalGraph::validate_journal(&journal).is_ok());
    }

    #[test]
    fn test_cycle_detection() {
        let mut journal = create_test_journal();

        // Get existing nodes
        let identity_id = JournalGraph::find_roots(&journal)[0];
        let device_ids = journal.get_children(&identity_id);
        let device1_id = device_ids[0];

        // Try to create a cycle: device1 -> identity
        let cycle_edge = KeyEdge::new(device1_id, identity_id, EdgeKind::Contains);

        assert!(JournalGraph::would_create_cycle(&journal, &cycle_edge).unwrap());
    }

    #[test]
    fn test_no_cycle_detection() {
        let journal = create_test_journal();

        // Create a new disconnected node
        let new_node_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());
        let new_device_id = aura_core::identifiers::DeviceId(uuid::Uuid::new_v4());

        let edge = KeyEdge::new(new_node_id, new_device_id, EdgeKind::Contains);

        assert!(!JournalGraph::would_create_cycle(&journal, &edge).unwrap());
    }

    #[test]
    fn test_topological_order() {
        let journal = create_test_journal();
        let roots = JournalGraph::find_roots(&journal);
        let root = roots[0];

        let order = JournalGraph::topological_order(&journal, root).unwrap();

        // Should have 3 nodes: identity + 2 devices
        assert_eq!(order.len(), 3);

        // Root should be last (topological order for derivation)
        assert_eq!(order[2], root);
    }

    #[test]
    fn test_node_depth() {
        let journal = create_test_journal();
        let roots = JournalGraph::find_roots(&journal);
        let root = roots[0];
        let children = journal.get_children(&root);

        // Root has depth 0
        assert_eq!(JournalGraph::node_depth(&journal, root), Some(0));

        // Children have depth 1
        for child in children {
            assert_eq!(JournalGraph::node_depth(&journal, child), Some(1));
        }
    }

    #[test]
    fn test_path_exists() {
        let journal = create_test_journal();
        let roots = JournalGraph::find_roots(&journal);
        let root = roots[0];
        let children = journal.get_children(&root);
        let child = children[0];

        // Path from root to child should exist
        assert!(JournalGraph::has_path(&journal, root, child).unwrap());

        // Path from child to root should not exist (directed graph)
        assert!(!JournalGraph::has_path(&journal, child, root).unwrap());
    }
}

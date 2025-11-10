//! Key Derivation
//!
//! Implements key derivation along Contains edges with policy-aware commitment computation.
//! Uses petgraph for graph traversal and Blake3 for commitment hashing.

use crate::journal::*;
use aura_core::AuraError;
use blake3::Hasher;
use std::collections::{BTreeMap, HashSet};

/// Derivation effects trait for external dependency injection
#[async_trait::async_trait]
pub trait DerivationEffects: Send + Sync {
    /// Compute Blake3 hash of input data
    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], AuraError>;

    /// Get children of a node via Contains edges
    async fn get_node_children(
        &self,
        node_id: NodeId,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<Vec<NodeId>, AuraError>;

    /// Check if graph traversal should continue
    async fn should_continue_traversal(&self, depth: u32) -> Result<bool, AuraError>;
}

/// Simple derivation effects adapter for MVP
/// TODO: complete complete implementation
pub struct SimpleDerivationEffects;

#[async_trait::async_trait]
impl DerivationEffects for SimpleDerivationEffects {
    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], AuraError> {
        Ok(*blake3::hash(data).as_bytes())
    }

    async fn get_node_children(
        &self,
        node_id: NodeId,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<Vec<NodeId>, AuraError> {
        let mut children: Vec<NodeId> = edges
            .values()
            .filter(|edge| edge.from == node_id && edge.kind == EdgeKind::Contains)
            .map(|edge| edge.to)
            .collect();

        // Sort for deterministic ordering per specification
        children.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Ok(children)
    }

    async fn should_continue_traversal(&self, depth: u32) -> Result<bool, AuraError> {
        // Prevent infinite loops - reasonable depth limit for MVP
        Ok(depth < 32)
    }
}

/// Derivation engine for computing node commitments
pub struct DerivationEngine {
    /// Injectable effects for hash computation and graph traversal
    effects: Box<dyn DerivationEffects>,
}

impl DerivationEngine {
    /// Create a new derivation engine with the provided effects
    pub fn new(effects: Box<dyn DerivationEffects>) -> Self {
        Self { effects }
    }

    /// Compute commitment for a single node
    ///
    /// Commitment formula per specification:
    /// C(node) = H(tag="NODE", kind, policy, epoch, sorted_child_commitments)
    pub async fn compute_node_commitment(
        &self,
        node: &KeyNode,
        child_commitments: &[NodeCommitment],
        _nodes: &BTreeMap<NodeId, KeyNode>,
        _edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<NodeCommitment, AuraError> {
        let mut hasher = Hasher::new();

        // Add tag
        hasher.update(b"NODE");

        // Add kind (single byte encoding per specification)
        let kind_byte = match node.kind {
            NodeKind::Device => 0x01u8,
            NodeKind::Identity => 0x02u8,
            NodeKind::Group => 0x03u8,
            NodeKind::Guardian => 0x04u8,
        };
        hasher.update(&[kind_byte]);

        // Add policy (canonical encoding per specification)
        let policy_bytes = match &node.policy {
            NodePolicy::All => vec![0x01u8],
            NodePolicy::Any => vec![0x02u8],
            NodePolicy::Threshold { m, n } => vec![0x03u8, *m, *n],
        };
        hasher.update(&policy_bytes);

        // Add epoch (little-endian u64)
        hasher.update(&node.epoch.to_le_bytes());

        // Add sorted child commitments
        let mut sorted_commitments = child_commitments.to_vec();
        sorted_commitments.sort_by(|a, b| a.0.cmp(&b.0));
        for commitment in sorted_commitments {
            hasher.update(&commitment.0);
        }

        let hash = self.effects.hash_data(hasher.finalize().as_bytes()).await?;
        Ok(NodeCommitment(hash))
    }

    /// Compute commitment for an entire subtree rooted at the given node
    pub async fn compute_subtree_commitment(
        &self,
        root_id: NodeId,
        nodes: &BTreeMap<NodeId, KeyNode>,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<NodeCommitment, AuraError> {
        self.compute_subtree_commitment_recursive(root_id, nodes, edges, 0, &mut HashSet::new())
            .await
    }

    /// Recursive helper for subtree commitment computation
    fn compute_subtree_commitment_recursive<'a>(
        &'a self,
        node_id: NodeId,
        nodes: &'a BTreeMap<NodeId, KeyNode>,
        edges: &'a BTreeMap<EdgeId, KeyEdge>,
        depth: u32,
        visited: &'a mut HashSet<NodeId>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<NodeCommitment, AuraError>> + Send + 'a>,
    > {
        Box::pin(async move {
            // Check traversal limits
            if !self.effects.should_continue_traversal(depth).await? {
                return Err(AuraError::invalid(format!(
                    "Derivation traversal depth exceeded - Node: {}, Depth: {}",
                    node_id, depth
                )));
            }

            // Cycle detection
            if visited.contains(&node_id) {
                return Err(AuraError::invalid(format!(
                    "Cycle detected in derivation graph - Node: {}",
                    node_id
                )));
            }
            visited.insert(node_id);

            // Get the node
            let node = nodes.get(&node_id).ok_or_else(|| {
                AuraError::not_found(format!("Node not found - Node ID: {}", node_id))
            })?;

            // Get children via Contains edges
            let children = self.effects.get_node_children(node_id, edges).await?;

            // Recursively compute child commitments
            let mut child_commitments = Vec::new();
            for child_id in children {
                let child_commitment = self
                    .compute_subtree_commitment_recursive(
                        child_id,
                        nodes,
                        edges,
                        depth + 1,
                        visited,
                    )
                    .await?;
                child_commitments.push(child_commitment);
            }

            // Compute this node's commitment
            let commitment = self
                .compute_node_commitment(node, &child_commitments, nodes, edges)
                .await?;

            // Remove from visited set (allow revisiting in other branches)
            visited.remove(&node_id);

            Ok(commitment)
        })
    }

    /// Verify a commitment for a subtree
    pub async fn verify_subtree_commitment(
        &self,
        root_id: NodeId,
        expected_commitment: &NodeCommitment,
        nodes: &BTreeMap<NodeId, KeyNode>,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<bool, AuraError> {
        let computed_commitment = self
            .compute_subtree_commitment(root_id, nodes, edges)
            .await?;
        Ok(computed_commitment == *expected_commitment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{NodeKind, NodePolicy};

    fn create_test_node(kind: NodeKind, policy: NodePolicy) -> KeyNode {
        KeyNode::new(NodeId::new(), kind, policy)
    }

    #[tokio::test]
    async fn test_single_node_commitment() {
        let effects = Box::new(SimpleDerivationEffects);
        let engine = DerivationEngine::new(effects);

        let node = create_test_node(NodeKind::Device, NodePolicy::Any);
        let nodes = BTreeMap::new();
        let edges = BTreeMap::new();

        let commitment = engine
            .compute_node_commitment(&node, &[], &nodes, &edges)
            .await
            .unwrap();
        assert_eq!(commitment.0.len(), 32); // Blake3 produces 32-byte hashes
    }

    #[tokio::test]
    async fn test_commitment_deterministic() {
        let effects1 = Box::new(SimpleDerivationEffects);
        let effects2 = Box::new(SimpleDerivationEffects);
        let engine1 = DerivationEngine::new(effects1);
        let engine2 = DerivationEngine::new(effects2);

        let node = create_test_node(NodeKind::Identity, NodePolicy::Threshold { m: 2, n: 3 });
        let nodes = BTreeMap::new();
        let edges = BTreeMap::new();

        let commitment1 = engine1
            .compute_node_commitment(&node, &[], &nodes, &edges)
            .await
            .unwrap();
        let commitment2 = engine2
            .compute_node_commitment(&node, &[], &nodes, &edges)
            .await
            .unwrap();

        assert_eq!(commitment1, commitment2);
    }

    #[tokio::test]
    async fn test_different_policies_different_commitments() {
        let effects = Box::new(SimpleDerivationEffects);
        let engine = DerivationEngine::new(effects);

        let node1 = create_test_node(NodeKind::Identity, NodePolicy::All);
        let node2 = create_test_node(NodeKind::Identity, NodePolicy::Any);
        let nodes = BTreeMap::new();
        let edges = BTreeMap::new();

        let commitment1 = engine
            .compute_node_commitment(&node1, &[], &nodes, &edges)
            .await
            .unwrap();
        let commitment2 = engine
            .compute_node_commitment(&node2, &[], &nodes, &edges)
            .await
            .unwrap();

        assert_ne!(commitment1, commitment2);
    }

    #[tokio::test]
    async fn test_subtree_with_children() {
        let effects = Box::new(SimpleDerivationEffects);
        let engine = DerivationEngine::new(effects);

        // Create identity with two devices
        let identity = create_test_node(NodeKind::Identity, NodePolicy::Threshold { m: 2, n: 2 });
        let device1 = create_test_node(NodeKind::Device, NodePolicy::Any);
        let device2 = create_test_node(NodeKind::Device, NodePolicy::Any);

        let mut nodes = BTreeMap::new();
        nodes.insert(identity.id, identity.clone());
        nodes.insert(device1.id, device1.clone());
        nodes.insert(device2.id, device2.clone());

        let mut edges = BTreeMap::new();
        let edge1 = KeyEdge::new(identity.id, device1.id, EdgeKind::Contains);
        let edge2 = KeyEdge::new(identity.id, device2.id, EdgeKind::Contains);
        edges.insert(edge1.id, edge1);
        edges.insert(edge2.id, edge2);

        let commitment = engine
            .compute_subtree_commitment(identity.id, &nodes, &edges)
            .await
            .unwrap();
        assert_eq!(commitment.0.len(), 32);

        // Verify the commitment
        let is_valid = engine
            .verify_subtree_commitment(identity.id, &commitment, &nodes, &edges)
            .await
            .unwrap();
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_cycle_detection() {
        let effects = Box::new(SimpleDerivationEffects);
        let engine = DerivationEngine::new(effects);

        let node1 = create_test_node(NodeKind::Identity, NodePolicy::Any);
        let node2 = create_test_node(NodeKind::Device, NodePolicy::Any);

        let mut nodes = BTreeMap::new();
        nodes.insert(node1.id, node1.clone());
        nodes.insert(node2.id, node2.clone());

        // Create a cycle: node1 -> node2 -> node1
        let mut edges = BTreeMap::new();
        #[allow(clippy::disallowed_methods)]
        let edge1 = KeyEdge::with_id(EdgeId::new_v4(), node1.id, node2.id, EdgeKind::Contains);
        #[allow(clippy::disallowed_methods)]
        let edge2 = KeyEdge::with_id(EdgeId::new_v4(), node2.id, node1.id, EdgeKind::Contains);
        edges.insert(edge1.id, edge1);
        edges.insert(edge2.id, edge2);

        let result = engine
            .compute_subtree_commitment(node1.id, &nodes, &edges)
            .await;
        assert!(result.is_err());
    }
}

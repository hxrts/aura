//! Materialization API
//!
//! Provides materialized views of the journal graph for different use cases.
//! Uses petgraph for efficient graph queries and maintains eventually consistent views.

use super::derivation::{DerivationEffects, DerivationEngine};
use crate::journal::*;
use aura_core::{AuraError, DeviceId};
use std::collections::{BTreeMap, HashSet};

/// View effects trait for external dependency injection
#[async_trait::async_trait]
pub trait ViewEffects: Send + Sync {
    /// Access journal nodes
    async fn get_nodes(&self) -> Result<&BTreeMap<NodeId, KeyNode>, AuraError>;

    /// Access journal edges
    async fn get_edges(&self) -> Result<&BTreeMap<EdgeId, KeyEdge>, AuraError>;

    /// Log view materialization for debugging
    async fn log_view_materialization(
        &self,
        view_type: &str,
        root_id: NodeId,
    ) -> Result<(), AuraError>;
}

/// Identity view - materialized view of an identity subtree
#[derive(Debug, Clone)]
pub struct IdentityView {
    /// Root identity node
    pub identity_node: KeyNode,

    /// Identity commitment
    pub commitment: NodeCommitment,

    /// Device nodes under this identity
    pub devices: Vec<KeyNode>,

    /// Guardian nodes (if recovery subtree exists)
    pub guardians: Vec<KeyNode>,

    /// Recovery policy (if recovery subtree exists)
    pub recovery_policy: Option<NodePolicy>,

    /// Total threshold requirement for this identity
    pub total_threshold: ThresholdRequirement,
}

/// Group view - materialized view of a group subtree
#[derive(Debug, Clone)]
pub struct GroupView {
    /// Root group node
    pub group_node: KeyNode,

    /// Group commitment
    pub commitment: NodeCommitment,

    /// Member identity references
    pub members: Vec<NodeId>,

    /// Group messaging policy
    pub messaging_policy: NodePolicy,

    /// Whether group has encrypted messaging key
    pub has_messaging_key: bool,
}

/// Threshold requirement summary
#[derive(Debug, Clone)]
pub struct ThresholdRequirement {
    /// Minimum required participants
    pub required: u8,

    /// Total available participants
    pub available: u8,

    /// Number of device participants available
    pub devices: u8,
    /// Number of guardian participants available
    pub guardians: u8,
}

/// View handler using effects pattern
pub struct ViewHandler {
    /// Injectable effects for accessing journal state
    effects: Box<dyn ViewEffects>,
    /// Derivation engine for computing commitments
    derivation: DerivationEngine,
}

impl ViewHandler {
    /// Create a new view handler with the provided effects
    pub fn new(
        effects: Box<dyn ViewEffects>,
        derivation_effects: Box<dyn DerivationEffects>,
    ) -> Self {
        Self {
            effects,
            derivation: DerivationEngine::new(derivation_effects),
        }
    }

    /// Materialize an identity view from a root identity node
    pub async fn materialize_identity(&self, root_id: NodeId) -> Result<IdentityView, AuraError> {
        self.effects
            .log_view_materialization("identity", root_id)
            .await?;

        let nodes = self.effects.get_nodes().await?;
        let edges = self.effects.get_edges().await?;

        // Get the root identity node
        let identity_node = nodes
            .get(&root_id)
            .ok_or_else(|| {
                AuraError::internal(format!("Identity node not found: Node ID: {}", root_id))
            })?
            .clone();

        // Verify it's actually an identity node
        if !matches!(identity_node.kind, NodeKind::Identity) {
            return Err(AuraError::internal(format!(
                "Node is not an identity: Node kind: {:?}",
                identity_node.kind
            )));
        }

        // Compute commitment for the identity
        let commitment = self
            .derivation
            .compute_subtree_commitment(root_id, nodes, edges)
            .await?;

        // Find all devices under this identity
        let devices = self
            .collect_nodes_by_kind(root_id, NodeKind::Device, nodes, edges)
            .await?;

        // Find recovery subtree and guardians
        let (guardians, recovery_policy) =
            self.collect_recovery_info(root_id, nodes, edges).await?;

        // Calculate threshold requirements
        let total_threshold =
            self.calculate_threshold_requirement(&identity_node, &devices, &guardians)?;

        Ok(IdentityView {
            identity_node,
            commitment,
            devices,
            guardians,
            recovery_policy,
            total_threshold,
        })
    }

    /// Materialize a group view from a root group node
    pub async fn materialize_group(&self, root_id: NodeId) -> Result<GroupView, AuraError> {
        self.effects
            .log_view_materialization("group", root_id)
            .await?;

        let nodes = self.effects.get_nodes().await?;
        let edges = self.effects.get_edges().await?;

        // Get the root group node
        let group_node = nodes
            .get(&root_id)
            .ok_or_else(|| {
                AuraError::internal(format!("Group node not found: Node ID: {}", root_id))
            })?
            .clone();

        // Verify it's actually a group node
        if !matches!(group_node.kind, NodeKind::Group) {
            return Err(AuraError::internal(format!(
                "Node is not a group: Node kind: {:?}",
                group_node.kind
            )));
        }

        // Compute commitment for the group
        let commitment = self
            .derivation
            .compute_subtree_commitment(root_id, nodes, edges)
            .await?;

        // Find all member references
        let members = self.collect_member_references(root_id, edges).await?;

        // Group messaging policy is the group's own policy
        let messaging_policy = group_node.policy.clone();

        // Check if group has messaging key
        let has_messaging_key = group_node.enc_messaging_key.is_some();

        Ok(GroupView {
            group_node,
            commitment,
            members,
            messaging_policy,
            has_messaging_key,
        })
    }

    /// Collect all nodes of a specific kind under a root node
    async fn collect_nodes_by_kind(
        &self,
        root_id: NodeId,
        target_kind: NodeKind,
        nodes: &BTreeMap<NodeId, KeyNode>,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<Vec<KeyNode>, AuraError> {
        let mut result = Vec::new();
        let mut visited = HashSet::new();

        self.collect_nodes_recursive(
            root_id,
            &target_kind,
            nodes,
            edges,
            &mut result,
            &mut visited,
        )
        .await?;

        // Sort for deterministic ordering
        result.sort_by(|a, b| a.id.0.as_bytes().cmp(b.id.0.as_bytes()));
        Ok(result)
    }

    /// Recursive helper for collecting nodes
    #[allow(clippy::only_used_in_recursion)]
    fn collect_nodes_recursive<'a>(
        &'a self,
        node_id: NodeId,
        target_kind: &'a NodeKind,
        nodes: &'a BTreeMap<NodeId, KeyNode>,
        edges: &'a BTreeMap<EdgeId, KeyEdge>,
        result: &'a mut Vec<KeyNode>,
        visited: &'a mut HashSet<NodeId>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AuraError>> + Send + 'a>>
    {
        Box::pin(async move {
            if visited.contains(&node_id) {
                return Ok(()); // Avoid cycles
            }
            visited.insert(node_id);

            let node = nodes.get(&node_id).ok_or_else(|| {
                AuraError::internal(format!(
                    "Node not found during collection: Node ID: {}",
                    node_id
                ))
            })?;

            // If this node matches the target kind, add it
            if std::mem::discriminant(&node.kind) == std::mem::discriminant(target_kind) {
                result.push(node.clone());
            }

            // Recurse into children via Contains edges
            for edge in edges.values() {
                if edge.from == node_id && edge.kind == EdgeKind::Contains {
                    self.collect_nodes_recursive(
                        edge.to,
                        target_kind,
                        nodes,
                        edges,
                        result,
                        visited,
                    )
                    .await?;
                }
            }

            Ok(())
        })
    }

    /// Collect recovery information (guardians and policy)
    async fn collect_recovery_info(
        &self,
        root_id: NodeId,
        nodes: &BTreeMap<NodeId, KeyNode>,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<(Vec<KeyNode>, Option<NodePolicy>), AuraError> {
        // Look for recovery subtrees under the identity
        for edge in edges.values() {
            if edge.from == root_id && edge.kind == EdgeKind::Contains {
                if let Some(child_node) = nodes.get(&edge.to) {
                    // Check if this is a recovery-related node
                    // For MVP, we assume guardian nodes directly under identity are part of recovery
                    if matches!(child_node.kind, NodeKind::Guardian) {
                        let guardians = self
                            .collect_nodes_by_kind(root_id, NodeKind::Guardian, nodes, edges)
                            .await?;
                        // For MVP, use the identity's policy for recovery
                        let identity_node = nodes
                            .get(&root_id)
                            .ok_or_else(|| AuraError::not_found("Identity node not found"))?;
                        return Ok((guardians, Some(identity_node.policy.clone())));
                    }
                }
            }
        }

        Ok((Vec::new(), None))
    }

    /// Collect member references for a group
    async fn collect_member_references(
        &self,
        root_id: NodeId,
        edges: &BTreeMap<EdgeId, KeyEdge>,
    ) -> Result<Vec<NodeId>, AuraError> {
        let mut members: Vec<NodeId> = edges
            .values()
            .filter(|edge| edge.from == root_id && edge.kind == EdgeKind::Contains)
            .map(|edge| edge.to)
            .collect();

        // Sort for deterministic ordering
        members.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Ok(members)
    }

    /// Calculate threshold requirements for an identity
    fn calculate_threshold_requirement(
        &self,
        identity_node: &KeyNode,
        devices: &[KeyNode],
        guardians: &[KeyNode],
    ) -> Result<ThresholdRequirement, AuraError> {
        let device_count = devices.len() as u8;
        let guardian_count = guardians.len() as u8;
        let total_available = device_count + guardian_count;

        let required = match &identity_node.policy {
            NodePolicy::All => total_available,
            NodePolicy::Any => 1,
            NodePolicy::Threshold { m, n: _ } => *m,
        };

        Ok(ThresholdRequirement {
            required,
            available: total_available,
            devices: device_count,
            guardians: guardian_count,
        })
    }
}

/// Simple view effects adapter for MVP
pub struct SimpleViewEffects<'a> {
    /// Reference to journal nodes
    nodes: &'a BTreeMap<NodeId, KeyNode>,
    /// Reference to journal edges
    edges: &'a BTreeMap<EdgeId, KeyEdge>,
    /// Device ID for logging and context
    device_id: DeviceId,
}

impl<'a> SimpleViewEffects<'a> {
    /// Create a new simple view effects adapter with references to journal data
    pub fn new(
        nodes: &'a BTreeMap<NodeId, KeyNode>,
        edges: &'a BTreeMap<EdgeId, KeyEdge>,
        device_id: DeviceId,
    ) -> Self {
        Self {
            nodes,
            edges,
            device_id,
        }
    }
}

#[async_trait::async_trait]
impl<'a> ViewEffects for SimpleViewEffects<'a> {
    async fn get_nodes(&self) -> Result<&BTreeMap<NodeId, KeyNode>, AuraError> {
        Ok(self.nodes)
    }

    async fn get_edges(&self) -> Result<&BTreeMap<EdgeId, KeyEdge>, AuraError> {
        Ok(self.edges)
    }

    async fn log_view_materialization(
        &self,
        view_type: &str,
        root_id: NodeId,
    ) -> Result<(), AuraError> {
        // For MVP, just log to debug (in production this might go to structured logging)
        tracing::debug!(
            device_id = %self.device_id,
            view_type = view_type,
            root_id = %root_id,
            "Materializing journal view"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::derivation::SimpleDerivationEffects;
    use super::*;
    use crate::journal::{NodeKind, NodePolicy};
    use aura_macros::aura_test;

    fn create_test_node_with_id(id_bytes: [u8; 16], kind: NodeKind, policy: NodePolicy) -> KeyNode {
        KeyNode::new(
            aura_core::identifiers::DeviceId(uuid::Uuid::from_bytes(id_bytes)),
            kind,
            policy,
        )
    }

    /// Test-specific ViewEffects that owns its data for lifetime compatibility
    struct TestViewEffects {
        nodes: BTreeMap<NodeId, KeyNode>,
        edges: BTreeMap<EdgeId, KeyEdge>,
        device_id: DeviceId,
    }

    impl TestViewEffects {
        fn new(
            nodes: BTreeMap<NodeId, KeyNode>,
            edges: BTreeMap<EdgeId, KeyEdge>,
            device_id: DeviceId,
        ) -> Self {
            Self {
                nodes,
                edges,
                device_id,
            }
        }
    }

    #[async_trait::async_trait]
    impl ViewEffects for TestViewEffects {
        async fn get_nodes(&self) -> Result<&BTreeMap<NodeId, KeyNode>, AuraError> {
            Ok(&self.nodes)
        }

        async fn get_edges(&self) -> Result<&BTreeMap<EdgeId, KeyEdge>, AuraError> {
            Ok(&self.edges)
        }

        async fn log_view_materialization(
            &self,
            view_type: &str,
            root_id: NodeId,
        ) -> Result<(), AuraError> {
            tracing::debug!(
                device_id = %self.device_id,
                view_type = view_type,
                root_id = %root_id,
                "Materializing journal view"
            );
            Ok(())
        }
    }

    #[aura_test]
    async fn test_identity_view_materialization() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([2u8; 16]));

        // Create identity with devices
        let identity = create_test_node_with_id([10u8; 16], NodeKind::Identity, NodePolicy::Threshold { m: 2, n: 3 });
        let device1 = create_test_node_with_id([11u8; 16], NodeKind::Device, NodePolicy::Any);
        let device2 = create_test_node_with_id([12u8; 16], NodeKind::Device, NodePolicy::Any);
        let device3 = create_test_node_with_id([13u8; 16], NodeKind::Device, NodePolicy::Any);

        let mut nodes = BTreeMap::new();
        nodes.insert(identity.id, identity.clone());
        nodes.insert(device1.id, device1.clone());
        nodes.insert(device2.id, device2.clone());
        nodes.insert(device3.id, device3.clone());

        let mut edges = BTreeMap::new();
        let edge1 = KeyEdge::new(identity.id, device1.id, EdgeKind::Contains);
        let edge2 = KeyEdge::new(identity.id, device2.id, EdgeKind::Contains);
        let edge3 = KeyEdge::new(identity.id, device3.id, EdgeKind::Contains);
        edges.insert(edge1.id, edge1);
        edges.insert(edge2.id, edge2);
        edges.insert(edge3.id, edge3);

        // Create view handler
        let view_effects = Box::new(TestViewEffects::new(nodes, edges, device_id));
        let derivation_effects = Box::new(SimpleDerivationEffects);
        let handler = ViewHandler::new(view_effects, derivation_effects);

        // Materialize identity view
        let view = handler.materialize_identity(identity.id).await.unwrap();

        assert_eq!(view.identity_node.id, identity.id);
        assert_eq!(view.devices.len(), 3);
        assert_eq!(view.guardians.len(), 0);
        assert_eq!(view.total_threshold.required, 2);
        assert_eq!(view.total_threshold.available, 3);
        assert_eq!(view.total_threshold.devices, 3);
        assert_eq!(view.total_threshold.guardians, 0);
        Ok(())
    }

    #[aura_test]
    async fn test_identity_with_guardians() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([2u8; 16]));

        // Create identity with devices and guardians
        let identity = create_test_node_with_id([30u8; 16], NodeKind::Identity, NodePolicy::Threshold { m: 2, n: 4 });
        let device1 = create_test_node_with_id([31u8; 16], NodeKind::Device, NodePolicy::Any);
        let device2 = create_test_node_with_id([32u8; 16], NodeKind::Device, NodePolicy::Any);
        let guardian1 = create_test_node_with_id([33u8; 16], NodeKind::Guardian, NodePolicy::Any);
        let guardian2 = create_test_node_with_id([34u8; 16], NodeKind::Guardian, NodePolicy::Any);

        let mut nodes = BTreeMap::new();
        nodes.insert(identity.id, identity.clone());
        nodes.insert(device1.id, device1.clone());
        nodes.insert(device2.id, device2.clone());
        nodes.insert(guardian1.id, guardian1.clone());
        nodes.insert(guardian2.id, guardian2.clone());

        let mut edges = BTreeMap::new();
        let edge1 = KeyEdge::new(identity.id, device1.id, EdgeKind::Contains);
        let edge2 = KeyEdge::new(identity.id, device2.id, EdgeKind::Contains);
        let edge3 = KeyEdge::new(identity.id, guardian1.id, EdgeKind::Contains);
        let edge4 = KeyEdge::new(identity.id, guardian2.id, EdgeKind::Contains);
        edges.insert(edge1.id, edge1);
        edges.insert(edge2.id, edge2);
        edges.insert(edge3.id, edge3);
        edges.insert(edge4.id, edge4);

        // Create view handler
        let view_effects = Box::new(TestViewEffects::new(nodes, edges, device_id));
        let derivation_effects = Box::new(SimpleDerivationEffects);
        let handler = ViewHandler::new(view_effects, derivation_effects);

        // Materialize identity view
        let view = handler.materialize_identity(identity.id).await.unwrap();

        assert_eq!(view.devices.len(), 2);
        assert_eq!(view.guardians.len(), 2);
        assert_eq!(view.total_threshold.required, 2);
        assert_eq!(view.total_threshold.available, 4);
        assert_eq!(view.total_threshold.devices, 2);
        assert_eq!(view.total_threshold.guardians, 2);
        assert!(view.recovery_policy.is_some());
        Ok(())
    }

    #[aura_test]
    async fn test_group_view_materialization() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([2u8; 16]));

        // Create group with member references
        let group = create_test_node_with_id([20u8; 16], NodeKind::Group, NodePolicy::Threshold { m: 2, n: 3 });

        // Create member identity nodes for the group
        let member1 = create_test_node_with_id([21u8; 16], NodeKind::Identity, NodePolicy::Any);
        let member2 = create_test_node_with_id([22u8; 16], NodeKind::Identity, NodePolicy::Any);
        let member3 = create_test_node_with_id([23u8; 16], NodeKind::Identity, NodePolicy::Any);

        // Use the member nodes' actual IDs
        let member1_id = member1.id;
        let member2_id = member2.id;
        let member3_id = member3.id;

        let mut nodes = BTreeMap::new();
        nodes.insert(group.id, group.clone());
        nodes.insert(member1_id, member1);
        nodes.insert(member2_id, member2);
        nodes.insert(member3_id, member3);

        let mut edges = BTreeMap::new();
        #[allow(clippy::disallowed_methods)]
        #[allow(clippy::disallowed_methods)]
        let edge1 = KeyEdge::with_id(
            uuid::Uuid::from_bytes([3u8; 16]),
            group.id,
            member1_id,
            EdgeKind::Contains,
        );
        #[allow(clippy::disallowed_methods)]
        #[allow(clippy::disallowed_methods)]
        let edge2 = KeyEdge::with_id(
            uuid::Uuid::from_bytes([4u8; 16]),
            group.id,
            member2_id,
            EdgeKind::Contains,
        );
        #[allow(clippy::disallowed_methods)]
        #[allow(clippy::disallowed_methods)]
        let edge3 = KeyEdge::with_id(
            uuid::Uuid::from_bytes([5u8; 16]),
            group.id,
            member3_id,
            EdgeKind::Contains,
        );
        edges.insert(edge1.id, edge1);
        edges.insert(edge2.id, edge2);
        edges.insert(edge3.id, edge3);

        // Create view handler
        let view_effects = Box::new(TestViewEffects::new(nodes, edges, device_id));
        let derivation_effects = Box::new(SimpleDerivationEffects);
        let handler = ViewHandler::new(view_effects, derivation_effects);

        // Materialize group view
        let view = handler.materialize_group(group.id).await.unwrap();

        assert_eq!(view.group_node.id, group.id);
        assert_eq!(view.members.len(), 3);
        assert!(matches!(
            view.messaging_policy,
            NodePolicy::Threshold { m: 2, n: 3 }
        ));
        assert!(!view.has_messaging_key); // No messaging key set in test
        Ok(())
    }
}

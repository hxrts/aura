//! KeyFabric Handler Implementation
//!
//! This module provides the handler implementations for KeyFabric operations,
//! following the established Aura effects/handler pattern.

use aura_types::{fabric::*, AuraError};
use async_trait::async_trait;
use std::sync::Arc;

use super::{
    effects::FabricEffects,
    graph::{FabricGraph, GraphError},
    ops::{FabricOp, FabricOpResult, FabricOpData, FabricOpMetrics},
    types::{FabricState, KeyFabric},
};

/// Configuration for the fabric handler
#[derive(Debug, Clone)]
pub struct FabricHandlerConfig {
    /// Maximum number of nodes allowed in the fabric
    pub max_nodes: usize,
    
    /// Maximum number of edges allowed in the fabric
    pub max_edges: usize,
    
    /// Whether to enable strict validation
    pub strict_validation: bool,
    
    /// Default cryptographic backend
    pub default_crypto_backend: CryptoBackendId,
    
    /// Default hash function
    pub default_hash_function: HashFunctionId,
}

impl Default for FabricHandlerConfig {
    fn default() -> Self {
        Self {
            max_nodes: 10000,
            max_edges: 50000,
            strict_validation: true,
            default_crypto_backend: CryptoBackendId::Ed25519V1,
            default_hash_function: HashFunctionId::Blake3V1,
        }
    }
}

/// Handler trait for fabric operations
#[async_trait]
pub trait FabricHandler: Send + Sync {
    /// Apply a fabric operation to the state
    async fn apply_operation(
        &self,
        op: FabricOp,
        state: &mut FabricState,
    ) -> Result<FabricOpResult, AuraError>;
    
    /// Validate a fabric operation before applying
    async fn validate_operation(
        &self,
        op: &FabricOp,
        state: &FabricState,
    ) -> Result<(), AuraError>;
    
    /// Get the current fabric configuration
    fn config(&self) -> &FabricHandlerConfig;
}

/// Production implementation of FabricHandler
pub struct FabricHandlerImpl<E: FabricEffects> {
    effects: Arc<E>,
    config: FabricHandlerConfig,
}

impl<E: FabricEffects> FabricHandlerImpl<E> {
    /// Create a new fabric handler with the given effects and config
    pub fn new(effects: Arc<E>, config: FabricHandlerConfig) -> Self {
        Self { effects, config }
    }
    
    /// Create a new fabric handler with default config
    pub fn with_effects(effects: Arc<E>) -> Self {
        Self::new(effects, FabricHandlerConfig::default())
    }
}

#[async_trait]
impl<E: FabricEffects> FabricHandler for FabricHandlerImpl<E> {
    async fn apply_operation(
        &self,
        op: FabricOp,
        state: &mut FabricState,
    ) -> Result<FabricOpResult, AuraError> {
        let start_time = self.effects.current_timestamp().await?;
        
        // Validate operation first
        self.validate_operation(&op, state).await?;
        
        // Apply the operation
        let result = match op {
            FabricOp::AddNode { node } => {
                self.apply_add_node(node, state).await
            },
            FabricOp::AddEdge { edge } => {
                self.apply_add_edge(edge, state).await
            },
            FabricOp::RemoveEdge { edge } => {
                self.apply_remove_edge(edge, state).await
            },
            FabricOp::UpdateNodePolicy { node, policy } => {
                self.apply_update_policy(node, policy, state).await
            },
            FabricOp::RotateNode { node, new_secret, new_messaging_key } => {
                self.apply_rotate_node(node, new_secret, new_messaging_key, state).await
            },
            // Note: Share contribution now handled by choreographies
            FabricOp::ContributeShare { .. } => {
                Err(AuraError::Data("Share contribution must be coordinated via choreographies".to_string()))
            },
            FabricOp::GrantCapability { token_id, target } => {
                self.apply_grant_capability(token_id, target, state).await
            },
            FabricOp::RevokeCapability { token_id } => {
                self.apply_revoke_capability(token_id, state).await
            },
            _ => {
                return Err(AuraError::Data("Operation not yet implemented".to_string()));
            }
        }?;
        
        // Update timestamp
        state.touch();
        
        // Calculate metrics
        let end_time = self.effects.current_timestamp().await?;
        let mut metrics = FabricOpMetrics::default();
        metrics.duration_ms = end_time.saturating_sub(start_time) * 1000; // Convert to ms
        
        Ok(FabricOpResult::success(result).with_metrics(metrics))
    }
    
    async fn validate_operation(
        &self,
        op: &FabricOp,
        state: &FabricState,
    ) -> Result<(), AuraError> {
        // Check fabric limits
        match op {
            FabricOp::AddNode { .. } => {
                if state.fabric.nodes.len() >= self.config.max_nodes {
                    return Err(AuraError::Data("Maximum number of nodes exceeded".to_string()));
                }
            },
            FabricOp::AddEdge { .. } => {
                if state.fabric.edges.len() >= self.config.max_edges {
                    return Err(AuraError::Data("Maximum number of edges exceeded".to_string()));
                }
            },
            _ => {}
        }
        
        // Validate specific operations
        match op {
            FabricOp::AddNode { node } => {
                if !node.policy.is_valid() {
                    return Err(AuraError::Data("Invalid node policy".to_string()));
                }
                if state.fabric.nodes.contains_key(&node.id) {
                    return Err(AuraError::Data("Node already exists".to_string()));
                }
            },
            
            FabricOp::AddEdge { edge } => {
                if !state.fabric.nodes.contains_key(&edge.from) {
                    return Err(AuraError::Data("Source node does not exist".to_string()));
                }
                if !state.fabric.nodes.contains_key(&edge.to) {
                    return Err(AuraError::Data("Target node does not exist".to_string()));
                }
                if state.fabric.edges.contains_key(&edge.id) {
                    return Err(AuraError::Data("Edge already exists".to_string()));
                }
                
                // Check for cycles if this is a Contains edge
                if edge.kind == EdgeKind::Contains {
                    if FabricGraph::would_create_cycle(&state.fabric, edge)
                        .map_err(|e| AuraError::Data(format!("Cycle check failed: {}", e)))? {
                        return Err(AuraError::Data("Edge would create cycle".to_string()));
                    }
                }
            },
            
            FabricOp::RemoveEdge { edge } => {
                if !state.fabric.edges.contains_key(edge) {
                    return Err(AuraError::Data("Edge does not exist".to_string()));
                }
            },
            
            FabricOp::UpdateNodePolicy { node, policy } => {
                if !state.fabric.nodes.contains_key(node) {
                    return Err(AuraError::Data("Node does not exist".to_string()));
                }
                if !policy.is_valid() {
                    return Err(AuraError::Data("Invalid policy".to_string()));
                }
            },
            
            _ => {} // Other validations deferred
        }
        
        Ok(())
    }
    
    fn config(&self) -> &FabricHandlerConfig {
        &self.config
    }
}

// Private implementation methods
impl<E: FabricEffects> FabricHandlerImpl<E> {
    async fn apply_add_node(
        &self,
        mut node: KeyNode,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        // Set default crypto backends if not specified
        if node.crypto_backend == CryptoBackendId::Ed25519V1 && node.crypto_backend != self.config.default_crypto_backend {
            node.crypto_backend = self.config.default_crypto_backend.clone();
        }
        if node.hash_function == HashFunctionId::Blake3V1 && node.hash_function != self.config.default_hash_function {
            node.hash_function = self.config.default_hash_function.clone();
        }
        
        let node_id = node.id;
        let commitment = node.compute_commitment(&[]);
        
        state.fabric.add_node(node)?;
        
        Ok(Some(FabricOpData::NodeData {
            node_id,
            commitment,
        }))
    }
    
    async fn apply_add_edge(
        &self,
        edge: KeyEdge,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        let edge_id = edge.id;
        
        state.fabric.add_edge(edge)?;
        
        Ok(Some(FabricOpData::EdgeData { edge_id }))
    }
    
    async fn apply_remove_edge(
        &self,
        edge_id: EdgeId,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        state.fabric.remove_edge(edge_id)?;
        
        Ok(Some(FabricOpData::EdgeData { edge_id }))
    }
    
    async fn apply_update_policy(
        &self,
        node_id: NodeId,
        policy: NodePolicy,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        if let Some(node) = state.fabric.nodes.get_mut(&node_id) {
            node.policy = policy;
            node.epoch += 1; // Increment epoch on policy change
            
            // Clear cached secrets since policy changed
            state.clear_secrets(&node_id);
            
            let commitment = node.compute_commitment(&[]);
            
            Ok(Some(FabricOpData::NodeData {
                node_id,
                commitment,
            }))
        } else {
            Err(AuraError::Data("Node not found".to_string()))
        }
    }
    
    async fn apply_rotate_node(
        &self,
        node_id: NodeId,
        new_secret: Vec<u8>,
        new_messaging_key: Option<Vec<u8>>,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        if let Some(node) = state.fabric.nodes.get_mut(&node_id) {
            node.enc_secret = new_secret;
            node.epoch += 1;
            
            if let Some(messaging_key) = new_messaging_key {
                if node.supports_messaging() {
                    node.enc_messaging_key = Some(messaging_key);
                }
            }
            
            // Clear cached secrets for this node
            state.clear_secrets(&node_id);
            
            Ok(Some(FabricOpData::SecretData {
                node_id,
                epoch: node.epoch,
            }))
        } else {
            Err(AuraError::Data("Node not found".to_string()))
        }
    }
    
    // Share contribution operations have been moved to choreographies
    // Local share validation can be done here, but coordination must use choreographies
    
    async fn apply_grant_capability(
        &self,
        token_id: String,
        target: super::types::ResourceRef,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        state.capability_bindings.insert(token_id.clone(), target.clone());
        
        Ok(Some(FabricOpData::CapabilityData {
            token_id,
            resource: target.resource_type,
        }))
    }
    
    async fn apply_revoke_capability(
        &self,
        token_id: String,
        state: &mut FabricState,
    ) -> Result<Option<FabricOpData>, AuraError> {
        if let Some(target) = state.capability_bindings.remove(&token_id) {
            Ok(Some(FabricOpData::CapabilityData {
                token_id,
                resource: target.resource_type,
            }))
        } else {
            Err(AuraError::Data("Capability binding not found".to_string()))
        }
    }
}

// Helper trait to add metrics to operation results
trait FabricOpResultExt {
    fn with_metrics(self, metrics: FabricOpMetrics) -> Self;
}

impl FabricOpResultExt for FabricOpResult {
    fn with_metrics(mut self, metrics: FabricOpMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fabric::effects::FabricEffectsAdapter;
    use aura_types::fabric::{NodeKind, NodePolicy};
    use std::sync::Arc;
    
    #[tokio::test]
    async fn test_handler_creation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(FabricEffectsAdapter::new(device_id));
        let handler = FabricHandlerImpl::with_effects(effects);
        
        assert_eq!(handler.config().max_nodes, 10000);
        assert!(handler.config().strict_validation);
    }
    
    #[tokio::test]
    async fn test_add_node_operation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(FabricEffectsAdapter::new(device_id));
        let handler = FabricHandlerImpl::with_effects(effects);
        let mut state = FabricState::new();
        
        let node_id = NodeId::new_v4();
        let node = KeyNode::new(node_id, NodeKind::Device, NodePolicy::Any);
        let op = FabricOp::AddNode { node };
        
        let result = handler.apply_operation(op, &mut state).await.unwrap();
        assert!(result.success);
        assert!(state.fabric.nodes.contains_key(&node_id));
    }
    
    #[tokio::test]
    async fn test_add_edge_operation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(FabricEffectsAdapter::new(device_id));
        let handler = FabricHandlerImpl::with_effects(effects);
        let mut state = FabricState::new();
        
        // Add two nodes first
        let parent_id = NodeId::new_v4();
        let child_id = NodeId::new_v4();
        
        let parent = KeyNode::new(parent_id, NodeKind::Identity, NodePolicy::Threshold { m: 1, n: 1 });
        let child = KeyNode::new(child_id, NodeKind::Device, NodePolicy::Any);
        
        handler.apply_operation(FabricOp::AddNode { node: parent }, &mut state).await.unwrap();
        handler.apply_operation(FabricOp::AddNode { node: child }, &mut state).await.unwrap();
        
        // Add edge
        let edge = KeyEdge::new(parent_id, child_id, EdgeKind::Contains);
        let edge_id = edge.id;
        let op = FabricOp::AddEdge { edge };
        
        let result = handler.apply_operation(op, &mut state).await.unwrap();
        assert!(result.success);
        assert!(state.fabric.edges.contains_key(&edge_id));
    }
    
    #[tokio::test]
    async fn test_cycle_prevention() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(FabricEffectsAdapter::new(device_id));
        let handler = FabricHandlerImpl::with_effects(effects);
        let mut state = FabricState::new();
        
        // Add nodes
        let node1_id = NodeId::new_v4();
        let node2_id = NodeId::new_v4();
        
        let node1 = KeyNode::new(node1_id, NodeKind::Identity, NodePolicy::Any);
        let node2 = KeyNode::new(node2_id, NodeKind::Device, NodePolicy::Any);
        
        handler.apply_operation(FabricOp::AddNode { node: node1 }, &mut state).await.unwrap();
        handler.apply_operation(FabricOp::AddNode { node: node2 }, &mut state).await.unwrap();
        
        // Add edge 1 -> 2
        let edge1 = KeyEdge::new(node1_id, node2_id, EdgeKind::Contains);
        handler.apply_operation(FabricOp::AddEdge { edge: edge1 }, &mut state).await.unwrap();
        
        // Try to add edge 2 -> 1 (would create cycle)
        let edge2 = KeyEdge::new(node2_id, node1_id, EdgeKind::Contains);
        let op = FabricOp::AddEdge { edge: edge2 };
        
        let result = handler.validate_operation(&op, &state).await;
        assert!(result.is_err());
    }
}
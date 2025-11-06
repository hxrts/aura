//! KeyJournal Handler Implementation
//!
//! This module provides the handler implementations for KeyJournal operations,
//! following the established Aura effects/handler pattern.

use aura_types::AuraError;
use crate::journal::*;
use async_trait::async_trait;
use std::sync::Arc;

use super::{
    effects::JournalEffects,
    graph::{JournalGraph, GraphError},
    ops::{JournalOp, JournalOpResult, JournalOpData, JournalOpMetrics},
    types::{JournalState, KeyJournal},
};

/// Configuration for the journal handler
#[derive(Debug, Clone)]
pub struct JournalHandlerConfig {
    /// Maximum number of nodes allowed in the journal
    pub max_nodes: usize,
    
    /// Maximum number of edges allowed in the journal
    pub max_edges: usize,
    
    /// Whether to enable strict validation
    pub strict_validation: bool,
    
    /// Default cryptographic backend
    pub default_crypto_backend: CryptoBackendId,
    
    /// Default hash function
    pub default_hash_function: HashFunctionId,
}

impl Default for JournalHandlerConfig {
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

/// Handler trait for journal operations
#[async_trait]
pub trait JournalHandler: Send + Sync {
    /// Apply a journal operation to the state
    async fn apply_operation(
        &self,
        op: JournalOp,
        state: &mut JournalState,
    ) -> Result<JournalOpResult, AuraError>;
    
    /// Validate a journal operation before applying
    async fn validate_operation(
        &self,
        op: &JournalOp,
        state: &JournalState,
    ) -> Result<(), AuraError>;
    
    /// Get the current journal configuration
    fn config(&self) -> &JournalHandlerConfig;
}

/// Production implementation of JournalHandler
pub struct JournalHandlerImpl<E: JournalEffects> {
    effects: Arc<E>,
    config: JournalHandlerConfig,
}

impl<E: JournalEffects> JournalHandlerImpl<E> {
    /// Create a new journal handler with the given effects and config
    pub fn new(effects: Arc<E>, config: JournalHandlerConfig) -> Self {
        Self { effects, config }
    }
    
    /// Create a new journal handler with default config
    pub fn with_effects(effects: Arc<E>) -> Self {
        Self::new(effects, JournalHandlerConfig::default())
    }
}

#[async_trait]
impl<E: JournalEffects> JournalHandler for JournalHandlerImpl<E> {
    async fn apply_operation(
        &self,
        op: JournalOp,
        state: &mut JournalState,
    ) -> Result<JournalOpResult, AuraError> {
        let start_time = self.effects.current_timestamp().await?;
        
        // Validate operation first
        self.validate_operation(&op, state).await?;
        
        // Apply the operation
        let result = match op {
            JournalOp::AddNode { node } => {
                self.apply_add_node(node, state).await
            },
            JournalOp::AddEdge { edge } => {
                self.apply_add_edge(edge, state).await
            },
            JournalOp::RemoveEdge { edge } => {
                self.apply_remove_edge(edge, state).await
            },
            JournalOp::UpdateNodePolicy { node, policy } => {
                self.apply_update_policy(node, policy, state).await
            },
            JournalOp::RotateNode { node, new_secret, new_messaging_key } => {
                self.apply_rotate_node(node, new_secret, new_messaging_key, state).await
            },
            // Note: Share contribution now handled by choreographies
            JournalOp::ContributeShare { .. } => {
                Err(AuraError::Data("Share contribution must be coordinated via choreographies".to_string()))
            },
            JournalOp::GrantCapability { token_id, target } => {
                self.apply_grant_capability(token_id, target, state).await
            },
            JournalOp::RevokeCapability { token_id } => {
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
        let mut metrics = JournalOpMetrics::default();
        metrics.duration_ms = end_time.saturating_sub(start_time) * 1000; // Convert to ms
        
        Ok(JournalOpResult::success(result).with_metrics(metrics))
    }
    
    async fn validate_operation(
        &self,
        op: &JournalOp,
        state: &JournalState,
    ) -> Result<(), AuraError> {
        // Check journal limits
        match op {
            JournalOp::AddNode { .. } => {
                if state.journal.nodes.len() >= self.config.max_nodes {
                    return Err(AuraError::Data("Maximum number of nodes exceeded".to_string()));
                }
            },
            JournalOp::AddEdge { .. } => {
                if state.journal.edges.len() >= self.config.max_edges {
                    return Err(AuraError::Data("Maximum number of edges exceeded".to_string()));
                }
            },
            _ => {}
        }
        
        // Validate specific operations
        match op {
            JournalOp::AddNode { node } => {
                if !node.policy.is_valid() {
                    return Err(AuraError::Data("Invalid node policy".to_string()));
                }
                if state.journal.nodes.contains_key(&node.id) {
                    return Err(AuraError::Data("Node already exists".to_string()));
                }
            },
            
            JournalOp::AddEdge { edge } => {
                if !state.journal.nodes.contains_key(&edge.from) {
                    return Err(AuraError::Data("Source node does not exist".to_string()));
                }
                if !state.journal.nodes.contains_key(&edge.to) {
                    return Err(AuraError::Data("Target node does not exist".to_string()));
                }
                if state.journal.edges.contains_key(&edge.id) {
                    return Err(AuraError::Data("Edge already exists".to_string()));
                }
                
                // Check for cycles if this is a Contains edge
                if edge.kind == EdgeKind::Contains {
                    if JournalGraph::would_create_cycle(&state.journal, edge)
                        .map_err(|e| AuraError::Data(format!("Cycle check failed: {}", e)))? {
                        return Err(AuraError::Data("Edge would create cycle".to_string()));
                    }
                }
            },
            
            JournalOp::RemoveEdge { edge } => {
                if !state.journal.edges.contains_key(edge) {
                    return Err(AuraError::Data("Edge does not exist".to_string()));
                }
            },
            
            JournalOp::UpdateNodePolicy { node, policy } => {
                if !state.journal.nodes.contains_key(node) {
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
    
    fn config(&self) -> &JournalHandlerConfig {
        &self.config
    }
}

// Private implementation methods
impl<E: JournalEffects> JournalHandlerImpl<E> {
    async fn apply_add_node(
        &self,
        mut node: KeyNode,
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        // Set default crypto backends if not specified
        if node.crypto_backend == CryptoBackendId::Ed25519V1 && node.crypto_backend != self.config.default_crypto_backend {
            node.crypto_backend = self.config.default_crypto_backend.clone();
        }
        if node.hash_function == HashFunctionId::Blake3V1 && node.hash_function != self.config.default_hash_function {
            node.hash_function = self.config.default_hash_function.clone();
        }
        
        let node_id = node.id;
        let commitment = node.compute_commitment(&[]);
        
        state.journal.add_node(node)?;
        
        Ok(Some(JournalOpData::NodeData {
            node_id,
            commitment,
        }))
    }
    
    async fn apply_add_edge(
        &self,
        edge: KeyEdge,
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        let edge_id = edge.id;
        
        state.journal.add_edge(edge)?;
        
        Ok(Some(JournalOpData::EdgeData { edge_id }))
    }
    
    async fn apply_remove_edge(
        &self,
        edge_id: EdgeId,
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        state.journal.remove_edge(edge_id)?;
        
        Ok(Some(JournalOpData::EdgeData { edge_id }))
    }
    
    async fn apply_update_policy(
        &self,
        node_id: NodeId,
        policy: NodePolicy,
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        if let Some(node) = state.journal.nodes.get_mut(&node_id) {
            node.policy = policy;
            node.epoch += 1; // Increment epoch on policy change
            
            // Clear cached secrets since policy changed
            state.clear_secrets(&node_id);
            
            let commitment = node.compute_commitment(&[]);
            
            Ok(Some(JournalOpData::NodeData {
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
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        if let Some(node) = state.journal.nodes.get_mut(&node_id) {
            node.enc_secret = new_secret;
            node.epoch += 1;
            
            if let Some(messaging_key) = new_messaging_key {
                if node.supports_messaging() {
                    node.enc_messaging_key = Some(messaging_key);
                }
            }
            
            // Clear cached secrets for this node
            state.clear_secrets(&node_id);
            
            Ok(Some(JournalOpData::SecretData {
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
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        state.capability_bindings.insert(token_id.clone(), target.clone());
        
        Ok(Some(JournalOpData::CapabilityData {
            token_id,
            resource: target.resource_type,
        }))
    }
    
    async fn apply_revoke_capability(
        &self,
        token_id: String,
        state: &mut JournalState,
    ) -> Result<Option<JournalOpData>, AuraError> {
        if let Some(target) = state.capability_bindings.remove(&token_id) {
            Ok(Some(JournalOpData::CapabilityData {
                token_id,
                resource: target.resource_type,
            }))
        } else {
            Err(AuraError::Data("Capability binding not found".to_string()))
        }
    }
}

// Helper trait to add metrics to operation results
trait JournalOpResultExt {
    fn with_metrics(self, metrics: JournalOpMetrics) -> Self;
}

impl JournalOpResultExt for JournalOpResult {
    fn with_metrics(mut self, metrics: JournalOpMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::effects::JournalEffectsAdapter;
    use crate::journal::{NodeKind, NodePolicy};
    use std::sync::Arc;
    
    #[tokio::test]
    async fn test_handler_creation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = JournalHandlerImpl::with_effects(effects);
        
        assert_eq!(handler.config().max_nodes, 10000);
        assert!(handler.config().strict_validation);
    }
    
    #[tokio::test]
    async fn test_add_node_operation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = JournalHandlerImpl::with_effects(effects);
        let mut state = JournalState::new();
        
        let node_id = NodeId::new_v4();
        let node = KeyNode::new(node_id, NodeKind::Device, NodePolicy::Any);
        let op = JournalOp::AddNode { node };
        
        let result = handler.apply_operation(op, &mut state).await.unwrap();
        assert!(result.success);
        assert!(state.journal.nodes.contains_key(&node_id));
    }
    
    #[tokio::test]
    async fn test_add_edge_operation() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = JournalHandlerImpl::with_effects(effects);
        let mut state = JournalState::new();
        
        // Add two nodes first
        let parent_id = NodeId::new_v4();
        let child_id = NodeId::new_v4();
        
        let parent = KeyNode::new(parent_id, NodeKind::Identity, NodePolicy::Threshold { m: 1, n: 1 });
        let child = KeyNode::new(child_id, NodeKind::Device, NodePolicy::Any);
        
        handler.apply_operation(JournalOp::AddNode { node: parent }, &mut state).await.unwrap();
        handler.apply_operation(JournalOp::AddNode { node: child }, &mut state).await.unwrap();
        
        // Add edge
        let edge = KeyEdge::new(parent_id, child_id, EdgeKind::Contains);
        let edge_id = edge.id;
        let op = JournalOp::AddEdge { edge };
        
        let result = handler.apply_operation(op, &mut state).await.unwrap();
        assert!(result.success);
        assert!(state.journal.edges.contains_key(&edge_id));
    }
    
    #[tokio::test]
    async fn test_cycle_prevention() {
        let device_id = aura_types::DeviceId::new_v4();
        let effects = Arc::new(JournalEffectsAdapter::new(device_id));
        let handler = JournalHandlerImpl::with_effects(effects);
        let mut state = JournalState::new();
        
        // Add nodes
        let node1_id = NodeId::new_v4();
        let node2_id = NodeId::new_v4();
        
        let node1 = KeyNode::new(node1_id, NodeKind::Identity, NodePolicy::Any);
        let node2 = KeyNode::new(node2_id, NodeKind::Device, NodePolicy::Any);
        
        handler.apply_operation(JournalOp::AddNode { node: node1 }, &mut state).await.unwrap();
        handler.apply_operation(JournalOp::AddNode { node: node2 }, &mut state).await.unwrap();
        
        // Add edge 1 -> 2
        let edge1 = KeyEdge::new(node1_id, node2_id, EdgeKind::Contains);
        handler.apply_operation(JournalOp::AddEdge { edge: edge1 }, &mut state).await.unwrap();
        
        // Try to add edge 2 -> 1 (would create cycle)
        let edge2 = KeyEdge::new(node2_id, node1_id, EdgeKind::Contains);
        let op = JournalOp::AddEdge { edge: edge2 };
        
        let result = handler.validate_operation(&op, &state).await;
        assert!(result.is_err());
    }
}
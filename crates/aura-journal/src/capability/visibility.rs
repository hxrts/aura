// Visibility index for operation materialization based on authorization state

use crate::capability::{
    authority_graph::AuthorityGraph,
    types::{CapabilityResult, CapabilityScope, Subject},
};
use std::collections::BTreeMap;
use tracing::{debug, trace};

/// Operation visibility tracking based on capability authorization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VisibilityIndex {
    /// Authority graph for capability evaluation
    authority_graph: AuthorityGraph,
    /// Cached visibility results (operation_id -> visible)
    visibility_cache: BTreeMap<String, bool>,
    /// Last update timestamp for cache invalidation
    last_updated: u64,
}

impl VisibilityIndex {
    /// Create new visibility index
    pub fn new(authority_graph: AuthorityGraph, effects: &aura_crypto::Effects) -> Self {
        Self {
            authority_graph,
            visibility_cache: BTreeMap::new(),
            last_updated: effects.now().unwrap_or(0),
        }
    }

    /// Update the underlying authority graph
    pub fn update_authority_graph(
        &mut self,
        authority_graph: AuthorityGraph,
        effects: &aura_crypto::Effects,
    ) {
        self.authority_graph = authority_graph;
        self.invalidate_cache(effects);
    }

    /// Check if an operation should be visible/materialized
    pub fn is_operation_visible(
        &mut self,
        operation_id: &str,
        required_scope: &CapabilityScope,
        actor: &Subject,
        effects: &aura_crypto::Effects,
    ) -> bool {
        let cache_key = format!(
            "{}:{}:{}",
            operation_id,
            serde_json::to_string(required_scope).unwrap_or_default(),
            match actor {
                Subject::Device(device_id) => device_id.to_string(),
                Subject::Guardian(guardian_id) => guardian_id.to_string(),
                Subject::System => "system".to_string(),
                Subject::Generic(s) => s.clone(),
            }
        );

        // Check cache first
        if let Some(&visible) = self.visibility_cache.get(&cache_key) {
            trace!(
                "Cache hit for operation visibility: {} -> {}",
                operation_id,
                visible
            );
            return visible;
        }

        // Evaluate capability
        let result = self
            .authority_graph
            .evaluate_capability(actor, required_scope, effects);
        let visible = matches!(result, CapabilityResult::Granted);

        debug!(
            "Operation {} visibility for {}: {} (scope: {:?})",
            operation_id, 
            match actor {
                Subject::Device(device_id) => device_id.to_string(),
                Subject::Guardian(guardian_id) => guardian_id.to_string(),
                Subject::System => "system".to_string(),
                Subject::Generic(s) => s.clone(),
            }, 
            visible, required_scope
        );

        // Cache the result
        self.visibility_cache.insert(cache_key, visible);

        visible
    }

    /// Invalidate all caches
    fn invalidate_cache(&mut self, effects: &aura_crypto::Effects) {
        self.visibility_cache.clear();
        self.last_updated = effects.now().unwrap_or(0);
        debug!("Visibility index cache invalidated");
    }

    /// Record a delegation in the visibility index
    pub fn record_delegation(
        &mut self,
        capability_id: &str,
        device_id: &aura_types::DeviceId,
        permissions: &[String],
        delegated_at: u64,
        effects: &aura_crypto::Effects,
    ) -> crate::capability::Result<()> {
        // Record in underlying authority graph
        self.authority_graph.record_delegation(
            capability_id,
            device_id,
            permissions,
            delegated_at,
            effects,
        )?;
        
        // Invalidate cache since delegation state changed
        self.invalidate_cache(effects);
        
        debug!(
            "Recorded delegation {} for device {} in visibility index",
            capability_id, device_id
        );
        
        Ok(())
    }

    /// Record a revocation in the visibility index
    pub fn record_revocation(
        &mut self,
        capability_id: &str,
        device_id: &aura_types::DeviceId,
        revoked_at: u64,
        effects: &aura_crypto::Effects,
    ) -> crate::capability::Result<()> {
        // Record in underlying authority graph
        self.authority_graph.record_revocation(
            capability_id,
            device_id,
            revoked_at,
            effects,
        )?;
        
        // Invalidate cache since revocation state changed
        self.invalidate_cache(effects);
        
        debug!(
            "Recorded revocation {} for device {} in visibility index",
            capability_id, device_id
        );
        
        Ok(())
    }
}

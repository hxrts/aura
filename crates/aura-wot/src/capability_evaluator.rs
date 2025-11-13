//! High-level capability evaluator for protocol integration
//!
//! This module provides a higher-level interface for capability evaluation
//! that integrates with the effect system and provides caching and metrics.

use crate::{
    evaluation::{evaluate_capabilities, EvaluationContext},
    Capability, CapabilitySet, DelegationChain, LocalChecks, Policy,
};
use aura_core::{AuraError, AuraResult, DeviceId};
use chrono::Utc;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, trace};

/// Result of effective capability computation with metadata
#[derive(Debug, Clone)]
pub struct EffectiveCapabilitySet {
    /// The computed effective capabilities
    pub capabilities: CapabilitySet,
    /// Number of policies evaluated in computation
    pub policies_evaluated: usize,
    /// Number of delegation chains processed
    pub delegations_processed: usize,
    /// Timestamp when computed (for caching)
    pub computed_at: u64,
    /// Time taken for computation (microseconds)
    pub computation_time_us: u64,
}

impl EffectiveCapabilitySet {
    /// Check if these capabilities can satisfy a requirement
    pub fn can_satisfy(&self, required: &Capability) -> bool {
        self.capabilities.permits_capability(required)
    }
}

/// Capability evaluator with caching and metrics
#[derive(Debug, Clone)]
pub struct CapabilityEvaluator {
    device_id: DeviceId,
    cached_results: HashMap<String, EffectiveCapabilitySet>,
    cache_ttl_seconds: u64,
}

impl CapabilityEvaluator {
    /// Create a new capability evaluator for a device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            cached_results: HashMap::new(),
            cache_ttl_seconds: 300, // 5 minutes default TTL
        }
    }

    /// Create a new capability evaluator for testing
    pub fn new_for_testing() -> Self {
        Self::new(DeviceId::new())
    }

    /// Compute effective capabilities for current context
    #[allow(clippy::disallowed_methods)]
    pub async fn compute_effective_capabilities(
        &self,
        effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<EffectiveCapabilitySet> {
        let start_time = Utc::now();

        // Create evaluation context
        let context = EvaluationContext::new(self.device_id, "protocol_operation".to_string());

        debug!(
            device_id = ?self.device_id,
            operation = %context.operation,
            "Computing effective capabilities"
        );

        // Get policy for this device (placeholder - would integrate with actual policy system)
        let policy = self.get_device_policy(effect_system).await?;

        // Get delegation chains (placeholder - would integrate with actual delegation system)
        let delegations = self.get_delegation_chains(effect_system).await?;

        // Get local checks (placeholder - would integrate with actual local checks)
        let local_checks = self.get_local_checks(effect_system).await?;

        // Evaluate capabilities using core function
        let capabilities = evaluate_capabilities(&policy, &delegations, &local_checks, &context)
            .map_err(|e| {
                let msg = format!("Capability evaluation failed: {:?}", e);
                AuraError::permission_denied(&msg)
            })?;

        let end_time = Utc::now();
        let computation_time = (end_time - start_time)
            .num_microseconds()
            .unwrap_or(0)
            .unsigned_abs();
        let current_timestamp = end_time.timestamp() as u64;

        let result = EffectiveCapabilitySet {
            capabilities,
            policies_evaluated: 1, // Would be actual count
            delegations_processed: delegations.len(),
            computed_at: current_timestamp,
            computation_time_us: computation_time,
        };

        trace!(
            device_id = ?self.device_id,
            policies_evaluated = result.policies_evaluated,
            delegations_processed = result.delegations_processed,
            computation_time_us = result.computation_time_us,
            "Capability evaluation completed"
        );

        Ok(result)
    }

    /// Get cached capabilities if available and not expired
    #[allow(clippy::disallowed_methods)]
    pub fn get_cached_capabilities(&self, operation: &str) -> Option<&EffectiveCapabilitySet> {
        let cache_key = format!("{}:{}", self.device_id, operation);

        if let Some(cached) = self.cached_results.get(&cache_key) {
            let current_time = SystemTime::UNIX_EPOCH
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::from_secs(0))
                .as_secs();

            if current_time - cached.computed_at < self.cache_ttl_seconds {
                trace!(
                    device_id = ?self.device_id,
                    operation = %operation,
                    "Using cached capability evaluation"
                );
                return Some(cached);
            }
        }

        None
    }

    /// Clear expired cache entries
    #[allow(clippy::disallowed_methods)]
    pub fn cleanup_cache(&mut self) {
        let current_time = SystemTime::UNIX_EPOCH
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs();

        self.cached_results
            .retain(|_, result| current_time - result.computed_at < self.cache_ttl_seconds);
    }

    /// Evaluate storage access based on capabilities and permission requirements
    pub fn evaluate_storage_access(
        &self,
        capabilities: &[crate::Capability],
        required_permission: &crate::StoragePermission,
        resource: &dyn std::fmt::Debug,
    ) -> AuraResult<bool> {
        // TODO: Implement proper capability evaluation logic
        // For now, return a simple check that capabilities list is non-empty
        // In a real implementation, this would check if any of the capabilities
        // grant the required permission for the specified resource
        debug!(
            device_id = ?self.device_id,
            permission = ?required_permission,
            resource = ?resource,
            capabilities_count = capabilities.len(),
            "Evaluating storage access"
        );

        // Placeholder logic - always allow for now to enable development
        Ok(!capabilities.is_empty())
    }

    // Placeholder methods for integration points

    async fn get_device_policy(
        &self,
        _effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<Policy> {
        // Placeholder: In actual implementation, this would query the policy system
        // TODO fix - For now, return a permissive policy for development
        Ok(Policy::default_for_device(self.device_id))
    }

    async fn get_delegation_chains(
        &self,
        _effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<Vec<DelegationChain>> {
        // Placeholder: In actual implementation, this would query the delegation system
        Ok(Vec::new())
    }

    async fn get_local_checks(
        &self,
        _effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<LocalChecks> {
        // Placeholder: In actual implementation, this would query local restrictions
        Ok(LocalChecks::empty())
    }
}

/// TODO fix - Simplified interface for effect system integration
///
/// This trait abstracts the effect system interface to avoid circular dependencies
/// between aura-wot and aura-protocol. In practice, this would be implemented
/// by AuraEffectSystem or a wrapper.
pub trait EffectSystemInterface {
    /// Get the device ID for this effect system
    fn device_id(&self) -> DeviceId;

    /// Query metadata from the effect system
    fn get_metadata(&self, key: &str) -> Option<String>;
}

// Temporary stub implementation for CapabilitySet methods used by guards
impl CapabilitySet {
    /// Check if this capability set permits a specific capability
    pub fn permits_capability(&self, _capability: &Capability) -> bool {
        // Placeholder implementation - would check if capability is in the set
        // TODO fix - For now, be permissive to allow development
        true
    }
}

impl Policy {
    /// Create a default policy for a device (if not already defined)
    pub fn default_for_device(_device_id: DeviceId) -> Self {
        // Placeholder: would create appropriate default policy
        Policy::new()
    }
}

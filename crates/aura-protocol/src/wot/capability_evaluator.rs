//! High-level capability evaluator for protocol integration
//!
//! **Layer 4 (aura-protocol)**: Orchestration logic with effect system integration.
//!
//! This module was moved from aura-wot (Layer 2) because it contains effect-dependent
//! orchestration logic (async methods, caching, effect system integration) that violates
//! Layer 2's principle of "semantics without implementation."
//!
//! The pure capability evaluation logic (`evaluate_capabilities`) remains in aura-wot
//! as domain semantics.

use aura_core::{AuraError, AuraResult, DeviceId};
use aura_wot::{
    evaluation::{evaluate_capabilities, EvaluationContext, LocalChecks},
    Capability, CapabilitySet, DelegationChain, Policy,
};
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
        self.capabilities
            .capabilities()
            .any(|cap| cap.implies(required))
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

        // Evaluate capabilities using core function from aura-wot
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

        trace!(
            device_id = ?self.device_id,
            policies_evaluated = 1,
            delegations_processed = delegations.len(),
            computation_time_us = computation_time,
            "Capability computation complete"
        );

        Ok(EffectiveCapabilitySet {
            capabilities,
            policies_evaluated: 1,
            delegations_processed: delegations.len(),
            computed_at: current_timestamp,
            computation_time_us: computation_time,
        })
    }

    /// Compute effective capabilities with caching
    pub async fn compute_effective_capabilities_cached(
        &mut self,
        cache_key: String,
        effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<EffectiveCapabilitySet> {
        // Check cache
        if let Some(cached) = self.cached_results.get(&cache_key) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            if now - cached.computed_at < self.cache_ttl_seconds {
                debug!(
                    cache_key = %cache_key,
                    "Using cached capability evaluation"
                );
                return Ok(cached.clone());
            }
        }

        // Compute fresh
        let result = self.compute_effective_capabilities(effect_system).await?;

        // Update cache
        self.cached_results.insert(cache_key, result.clone());

        Ok(result)
    }

    /// Set cache TTL (time-to-live) in seconds
    pub fn set_cache_ttl(&mut self, ttl_seconds: u64) {
        self.cache_ttl_seconds = ttl_seconds;
    }

    /// Clear capability cache
    pub fn clear_cache(&mut self) {
        self.cached_results.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        CacheStats {
            total_entries: self.cached_results.len(),
            ttl_seconds: self.cache_ttl_seconds,
        }
    }

    // Placeholder integration methods - would be replaced with real effect system integration

    async fn get_device_policy(
        &self,
        _effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<Policy> {
        // Placeholder: In actual implementation, this would query the policy from storage
        Ok(Policy::new())
    }

    async fn get_delegation_chains(
        &self,
        _effect_system: &dyn EffectSystemInterface,
    ) -> AuraResult<Vec<DelegationChain>> {
        // Placeholder: In actual implementation, this would query delegation chains
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

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of cached entries
    pub total_entries: usize,
    /// Cache TTL in seconds
    pub ttl_seconds: u64,
}

/// Effect system interface for capability evaluation
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

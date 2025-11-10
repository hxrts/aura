//! Guard evaluation logic integrating with aura-wot capability system
//!
//! This module implements the `need(σ) ≤ C` checking required by the formal model.
//! It bridges the protocol layer with the capability calculus implemented in aura-wot.

use super::{ExecutionMetrics, ProtocolGuard};
use crate::effects::system::AuraEffectSystem;
use aura_core::{AuraError, AuraResult, DeviceId};
use aura_wot::{Capability, CapabilityEvaluator, EffectSystemInterface, EffectiveCapabilitySet};
use std::time::Instant;
use tracing::{debug, info, warn};

/// Guard evaluation result
#[derive(Debug)]
pub struct GuardEvaluationResult {
    /// Whether all capability guards passed
    pub passed: bool,
    /// Effective capabilities computed for the current context
    pub effective_capabilities: EffectiveCapabilitySet,
    /// Failed capability requirements (empty if passed)
    pub failed_requirements: Vec<Capability>,
    /// Evaluation metrics
    pub metrics: GuardEvaluationMetrics,
}

/// Metrics for guard evaluation
#[derive(Debug, Default)]
pub struct GuardEvaluationMetrics {
    /// Time to compute effective capabilities (microseconds)
    pub capability_computation_time_us: u64,
    /// Time to check requirements (microseconds)
    pub requirement_check_time_us: u64,
    /// Number of policies evaluated
    pub policies_evaluated: usize,
    /// Number of delegations processed
    pub delegations_processed: usize,
}

/// Guard evaluator that integrates with the capability system
pub struct GuardEvaluator {
    capability_evaluator: CapabilityEvaluator,
    device_id: DeviceId,
}

impl GuardEvaluator {
    /// Create a new guard evaluator for a specific device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            capability_evaluator: CapabilityEvaluator::new(device_id),
            device_id,
        }
    }

    /// Evaluate protocol guards against current capabilities
    pub async fn evaluate_guards(
        &self,
        guard: &ProtocolGuard,
        effect_system: &AuraEffectSystem,
    ) -> AuraResult<GuardEvaluationResult> {
        let start_time = Instant::now();

        debug!(
            operation_id = %guard.operation_id,
            required_capabilities = guard.required_capabilities.len(),
            "Starting guard evaluation"
        );

        // Compute effective capabilities for current context
        let capability_start = Instant::now();
        let effective_capabilities = self
            .capability_evaluator
            .compute_effective_capabilities(effect_system)
            .await
            .map_err(|e| {
                AuraError::permission_denied(&format!("Failed to compute capabilities: {}", e))
            })?;
        let capability_time = capability_start.elapsed();

        // Check each required capability against effective capabilities
        let requirement_start = Instant::now();
        let mut failed_requirements = Vec::new();

        for required_cap in &guard.required_capabilities {
            if !self.check_capability_satisfied(required_cap, &effective_capabilities) {
                failed_requirements.push(required_cap.clone());
                warn!(
                    operation_id = %guard.operation_id,
                    capability = ?required_cap,
                    "Capability requirement not satisfied"
                );
            } else {
                debug!(
                    operation_id = %guard.operation_id,
                    capability = ?required_cap,
                    "Capability requirement satisfied"
                );
            }
        }
        let requirement_time = requirement_start.elapsed();

        let passed = failed_requirements.is_empty();
        let total_time = start_time.elapsed();

        let metrics = GuardEvaluationMetrics {
            capability_computation_time_us: capability_time.as_micros() as u64,
            requirement_check_time_us: requirement_time.as_micros() as u64,
            policies_evaluated: effective_capabilities.policies_evaluated,
            delegations_processed: effective_capabilities.delegations_processed,
        };

        if passed {
            info!(
                operation_id = %guard.operation_id,
                evaluation_time_us = total_time.as_micros(),
                "All guards passed"
            );
        } else {
            warn!(
                operation_id = %guard.operation_id,
                failed_count = failed_requirements.len(),
                evaluation_time_us = total_time.as_micros(),
                "Guard evaluation failed"
            );
        }

        Ok(GuardEvaluationResult {
            passed,
            effective_capabilities,
            failed_requirements,
            metrics,
        })
    }

    /// Check if a specific capability is satisfied by effective capabilities
    fn check_capability_satisfied(
        &self,
        required: &Capability,
        effective: &EffectiveCapabilitySet,
    ) -> bool {
        // Use meet-semilattice operation to check if requirement is satisfied
        // This implements the `need(σ) ≤ C` check from the formal model
        effective.can_satisfy(required)
    }

    /// Batch evaluate multiple guards (optimization for complex protocols)
    pub async fn evaluate_guards_batch(
        &self,
        guards: &[&ProtocolGuard],
        effect_system: &AuraEffectSystem,
    ) -> AuraResult<Vec<GuardEvaluationResult>> {
        // Optimize by computing effective capabilities once
        let effective_capabilities = self
            .capability_evaluator
            .compute_effective_capabilities(effect_system)
            .await
            .map_err(|e| {
                AuraError::permission_denied(&format!("Failed to compute capabilities: {}", e))
            })?;

        let mut results = Vec::new();

        for guard in guards {
            let start_time = Instant::now();

            let mut failed_requirements = Vec::new();
            for required_cap in &guard.required_capabilities {
                if !self.check_capability_satisfied(required_cap, &effective_capabilities) {
                    failed_requirements.push(required_cap.clone());
                }
            }

            let passed = failed_requirements.is_empty();
            let evaluation_time = start_time.elapsed();

            results.push(GuardEvaluationResult {
                passed,
                effective_capabilities: effective_capabilities.clone(),
                failed_requirements,
                metrics: GuardEvaluationMetrics {
                    capability_computation_time_us: 0, // Shared computation
                    requirement_check_time_us: evaluation_time.as_micros() as u64,
                    policies_evaluated: effective_capabilities.policies_evaluated,
                    delegations_processed: effective_capabilities.delegations_processed,
                },
            });
        }

        Ok(results)
    }
}

/// Create a guard evaluator from an effect system
pub async fn create_guard_evaluator(
    effect_system: &AuraEffectSystem,
) -> AuraResult<GuardEvaluator> {
    let device_id = effect_system.device_id();
    Ok(GuardEvaluator::new(device_id))
}

/// Convenience function to evaluate a single guard
pub async fn evaluate_guard(
    guard: &ProtocolGuard,
    effect_system: &AuraEffectSystem,
) -> AuraResult<GuardEvaluationResult> {
    let evaluator = create_guard_evaluator(effect_system).await?;
    evaluator.evaluate_guards(guard, effect_system).await
}

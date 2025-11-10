//! Guarded protocol execution orchestrating guards, deltas, and privacy tracking
//!
//! This module implements the complete execution pipeline for capability-guarded protocols:
//! 1. Evaluate capability guards before execution
//! 2. Execute the protocol operation
//! 3. Apply delta facts atomically
//! 4. Track privacy budget consumption
//! 5. Return comprehensive execution results

use super::{
    deltas::apply_delta_facts,
    evaluation::{evaluate_guard, GuardEvaluationResult},
    privacy::track_leakage_consumption,
    ExecutionMetrics, GuardedExecutionResult, LeakageBudget, ProtocolGuard,
};
use crate::effects::system::AuraEffectSystem;
use aura_core::{AuraError, AuraResult};
use std::future::Future;
use std::time::Instant;
use tracing::{debug, error, info, warn, Instrument};

/// Execute a protocol operation with full guard enforcement
///
/// This function implements the complete operational semantics from the formal model:
/// - Meet-guarded preconditions: Check `need(σ) ≤ C` before execution
/// - Join-only commits: Apply `merge_facts(Δfacts)` after successful execution
/// - Privacy tracking: Account for leakage budget consumption
///
/// The execution is atomic: either all guards pass and execution succeeds with
/// delta application, or the operation fails with no side effects.
pub async fn execute_guarded_operation<T, F, Fut>(
    guard: &ProtocolGuard,
    effect_system: &mut AuraEffectSystem,
    operation: F,
) -> AuraResult<GuardedExecutionResult<T>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AuraResult<T>>,
{
    let total_start_time = Instant::now();
    let span = tracing::info_span!("guarded_execution", operation_id = %guard.operation_id);

    async move {
        debug!("Starting guarded protocol execution");

        // Phase 1: Evaluate capability guards (meet-guarded preconditions)
        let guard_start_time = Instant::now();
        let guard_result = evaluate_guard(guard, effect_system).await?;
        let guard_eval_time = guard_start_time.elapsed();

        if !guard_result.passed {
            warn!(
                failed_capabilities = guard_result.failed_requirements.len(),
                "Guard evaluation failed, blocking execution"
            );

            return Err(AuraError::permission_denied(&format!(
                "Operation '{}' blocked: {} capability requirements not satisfied",
                guard.operation_id,
                guard_result.failed_requirements.len()
            )));
        }

        info!("All guards passed, proceeding with execution");

        // Phase 2: Execute the protocol operation
        let execution_start_time = Instant::now();
        let execution_result = operation().await;
        let execution_time = execution_start_time.elapsed();

        match execution_result {
            Ok(result) => {
                debug!("Protocol execution succeeded, applying deltas");

                // Phase 3: Apply delta facts (join-only commits)
                let delta_start_time = Instant::now();
                let applied_deltas = if !guard.delta_facts.is_empty() {
                    apply_delta_facts(&guard.delta_facts, effect_system)
                        .await
                        .map_err(|e| {
                            error!(error = %e, "Failed to apply delta facts");
                            AuraError::internal(&format!(
                                "Delta application failed for operation '{}': {}",
                                guard.operation_id, e
                            ))
                        })?
                } else {
                    Vec::new()
                };
                let delta_apply_time = delta_start_time.elapsed();

                // Phase 4: Track privacy budget consumption
                let consumed_budget = track_leakage_consumption(
                    &guard.leakage_budget,
                    &guard.operation_id,
                    effect_system,
                )
                .await?;

                let total_time = total_start_time.elapsed();

                let metrics = ExecutionMetrics {
                    guard_eval_time_us: guard_eval_time.as_micros() as u64,
                    delta_apply_time_us: delta_apply_time.as_micros() as u64,
                    total_execution_time_us: total_time.as_micros() as u64,
                    capabilities_checked: guard.required_capabilities.len(),
                    facts_applied: applied_deltas.len(),
                };

                info!(
                    execution_time_us = total_time.as_micros(),
                    facts_applied = applied_deltas.len(),
                    "Guarded execution completed successfully"
                );

                Ok(GuardedExecutionResult {
                    result,
                    guards_passed: true,
                    applied_deltas,
                    consumed_budget,
                    metrics,
                })
            }
            Err(e) => {
                warn!(
                    error = %e,
                    execution_time_us = execution_time.as_micros(),
                    "Protocol execution failed, no deltas applied"
                );

                // No delta application on failure - maintain consistency
                let total_time = total_start_time.elapsed();

                let metrics = ExecutionMetrics {
                    guard_eval_time_us: guard_eval_time.as_micros() as u64,
                    delta_apply_time_us: 0, // No deltas applied on failure
                    total_execution_time_us: total_time.as_micros() as u64,
                    capabilities_checked: guard.required_capabilities.len(),
                    facts_applied: 0,
                };

                // Still consume privacy budget even on failure (information leakage occurred)
                let consumed_budget = track_leakage_consumption(
                    &guard.leakage_budget,
                    &guard.operation_id,
                    effect_system,
                )
                .await
                .unwrap_or_else(|_| guard.leakage_budget.clone());

                // Return the original error, not a GuardedExecutionResult
                Err(e)
            }
        }
    }
    .instrument(span)
    .await
}

/// Execute multiple guarded operations in sequence with rollback on failure
///
/// This provides transaction-like semantics for complex protocols that require
/// multiple guarded steps. If any step fails, no delta facts are applied.
pub async fn execute_guarded_sequence<T>(
    guards_and_operations: Vec<(
        ProtocolGuard,
        Box<dyn FnOnce() -> std::pin::Pin<Box<dyn Future<Output = AuraResult<T>> + Send>> + Send>,
    )>,
    effect_system: &mut AuraEffectSystem,
) -> AuraResult<Vec<GuardedExecutionResult<T>>> {
    let sequence_start = Instant::now();
    let span = tracing::info_span!("guarded_sequence", operations = guards_and_operations.len());

    async move {
        debug!(
            operations_count = guards_and_operations.len(),
            "Starting guarded sequence execution"
        );

        // Phase 1: Evaluate all guards first (fail fast)
        let mut all_guard_results = Vec::new();
        for (guard, _) in &guards_and_operations {
            let guard_result = evaluate_guard(guard, effect_system).await?;
            if !guard_result.passed {
                warn!(
                    operation_id = %guard.operation_id,
                    "Sequence blocked by failed guard"
                );
                return Err(AuraError::permission_denied(&format!(
                    "Sequence blocked: operation '{}' failed guard evaluation",
                    guard.operation_id
                )));
            }
            all_guard_results.push(guard_result);
        }

        info!("All sequence guards passed, executing operations");

        // Phase 2: Execute all operations
        let mut results = Vec::new();
        let mut all_deltas = Vec::new();

        for (guard, operation) in guards_and_operations {
            let execution_start = Instant::now();
            let result = operation().await;
            let execution_time = execution_start.elapsed();

            match result {
                Ok(value) => {
                    // Collect deltas but don't apply yet
                    all_deltas.extend(guard.delta_facts.clone());

                    let consumed_budget = track_leakage_consumption(
                        &guard.leakage_budget,
                        &guard.operation_id,
                        effect_system,
                    )
                    .await?;

                    results.push(GuardedExecutionResult {
                        result: value,
                        guards_passed: true,
                        applied_deltas: guard.delta_facts.clone(),
                        consumed_budget,
                        metrics: ExecutionMetrics {
                            guard_eval_time_us: 0,  // Computed in batch
                            delta_apply_time_us: 0, // Applied in batch
                            total_execution_time_us: execution_time.as_micros() as u64,
                            capabilities_checked: guard.required_capabilities.len(),
                            facts_applied: guard.delta_facts.len(),
                        },
                    });
                }
                Err(e) => {
                    error!(
                        operation_id = %guard.operation_id,
                        error = %e,
                        "Sequence operation failed, rolling back"
                    );

                    // Rollback: don't apply any deltas from the sequence
                    return Err(e);
                }
            }
        }

        // Phase 3: Apply all deltas atomically
        if !all_deltas.is_empty() {
            apply_delta_facts(&all_deltas, effect_system)
                .await
                .map_err(|e| {
                    error!(error = %e, "Failed to apply sequence deltas");
                    AuraError::internal(&format!("Sequence delta application failed: {}", e))
                })?;
        }

        let total_time = sequence_start.elapsed();
        info!(
            sequence_time_us = total_time.as_micros(),
            operations_completed = results.len(),
            total_deltas_applied = all_deltas.len(),
            "Guarded sequence completed successfully"
        );

        Ok(results)
    }
    .instrument(span)
    .await
}

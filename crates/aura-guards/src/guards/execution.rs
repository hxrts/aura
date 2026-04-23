//! Guarded protocol execution orchestrating guards, deltas, and privacy tracking
//!
//! This module implements the complete execution pipeline for capability-guarded protocols:
//! 1. Evaluate capability guards before execution
//! 2. Execute the protocol operation
//! 3. Apply delta facts atomically
//! 4. Track privacy budget consumption
//! 5. Return comprehensive execution results

use super::{
    deltas::apply_delta_facts, privacy::track_leakage_consumption, traits::GuardContextProvider,
    BiscuitGuardEvaluator, ExecutionMetrics, GuardEffects, GuardError, GuardResult,
    GuardedExecutionResult, ProtocolGuard,
};
use crate::authorization::BiscuitAuthorizationBridge;
use crate::guards::types::CapabilityId;
use aura_authorization::{AuthorityOp, ResourceScope, VerifiedBiscuitToken};
use aura_core::{types::Epoch, AuraError, AuraResult, FlowBudget, FlowCost};
use std::future::Future;
use tracing::{debug, error, info, instrument, warn};

/// Evaluate protocol guard using Biscuit authorization
///
/// This function implements the `need(σ) ≤ C` checking from the formal model
/// using Biscuit tokens for cryptographically verifiable authorization.
pub async fn evaluate_guard(
    guard: &ProtocolGuard,
    current_time_seconds: u64,
) -> Result<GuardEvaluationResult, AuraError> {
    debug!(
        operation_id = %guard.operation_id,
        required_tokens = guard.required_tokens.len(),
        "Evaluating protocol guard with Biscuit tokens"
    );

    match &guard.authorization_policy {
        super::ProtocolGuardRequirement::UnauthenticatedAllowed(policy) => {
            info!(
                operation_id = %guard.operation_id,
                reason_code = %policy.reason_code,
                scope = %policy.scope,
                doc_ref = %policy.doc_ref,
                "Guard operation explicitly allowed without authentication"
            );
            return Ok(GuardEvaluationResult {
                passed: true,
                failed_requirements: Vec::new(),
                delegation_depth: None,
                flow_consumed: 0,
                unauthenticated_allowed: true,
            });
        }
        super::ProtocolGuardRequirement::RequiredTokens(_) => {
            if guard.required_tokens.is_empty() {
                warn!("No authorization tokens configured, denying guarded operation");
                return Ok(GuardEvaluationResult {
                    passed: false,
                    failed_requirements: vec![
                        "guard has no Biscuit authorization tokens configured".to_string(),
                    ],
                    delegation_depth: None,
                    flow_consumed: 0,
                    unauthenticated_allowed: false,
                });
            }
        }
    }

    // Get authorization bridge from effect system for Biscuit token verification
    // Authorization bridge derived from protocol guard context
    let operation_id = guard.operation_id.to_string();
    let auth_bridge = BiscuitAuthorizationBridge::for_guard(
        guard.root_public_key,
        guard.authority_id,
        &operation_id,
    );
    let evaluator = BiscuitGuardEvaluator::new(auth_bridge);
    let mut failed_requirements = Vec::new();
    let mut total_flow_consumed = 0;
    let mut max_delegation_depth: Option<u32> = None;
    let mut context = GuardVerificationContext::new(
        CapabilityId::try_from(guard.operation_id.to_string())
            .expect("guard operation ids use valid capability grammar"),
        ResourceScope::Authority {
            authority_id: guard.authority_id,
            operation: AuthorityOp::UpdateTree,
        },
        FlowCost::from(1),
        FlowBudget::new(guard.required_tokens.len() as u64 + 1, Epoch::new(0)),
    );

    for (idx, token) in guard.required_tokens.iter().enumerate() {
        debug!(token_idx = idx, "Evaluating Biscuit token requirement");

        // Evaluate already-verified token evidence through the guard policy.
        match verify_biscuit_token(token, &evaluator, &mut context, current_time_seconds) {
            Ok(result) => {
                debug!(
                    token_idx = idx,
                    delegation_depth = ?result.delegation_depth,
                    flow_consumed = result.flow_consumed,
                    "Biscuit token requirement satisfied"
                );
                total_flow_consumed += result.flow_consumed;
                if let Some(depth) = result.delegation_depth {
                    max_delegation_depth =
                        Some(max_delegation_depth.map_or(depth, |current| current.max(depth)));
                }
            }
            Err(error) => {
                warn!(
                    token_idx = idx,
                    error = %error,
                    "Biscuit token requirement failed"
                );
                failed_requirements.push(format!("Token {idx}: {error}"));
            }
        }
    }

    let passed = failed_requirements.is_empty();

    if passed {
        info!(
            operation_id = %guard.operation_id,
            flow_consumed = total_flow_consumed,
            delegation_depth = ?max_delegation_depth,
            "All guard requirements satisfied"
        );
    } else {
        warn!(
            operation_id = %guard.operation_id,
            failed_count = failed_requirements.len(),
            "Guard evaluation failed"
        );
    }

    Ok(GuardEvaluationResult {
        passed,
        failed_requirements,
        delegation_depth: max_delegation_depth,
        flow_consumed: total_flow_consumed,
        unauthenticated_allowed: false,
    })
}

/// Context for guard token verification
///
/// This struct provides the necessary context for verifying Biscuit tokens,
/// including the capability being exercised and the resource scope.
#[derive(Debug, Clone)]
pub struct GuardVerificationContext {
    /// The capability being exercised (e.g., "send_message", "execute_operation")
    pub capability: CapabilityId,
    /// The resource scope for authorization
    pub resource_scope: ResourceScope,
    /// Flow cost for this operation
    pub flow_cost: FlowCost,
    /// Available flow budget
    pub flow_budget: FlowBudget,
}

impl GuardVerificationContext {
    /// Create a new verification context
    pub fn new(
        capability: CapabilityId,
        resource_scope: ResourceScope,
        flow_cost: FlowCost,
        flow_budget: FlowBudget,
    ) -> Self {
        Self {
            capability,
            resource_scope,
            flow_cost,
            flow_budget,
        }
    }
}

/// Verify Biscuit token against an explicit capability/scope context.
fn verify_biscuit_token(
    token: &VerifiedBiscuitToken,
    evaluator: &BiscuitGuardEvaluator,
    context: &mut GuardVerificationContext,
    current_time_seconds: u64,
) -> Result<GuardResult, GuardError> {
    evaluator.evaluate_guard(
        token,
        &context.capability,
        &context.resource_scope,
        context.flow_cost,
        &mut context.flow_budget,
        current_time_seconds,
    )
}

// NOTE: Legacy `parse_and_verify_biscuit_token` and `create_biscuit_token` functions
// have been removed. They were dead code that created tokens with random keypairs,
// which is insecure. Token creation and verification should use:
//   - GuardVerificationContext for explicit verification context
//   - Effect system metadata for token retrieval (see send_guard.rs)
//   - BiscuitTokenManager from aura-authorization for proper token management

/// Guard evaluation results using Biscuit authorization
#[derive(Debug)]
pub struct GuardEvaluationResult {
    pub passed: bool,
    pub failed_requirements: Vec<String>,
    pub delegation_depth: Option<u32>,
    pub flow_consumed: u64,
    pub unauthenticated_allowed: bool,
}

/// Execute a protocol operation with full guard enforcement
///
/// This function implements the complete operational semantics from the formal model:
/// - Meet-guarded preconditions: Check `need(σ) ≤ C` before execution
/// - Join-only commits: Apply `merge_facts(Δfacts)` after successful execution
/// - Privacy tracking: Account for leakage budget consumption
///
/// The execution is atomic: either all guards pass and execution succeeds with
/// delta application, or the operation fails with no side effects.
///
/// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
#[instrument(skip(effect_system, operation), fields(operation_id = %guard.operation_id))]
pub async fn execute_guarded_operation<E, T, F, Fut>(
    guard: &ProtocolGuard,
    effect_system: &mut E,
    operation: F,
) -> AuraResult<GuardedExecutionResult<T>>
where
    E: GuardEffects + aura_core::TimeEffects + GuardContextProvider,
    F: FnOnce(&mut E) -> Fut,
    Fut: Future<Output = AuraResult<T>>,
{
    debug!("Starting guarded protocol execution");
    let operation_id = guard.operation_id.to_string();

    // Phase 1: Evaluate capability guards (meet-guarded preconditions)
    // Evaluate capability guards using Biscuit authorization
    let current_time_seconds = effect_system
        .physical_time()
        .await
        .map_err(|error| AuraError::internal(format!("guard time unavailable: {error}")))?
        .ts_ms
        / 1000;
    let guard_result = evaluate_guard(guard, current_time_seconds).await?;

    // Guard result now comes from actual Biscuit evaluation
    if !guard_result.passed {
        warn!(
            failed_capabilities = guard_result.failed_requirements.len(),
            "Guard evaluation failed, blocking execution"
        );

        return Err(AuraError::permission_denied(format!(
            "Operation '{}' blocked: {} capability requirements not satisfied",
            operation_id,
            guard_result.failed_requirements.len()
        )));
    }

    info!("All guards passed, proceeding with execution");

    // Phase 2: Execute the protocol operation
    let execution_result = operation(effect_system).await;

    match execution_result {
        Ok(result) => {
            debug!("Protocol execution succeeded, applying deltas");

            // Phase 3: Apply delta facts (join-only commits)
            let applied_deltas = if !guard.delta_facts.is_empty() {
                apply_delta_facts(&guard.delta_facts, effect_system)
                    .await
                    .map_err(|e| {
                        error!(error = %e, "Failed to apply delta facts");
                        AuraError::internal(format!(
                            "Delta application failed for operation '{operation_id}': {e}"
                        ))
                    })?
            } else {
                Vec::new()
            };

            // Phase 4: Track privacy budget consumption
            let consumed_budget = track_leakage_consumption(
                guard.context_id,
                None,
                &guard.leakage_budget,
                &operation_id,
                guard.observable_by.clone(),
                effect_system,
            )
            .await?;

            // Timing captured by tracing span, not explicit measurement
            let metrics = ExecutionMetrics {
                guard_eval_time_us: 0,
                delta_apply_time_us: 0,
                total_execution_time_us: 0,
                authorization_checks: guard.required_tokens.len() as u32,
                unauthenticated_allowed: guard_result.unauthenticated_allowed,
                facts_applied: applied_deltas.len() as u32,
            };

            info!(
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
                "Protocol execution failed, no deltas applied"
            );

            // No delta application on failure - maintain consistency
            // Timing captured by tracing span, not explicit measurement
            let metrics = ExecutionMetrics {
                guard_eval_time_us: 0,
                delta_apply_time_us: 0,
                total_execution_time_us: 0,
                authorization_checks: guard.required_tokens.len() as u32,
                unauthenticated_allowed: guard_result.unauthenticated_allowed,
                facts_applied: 0,
            };

            // Still consume privacy budget even on failure (information leakage occurred)
            let _consumed_budget = track_leakage_consumption(
                guard.context_id,
                None,
                &guard.leakage_budget,
                &operation_id,
                guard.observable_by.clone(),
                effect_system,
            )
            .await
            .unwrap_or_else(|_| guard.leakage_budget.clone());

            // Return the original error, not a GuardedExecutionResult
            Err(e)
        }
    }
}

/// Execute multiple guarded operations in sequence with rollback on failure
///
/// This provides transaction-like semantics for complex protocols that require
/// multiple guarded steps. If any step fails, no delta facts are applied.
///
/// Timing is captured via the tracing span (subscriber handles `Instant::now()`).
#[instrument(skip(effect_system, guards_and_operations))]
pub async fn execute_guarded_sequence<E, T>(
    guards_and_operations: Vec<(
        ProtocolGuard,
        Box<
            dyn FnOnce(&mut E) -> std::pin::Pin<Box<dyn Future<Output = AuraResult<T>> + Send>>
                + Send,
        >,
    )>,
    effect_system: &mut E,
) -> AuraResult<Vec<GuardedExecutionResult<T>>>
where
    E: GuardEffects + aura_core::TimeEffects + GuardContextProvider,
{
    debug!(
        operations_count = guards_and_operations.len(),
        "Starting guarded sequence execution"
    );

    // Phase 1: Evaluate all guards first (fail fast)
    let mut all_guard_results = Vec::new();
    for (guard, _) in &guards_and_operations {
        // Evaluate guard using Biscuit integration
        let current_time_seconds = effect_system
            .physical_time()
            .await
            .map_err(|error| AuraError::internal(format!("guard time unavailable: {error}")))?
            .ts_ms
            / 1000;
        let guard_result = evaluate_guard(guard, current_time_seconds).await?;
        if !guard_result.passed {
            warn!(
                operation_id = %guard.operation_id,
                failed_requirements = guard_result.failed_requirements.len(),
                "Sequence blocked by failed guard"
            );
            return Err(AuraError::permission_denied(format!(
                "Sequence blocked: operation '{}' failed guard evaluation: {:?}",
                guard.operation_id, guard_result.failed_requirements
            )));
        }
        all_guard_results.push(guard_result);
    }

    info!("All sequence guards passed, executing operations");

    // Phase 2: Execute all operations
    let mut results = Vec::new();
    let mut all_deltas = Vec::new();

    for (guard, operation) in guards_and_operations {
        let result = operation(effect_system).await;

        match result {
            Ok(value) => {
                // Collect deltas but don't apply yet
                all_deltas.extend(guard.delta_facts.clone());

                let consumed_budget = track_leakage_consumption(
                    guard.context_id,
                    None,
                    &guard.leakage_budget,
                    &guard.operation_id.to_string(),
                    guard.observable_by.clone(),
                    effect_system,
                )
                .await?;

                results.push(GuardedExecutionResult {
                    result: value,
                    guards_passed: true,
                    applied_deltas: guard.delta_facts.clone(),
                    consumed_budget,
                    metrics: ExecutionMetrics {
                        // Timing captured by tracing span, not explicit measurement
                        guard_eval_time_us: 0,
                        delta_apply_time_us: 0,
                        total_execution_time_us: 0,
                        authorization_checks: guard.required_tokens.len() as u32,
                        unauthenticated_allowed: matches!(
                            guard.authorization_policy,
                            super::ProtocolGuardRequirement::UnauthenticatedAllowed(_)
                        ),
                        facts_applied: guard.delta_facts.len() as u32,
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
                AuraError::internal(format!("Sequence delta application failed: {e}"))
            })?;
    }

    info!(
        operations_completed = results.len(),
        total_deltas_applied = all_deltas.len(),
        "Guarded sequence completed successfully"
    );

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guards::{ProtocolGuardRequirement, UnauthenticatedAllowed};

    #[tokio::test]
    async fn forgotten_tokens_deny_guard_evaluation() {
        let guard = ProtocolGuard::new_for_testing("test.protected.empty");
        let result = evaluate_guard(&guard, 1_700_000_000)
            .await
            .expect("guard evaluation should complete");

        assert!(!result.passed);
        assert_eq!(result.flow_consumed, 0);
        assert!(!result.unauthenticated_allowed);
        assert!(result
            .failed_requirements
            .iter()
            .any(|failure| failure.contains("no Biscuit authorization tokens")));
    }

    #[tokio::test]
    async fn explicit_unauthenticated_policy_allows_zero_auth_checks() {
        let guard = ProtocolGuard::new_unauthenticated_for_testing("test.unauthenticated.read");
        let result = evaluate_guard(&guard, 1_700_000_000)
            .await
            .expect("guard evaluation should complete");

        assert!(result.passed);
        assert!(result.failed_requirements.is_empty());
        assert_eq!(result.flow_consumed, 0);
        assert!(result.unauthenticated_allowed);
    }

    #[test]
    fn production_constructor_rejects_empty_required_tokens() {
        let err = ProtocolGuardRequirement::required_tokens(Vec::new()).unwrap_err();
        assert!(err.to_string().contains("requires at least one"));
    }

    #[test]
    fn unauthenticated_policy_requires_metadata() {
        let err = UnauthenticatedAllowed::new("", "scope", "doc").unwrap_err();
        assert!(err.to_string().contains("requires reason_code"));
    }
}

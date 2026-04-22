//! Capability guard for authority-based operations
//!
//! This module provides guard evaluation for authority operations,
//! integrating with Biscuit tokens for authorization.
//!
//! `CapabilityGuard` provides high-level typed operations (AuthorityOp, ContextOp)
//! and delegates to `BiscuitGuardEvaluator` for low-level capability evaluation.

use super::{BiscuitGuardEvaluator, CapabilityId, GuardResult};
use crate::authorization::BiscuitAuthorizationBridge;
use aura_authorization::{AuthorityOp, ContextOp, ResourceScope};
use aura_core::{AuraError, AuthorityId, ContextId, FlowBudget, FlowCost, Result};
use biscuit_auth::Biscuit;

/// Guard for evaluating capability-based authorization
///
/// Wraps `BiscuitGuardEvaluator` to provide typed operation semantics
/// with flow cost calculations for authority and context operations.
pub struct CapabilityGuard {
    /// Low-level Biscuit evaluator for capability checks
    evaluator: BiscuitGuardEvaluator,
    /// Optional context for contextual authorization
    context_id: Option<ContextId>,
}

impl CapabilityGuard {
    fn with_optional_context(
        evaluator: BiscuitGuardEvaluator,
        context_id: Option<ContextId>,
    ) -> Self {
        Self {
            evaluator,
            context_id,
        }
    }

    /// Create a new capability guard
    pub fn new(biscuit_bridge: BiscuitAuthorizationBridge) -> Self {
        Self::with_optional_context(BiscuitGuardEvaluator::new(biscuit_bridge), None)
    }

    /// Create a capability guard from an existing evaluator
    pub fn from_evaluator(evaluator: BiscuitGuardEvaluator) -> Self {
        Self::with_optional_context(evaluator, None)
    }

    /// Create a capability guard with context
    pub fn with_context(biscuit_bridge: BiscuitAuthorizationBridge, context_id: ContextId) -> Self {
        Self::with_optional_context(BiscuitGuardEvaluator::new(biscuit_bridge), Some(context_id))
    }

    /// Create a capability guard from an existing evaluator with context
    pub fn from_evaluator_with_context(
        evaluator: BiscuitGuardEvaluator,
        context_id: ContextId,
    ) -> Self {
        Self::with_optional_context(evaluator, Some(context_id))
    }

    /// Get the underlying evaluator for low-level operations
    pub fn evaluator(&self) -> &BiscuitGuardEvaluator {
        &self.evaluator
    }

    /// Get the authority ID from the underlying evaluator
    pub fn authority_id(&self) -> AuthorityId {
        self.evaluator.authority_id()
    }

    /// Evaluate an authority operation
    pub async fn evaluate_authority_op(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<bool> {
        let scope = ResourceScope::Authority {
            authority_id: *authority_id,
            operation: operation.clone(),
        };
        let flow_cost = Self::authority_op_flow_cost(operation);
        let capability =
            CapabilityId::try_from(operation.as_str()).expect("authority ops use valid names");
        self.evaluate_scope_bool(
            token,
            capability,
            scope,
            flow_cost,
            flow_budget,
            current_time_seconds,
        )
    }

    /// Get flow cost for an authority operation
    fn authority_op_flow_cost(operation: &AuthorityOp) -> FlowCost {
        FlowCost::from(match operation {
            AuthorityOp::UpdateTree => 100,
            AuthorityOp::AddDevice => 75,
            AuthorityOp::RemoveDevice => 75,
            AuthorityOp::Rotate => 150,
            AuthorityOp::AddGuardian => 200,
            AuthorityOp::RemoveGuardian => 200,
            AuthorityOp::ModifyThreshold => 300,
            AuthorityOp::RevokeDevice => 100,
        })
    }

    /// Evaluate an authority operation with full authorization result
    async fn evaluate_authority_op_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<(bool, GuardResult)> {
        let scope = ResourceScope::Authority {
            authority_id: *authority_id,
            operation: operation.clone(),
        };
        let flow_cost = Self::authority_op_flow_cost(operation);
        let capability =
            CapabilityId::try_from(operation.as_str()).expect("authority ops use valid names");
        self.evaluate_scope_with_result(
            token,
            capability,
            scope,
            flow_cost,
            flow_budget,
            current_time_seconds,
        )
    }

    /// Evaluate a context operation
    pub async fn evaluate_context_op(
        &self,
        context_id: &ContextId,
        operation: &ContextOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<bool> {
        let scope = ResourceScope::Context {
            context_id: *context_id,
            operation: operation.clone(),
        };
        let flow_cost = Self::context_op_flow_cost(operation);
        let capability =
            CapabilityId::try_from(operation.as_str()).expect("context ops use valid names");
        self.evaluate_scope_bool(
            token,
            capability,
            scope,
            flow_cost,
            flow_budget,
            current_time_seconds,
        )
    }

    /// Get flow cost for a context operation
    fn context_op_flow_cost(operation: &ContextOp) -> FlowCost {
        FlowCost::from(match operation {
            ContextOp::AddBinding => 100,
            ContextOp::ApproveRecovery => 200,
            ContextOp::UpdateParams => 50,
            ContextOp::RecoverDeviceKey => 250,
            ContextOp::RecoverAccountAccess => 300,
            ContextOp::UpdateGuardianSet => 250,
            ContextOp::EmergencyFreeze => 500,
        })
    }

    fn denied_result() -> GuardResult {
        GuardResult {
            authorized: false,
            flow_consumed: 0,
            delegation_depth: None,
        }
    }

    fn require_token<'a>(&self, token: Option<&'a Biscuit>) -> Result<&'a Biscuit> {
        token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })
    }

    fn map_guard_error(error: super::GuardError) -> AuraError {
        AuraError::permission_denied(format!("Guard error: {error:?}"))
    }

    fn evaluate_scope_with_result(
        &self,
        token: Option<&Biscuit>,
        capability: CapabilityId,
        scope: ResourceScope,
        flow_cost: FlowCost,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<(bool, GuardResult)> {
        let token = self.require_token(token)?;
        match self.evaluator.evaluate_guard(
            token,
            &capability,
            &scope,
            flow_cost,
            flow_budget,
            current_time_seconds,
        ) {
            Ok(result) => Ok((true, result)),
            Err(super::GuardError::MissingCapability { .. }) => Ok((false, Self::denied_result())),
            Err(error) => Err(Self::map_guard_error(error)),
        }
    }

    fn evaluate_scope_bool(
        &self,
        token: Option<&Biscuit>,
        capability: CapabilityId,
        scope: ResourceScope,
        flow_cost: FlowCost,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<bool> {
        self.evaluate_scope_with_result(
            token,
            capability,
            scope,
            flow_cost,
            flow_budget,
            current_time_seconds,
        )
        .map(|(authorized, _)| authorized)
    }
}

/// Extension trait for integrating with guard chains
#[allow(async_fn_in_trait)]
pub trait CapabilityGuardExt {
    /// Evaluate with guard result
    async fn evaluate_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<GuardResult>;
}

impl CapabilityGuardExt for CapabilityGuard {
    async fn evaluate_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<GuardResult> {
        let (authorized, result) = self
            .evaluate_authority_op_with_result(
                authority_id,
                operation,
                token,
                flow_budget,
                current_time_seconds,
            )
            .await?;

        // The result already contains the correct values from the evaluator
        Ok(if authorized {
            result
        } else {
            CapabilityGuard::denied_result()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::Epoch;
    use biscuit_auth::PublicKey;

    /// Missing Biscuit token fails evaluation and charges no budget.
    #[tokio::test]
    async fn test_authority_operation_guard() {
        // Create mock bridge
        let root_key = match PublicKey::from_bytes(&[0u8; 32]) {
            Ok(key) => key,
            Err(err) => panic!("invalid root key: {err}"),
        };
        let bridge =
            BiscuitAuthorizationBridge::new(root_key, AuthorityId::new_from_entropy([1u8; 32]));

        // Create guard
        let guard = CapabilityGuard::new(bridge);

        // Create flow budget
        let mut budget = FlowBudget::new(1000, Epoch::new(0)); // limit=1000, epoch=0

        // Test without token (should fail)
        let result = guard
            .evaluate_authority_op(
                &AuthorityId::new_from_entropy([71u8; 32]),
                &AuthorityOp::AddDevice,
                None,
                &mut budget,
                1,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(budget.spent, 0); // No charge on failure
    }
}

//! Capability guard for authority-based operations
//!
//! This module provides guard evaluation for authority operations,
//! integrating with Biscuit tokens for authorization.
//!
//! `CapabilityGuard` provides high-level typed operations (AuthorityOp, ContextOp)
//! and delegates to `BiscuitGuardEvaluator` for low-level capability evaluation.

use super::{BiscuitGuardEvaluator, GuardResult};
use crate::authorization::BiscuitAuthorizationBridge;
use aura_core::{AuraError, AuthorityId, ContextId, FlowBudget, Result};
use aura_authorization::{AuthorityOp, ContextOp, ResourceScope};
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
    /// Create a new capability guard
    pub fn new(biscuit_bridge: BiscuitAuthorizationBridge) -> Self {
        Self {
            evaluator: BiscuitGuardEvaluator::new(biscuit_bridge),
            context_id: None,
        }
    }

    /// Create a capability guard from an existing evaluator
    pub fn from_evaluator(evaluator: BiscuitGuardEvaluator) -> Self {
        Self {
            evaluator,
            context_id: None,
        }
    }

    /// Create a capability guard with context
    pub fn with_context(biscuit_bridge: BiscuitAuthorizationBridge, context_id: ContextId) -> Self {
        Self {
            evaluator: BiscuitGuardEvaluator::new(biscuit_bridge),
            context_id: Some(context_id),
        }
    }

    /// Create a capability guard from an existing evaluator with context
    pub fn from_evaluator_with_context(
        evaluator: BiscuitGuardEvaluator,
        context_id: ContextId,
    ) -> Self {
        Self {
            evaluator,
            context_id: Some(context_id),
        }
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
    ) -> Result<bool> {
        // Create resource scope
        let scope = ResourceScope::Authority {
            authority_id: *authority_id,
            operation: operation.clone(),
        };

        // Determine flow cost based on operation
        let flow_cost = Self::authority_op_flow_cost(operation);

        // If no token provided, deny by default
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Delegate to evaluator for authorization and budget management
        match self.evaluator.evaluate_guard(
            token,
            operation.as_str(),
            &scope,
            flow_cost,
            flow_budget,
            0, // current_time_seconds
        ) {
            Ok(_result) => Ok(true),
            Err(super::GuardError::AuthorizationFailed(_)) => Ok(false),
            Err(e) => Err(AuraError::permission_denied(format!(
                "Guard error: {:?}",
                e
            ))),
        }
    }

    /// Get flow cost for an authority operation
    fn authority_op_flow_cost(operation: &AuthorityOp) -> u64 {
        match operation {
            AuthorityOp::UpdateTree => 100,
            AuthorityOp::AddDevice => 75,
            AuthorityOp::RemoveDevice => 75,
            AuthorityOp::Rotate => 150,
            AuthorityOp::AddGuardian => 200,
            AuthorityOp::RemoveGuardian => 200,
            AuthorityOp::ModifyThreshold => 300,
            AuthorityOp::RevokeDevice => 100,
        }
    }

    /// Evaluate an authority operation with full authorization result
    async fn evaluate_authority_op_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
    ) -> Result<(bool, GuardResult)> {
        // Create resource scope
        let scope = ResourceScope::Authority {
            authority_id: *authority_id,
            operation: operation.clone(),
        };

        // Determine flow cost based on operation
        let flow_cost = Self::authority_op_flow_cost(operation);

        // If no token provided, return default result
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Delegate to evaluator for authorization and budget management
        match self.evaluator.evaluate_guard(
            token,
            operation.as_str(),
            &scope,
            flow_cost,
            flow_budget,
            0, // current_time_seconds
        ) {
            Ok(result) => Ok((true, result)),
            Err(super::GuardError::AuthorizationFailed(_)) => Ok((
                false,
                GuardResult {
                    authorized: false,
                    flow_consumed: 0,
                    delegation_depth: None,
                },
            )),
            Err(e) => Err(AuraError::permission_denied(format!(
                "Guard error: {:?}",
                e
            ))),
        }
    }

    /// Evaluate a context operation
    pub async fn evaluate_context_op(
        &self,
        context_id: &ContextId,
        operation: &ContextOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
    ) -> Result<bool> {
        // Create resource scope
        let scope = ResourceScope::Context {
            context_id: *context_id,
            operation: operation.clone(),
        };

        // Determine flow cost based on operation
        let flow_cost = Self::context_op_flow_cost(operation);

        // If no token provided, deny by default
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Delegate to evaluator for authorization and budget management
        match self.evaluator.evaluate_guard(
            token,
            operation.as_str(),
            &scope,
            flow_cost,
            flow_budget,
            0, // current_time_seconds
        ) {
            Ok(_result) => Ok(true),
            Err(super::GuardError::AuthorizationFailed(_)) => Ok(false),
            Err(e) => Err(AuraError::permission_denied(format!(
                "Guard error: {:?}",
                e
            ))),
        }
    }

    /// Get flow cost for a context operation
    fn context_op_flow_cost(operation: &ContextOp) -> u64 {
        match operation {
            ContextOp::AddBinding => 100,
            ContextOp::ApproveRecovery => 200,
            ContextOp::UpdateParams => 50,
            ContextOp::RecoverDeviceKey => 250,
            ContextOp::RecoverAccountAccess => 300,
            ContextOp::UpdateGuardianSet => 250,
            ContextOp::EmergencyFreeze => 500,
        }
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
    ) -> Result<GuardResult>;
}

impl CapabilityGuardExt for CapabilityGuard {
    async fn evaluate_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
    ) -> Result<GuardResult> {
        let (authorized, result) = self
            .evaluate_authority_op_with_result(authority_id, operation, token, flow_budget)
            .await?;

        // The result already contains the correct values from the evaluator
        Ok(if authorized {
            result
        } else {
            GuardResult {
                authorized: false,
                flow_consumed: 0,
                delegation_depth: None,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::epochs::Epoch;
    use biscuit_auth::PublicKey;

    #[tokio::test]
    async fn test_authority_operation_guard() {
        // Create mock bridge
        let root_key = PublicKey::from_bytes(&[0u8; 32]).unwrap();
        let bridge =
            BiscuitAuthorizationBridge::new(root_key, AuthorityId::new_from_entropy([1u8; 32]));

        // Create guard
        let guard = CapabilityGuard::new(bridge);

        // Create flow budget
        let mut budget = FlowBudget::new(1000, Epoch(0)); // limit=1000, epoch=0

        // Test without token (should fail)
        let result = guard
            .evaluate_authority_op(
                &AuthorityId::new_from_entropy([71u8; 32]),
                &AuthorityOp::AddDevice,
                None,
                &mut budget,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(budget.spent, 0); // No charge on failure
    }
}

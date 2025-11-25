//! Capability guard for authority-based operations
//!
//! This module provides guard evaluation for authority operations,
//! integrating with Biscuit tokens for authorization.

use super::GuardResult;
use crate::authorization::{AuthorizationResult, BiscuitAuthorizationBridge};
use aura_core::{AuraError, AuthorityId, ContextId, FlowBudget, Result};
use aura_wot::{AuthorityOp, ContextOp, ResourceScope};
use biscuit_auth::Biscuit;

/// Guard for evaluating capability-based authorization
pub struct CapabilityGuard {
    /// Biscuit authorization bridge
    biscuit_bridge: BiscuitAuthorizationBridge,
    /// Optional context for contextual authorization
    context_id: Option<ContextId>,
}

impl CapabilityGuard {
    /// Create a new capability guard
    pub fn new(biscuit_bridge: BiscuitAuthorizationBridge) -> Self {
        Self {
            biscuit_bridge,
            context_id: None,
        }
    }

    /// Create a capability guard with context
    pub fn with_context(biscuit_bridge: BiscuitAuthorizationBridge, context_id: ContextId) -> Self {
        Self {
            biscuit_bridge,
            context_id: Some(context_id),
        }
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
        let flow_cost = match operation {
            AuthorityOp::UpdateTree => 100,
            AuthorityOp::AddDevice => 75,
            AuthorityOp::RemoveDevice => 75,
            AuthorityOp::Rotate => 150,
            AuthorityOp::AddGuardian => 200,
            AuthorityOp::RemoveGuardian => 200,
            AuthorityOp::ModifyThreshold => 300,
            AuthorityOp::RevokeDevice => 100,
        };

        // Check flow budget
        if !flow_budget.can_charge(flow_cost) {
            return Err(AuraError::invalid(format!(
                "Insufficient budget for operation: {} (required: {}, available: {})",
                operation.as_str(),
                flow_cost,
                flow_budget.headroom()
            )));
        }

        // If no token provided, deny by default
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Authorize with Biscuit
        let auth_result = self
            .biscuit_bridge
            .authorize(token, operation.as_str(), &scope, 0)
            .map_err(|e| AuraError::permission_denied(format!("Biscuit error: {:?}", e)))?;

        if !auth_result.authorized {
            return Ok(false);
        }

        // Charge flow budget
        flow_budget.record_charge(flow_cost);

        Ok(true)
    }

    /// Evaluate an authority operation with full authorization result
    async fn evaluate_authority_op_with_result(
        &self,
        authority_id: &AuthorityId,
        operation: &AuthorityOp,
        token: Option<&Biscuit>,
        flow_budget: &mut FlowBudget,
    ) -> Result<(bool, AuthorizationResult)> {
        // Create resource scope
        let scope = ResourceScope::Authority {
            authority_id: *authority_id,
            operation: operation.clone(),
        };

        // Determine flow cost based on operation
        let flow_cost = match operation {
            AuthorityOp::UpdateTree => 100,
            AuthorityOp::AddDevice => 75,
            AuthorityOp::RemoveDevice => 75,
            AuthorityOp::Rotate => 150,
            AuthorityOp::AddGuardian => 200,
            AuthorityOp::RemoveGuardian => 200,
            AuthorityOp::ModifyThreshold => 300,
            AuthorityOp::RevokeDevice => 100,
        };

        // Check flow budget
        if !flow_budget.can_charge(flow_cost) {
            return Err(AuraError::invalid(format!(
                "Insufficient budget for operation: {} (required: {}, available: {})",
                operation.as_str(),
                flow_cost,
                flow_budget.headroom()
            )));
        }

        // If no token provided, return default result
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Authorize with Biscuit
        let auth_result = self
            .biscuit_bridge
            .authorize(token, operation.as_str(), &scope, 0)
            .map_err(|e| AuraError::permission_denied(format!("Biscuit error: {:?}", e)))?;

        if !auth_result.authorized {
            return Ok((false, auth_result));
        }

        // Charge flow budget
        flow_budget.record_charge(flow_cost);

        Ok((true, auth_result))
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
        let flow_cost = match operation {
            ContextOp::AddBinding => 100,
            ContextOp::ApproveRecovery => 200,
            ContextOp::UpdateParams => 50,
            ContextOp::RecoverDeviceKey => 250,
            ContextOp::RecoverAccountAccess => 300,
            ContextOp::UpdateGuardianSet => 250,
            ContextOp::EmergencyFreeze => 500,
        };

        // Check flow budget
        if !flow_budget.can_charge(flow_cost) {
            return Err(AuraError::invalid(format!(
                "Insufficient budget for operation: {} (required: {}, available: {})",
                operation.as_str(),
                flow_cost,
                flow_budget.headroom()
            )));
        }

        // If no token provided, deny by default
        let token = token.ok_or_else(|| {
            AuraError::permission_denied("No authorization token provided".to_string())
        })?;

        // Authorize with Biscuit
        let auth_result = self
            .biscuit_bridge
            .authorize(token, operation.as_str(), &scope, 0)
            .map_err(|e| AuraError::permission_denied(format!("Biscuit error: {:?}", e)))?;

        if !auth_result.authorized {
            return Ok(false);
        }

        // Charge flow budget
        flow_budget.record_charge(flow_cost);

        Ok(true)
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
        let flow_cost = match operation {
            AuthorityOp::UpdateTree => 100,
            AuthorityOp::AddDevice => 75,
            AuthorityOp::RemoveDevice => 75,
            AuthorityOp::Rotate => 150,
            AuthorityOp::AddGuardian => 200,
            AuthorityOp::RemoveGuardian => 200,
            AuthorityOp::ModifyThreshold => 300,
            AuthorityOp::RevokeDevice => 100,
        };

        let (authorized, auth_result) = self
            .evaluate_authority_op_with_result(authority_id, operation, token, flow_budget)
            .await?;

        Ok(GuardResult {
            authorized,
            flow_consumed: if authorized { flow_cost } else { 0 },
            delegation_depth: auth_result.delegation_depth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{identifiers::DeviceId, session_epochs::Epoch};
    use biscuit_auth::PublicKey;

    #[tokio::test]
    async fn test_authority_operation_guard() {
        // Create mock bridge
        let root_key = PublicKey::from_bytes(&[0u8; 32]).unwrap();
        let bridge = BiscuitAuthorizationBridge::new(root_key, DeviceId::new());

        // Create guard
        let guard = CapabilityGuard::new(bridge);

        // Create flow budget
        let mut budget = FlowBudget::new(1000, Epoch(0)); // limit=1000, epoch=0

        // Test without token (should fail)
        let result = guard
            .evaluate_authority_op(
                &AuthorityId::new(),
                &AuthorityOp::AddDevice,
                None,
                &mut budget,
            )
            .await;

        assert!(result.is_err());
        assert_eq!(budget.spent, 0); // No charge on failure
    }
}

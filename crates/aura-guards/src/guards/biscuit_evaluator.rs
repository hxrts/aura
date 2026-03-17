//! Biscuit guard evaluator for capability and flow budget enforcement.
//!
//! Combines Biscuit authorization with flow budget checking to provide
//! atomic capability verification and budget charging for guard chains.

use crate::authorization::BiscuitAuthorizationBridge;
use crate::guards::types::CapabilityId;
use aura_authorization::{BiscuitError, ResourceScope};
use aura_core::{AuthorityId, FlowBudget, FlowCost};
use biscuit_auth::Biscuit;

pub struct BiscuitGuardEvaluator {
    bridge: BiscuitAuthorizationBridge,
}

impl BiscuitGuardEvaluator {
    pub fn new(bridge: BiscuitAuthorizationBridge) -> Self {
        Self { bridge }
    }

    /// Get the authority ID from the underlying bridge
    pub fn authority_id(&self) -> AuthorityId {
        self.bridge.authority_id()
    }

    /// Backwards compatible wrapper for evaluate_guard without explicit time
    /// Uses default time value for testing/mock scenarios
    pub fn evaluate_guard_default_time(
        &self,
        token: &Biscuit,
        guard_capability: &CapabilityId,
        resource: &ResourceScope,
        flow_cost: FlowCost,
        budget: &mut FlowBudget,
    ) -> Result<GuardResult, GuardError> {
        self.evaluate_guard(token, guard_capability, resource, flow_cost, budget, 0)
    }

    /// Backwards compatible wrapper for check_guard without explicit time
    /// Uses default time value for testing/mock scenarios
    pub fn check_guard_default_time(
        &self,
        token: &Biscuit,
        guard_capability: &CapabilityId,
        resource: &ResourceScope,
    ) -> Result<bool, GuardError> {
        self.check_guard(token, guard_capability, resource, 0)
    }

    pub fn evaluate_guard(
        &self,
        token: &Biscuit,
        guard_capability: &CapabilityId,
        resource: &ResourceScope,
        flow_cost: FlowCost,
        budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<GuardResult, GuardError> {
        let can_charge =
            budget
                .can_charge(flow_cost)
                .map_err(|e| GuardError::FlowBudgetEvaluationFailed {
                    detail: e.to_string(),
                })?;
        if !can_charge {
            return Err(GuardError::BudgetExceeded {
                required: u64::from(flow_cost),
                available: budget.headroom(),
            });
        }

        let auth_result = self.bridge.authorize(
            token,
            guard_capability.as_str(),
            resource,
            current_time_seconds,
        )?;

        if !auth_result.authorized {
            return Err(GuardError::MissingCapability {
                capability: guard_capability.to_string(),
            });
        }

        if let Err(e) = budget.record_charge(flow_cost) {
            return Err(GuardError::FlowBudgetChargeFailed {
                detail: e.to_string(),
            });
        }

        Ok(GuardResult {
            authorized: true,
            flow_consumed: u64::from(flow_cost),
            delegation_depth: auth_result.delegation_depth,
        })
    }

    pub fn check_guard(
        &self,
        token: &Biscuit,
        guard_capability: &CapabilityId,
        resource: &ResourceScope,
        current_time_seconds: u64,
    ) -> Result<bool, GuardError> {
        let auth_result = self.bridge.authorize(
            token,
            guard_capability.as_str(),
            resource,
            current_time_seconds,
        )?;
        Ok(auth_result.authorized)
    }
}

#[derive(Debug, Clone)]
pub struct GuardResult {
    pub authorized: bool,
    pub flow_consumed: u64,
    pub delegation_depth: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("Budget exceeded: required {required}, available {available}")]
    BudgetExceeded { required: u64, available: u64 },

    #[error("authorization failed: missing capability {capability}")]
    MissingCapability { capability: String },

    #[error("Biscuit error: {0}")]
    Biscuit(#[from] BiscuitError),

    #[error("flow budget evaluation failed: {detail}")]
    FlowBudgetEvaluationFailed { detail: String },

    #[error("flow budget charge failed: {detail}")]
    FlowBudgetChargeFailed { detail: String },
}

impl aura_core::ProtocolErrorCode for GuardError {
    fn code(&self) -> &'static str {
        match self {
            GuardError::BudgetExceeded { .. } => "budget_exceeded",
            GuardError::MissingCapability { .. } => "unauthorized",
            GuardError::Biscuit(_) => "biscuit_error",
            GuardError::FlowBudgetEvaluationFailed { .. } => "flow_budget_evaluation_failed",
            GuardError::FlowBudgetChargeFailed { .. } => "flow_budget_charge_failed",
        }
    }
}

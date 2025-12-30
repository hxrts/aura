use crate::authorization::BiscuitAuthorizationBridge;
use aura_authorization::{BiscuitError, ResourceScope};
use aura_core::{AuthorityId, FlowBudget};
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
        guard_capability: &str,
        resource: &ResourceScope,
        flow_cost: u64,
        budget: &mut FlowBudget,
    ) -> Result<GuardResult, GuardError> {
        self.evaluate_guard(token, guard_capability, resource, flow_cost, budget, 0)
    }

    /// Backwards compatible wrapper for check_guard without explicit time
    /// Uses default time value for testing/mock scenarios
    pub fn check_guard_default_time(
        &self,
        token: &Biscuit,
        guard_capability: &str,
        resource: &ResourceScope,
    ) -> Result<bool, GuardError> {
        self.check_guard(token, guard_capability, resource, 0)
    }

    pub fn evaluate_guard(
        &self,
        token: &Biscuit,
        guard_capability: &str,
        resource: &ResourceScope,
        flow_cost: u64,
        budget: &mut FlowBudget,
        current_time_seconds: u64,
    ) -> Result<GuardResult, GuardError> {
        if !budget.can_charge(flow_cost) {
            return Err(GuardError::BudgetExceeded {
                required: flow_cost,
                available: budget.headroom(),
            });
        }

        let auth_result =
            self.bridge
                .authorize(token, guard_capability, resource, current_time_seconds)?;

        if !auth_result.authorized {
            return Err(GuardError::AuthorizationFailed(format!(
                "Token does not grant capability: {guard_capability}"
            )));
        }

        if let Err(e) = budget.record_charge(flow_cost) {
            return Err(GuardError::FlowBudget(format!(
                "Failed to record charge: {e}"
            )));
        }

        Ok(GuardResult {
            authorized: true,
            flow_consumed: flow_cost,
            delegation_depth: auth_result.delegation_depth,
        })
    }

    pub fn check_guard(
        &self,
        token: &Biscuit,
        guard_capability: &str,
        resource: &ResourceScope,
        current_time_seconds: u64,
    ) -> Result<bool, GuardError> {
        let auth_result =
            self.bridge
                .authorize(token, guard_capability, resource, current_time_seconds)?;
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

    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),

    #[error("Biscuit error: {0}")]
    Biscuit(#[from] BiscuitError),

    #[error("Flow budget error: {0}")]
    FlowBudget(String),
}

impl aura_core::ProtocolErrorCode for GuardError {
    fn code(&self) -> &'static str {
        match self {
            GuardError::BudgetExceeded { .. } => "budget_exceeded",
            GuardError::AuthorizationFailed(_) => "unauthorized",
            GuardError::Biscuit(_) => "biscuit_error",
            GuardError::FlowBudget(_) => "flow_budget",
        }
    }
}

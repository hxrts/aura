use crate::authorization::BiscuitAuthorizationBridge;
use aura_core::FlowBudget;
use aura_wot::{BiscuitError, ResourceScope};
use biscuit_auth::Biscuit;

pub struct BiscuitGuardEvaluator {
    bridge: BiscuitAuthorizationBridge,
}

impl BiscuitGuardEvaluator {
    pub fn new(bridge: BiscuitAuthorizationBridge) -> Self {
        Self { bridge }
    }

    pub fn evaluate_guard(
        &self,
        token: &Biscuit,
        guard_capability: &str,
        resource: &ResourceScope,
        flow_cost: u64,
        budget: &mut FlowBudget,
    ) -> Result<GuardResult, GuardError> {
        if !budget.can_charge(flow_cost) {
            return Err(GuardError::BudgetExceeded {
                required: flow_cost,
                available: budget.headroom(),
            });
        }

        let auth_result = self.bridge.authorize(token, guard_capability, resource)?;

        if !auth_result.authorized {
            return Err(GuardError::AuthorizationFailed(format!(
                "Token does not grant capability: {}",
                guard_capability
            )));
        }

        if !budget.record_charge(flow_cost) {
            return Err(GuardError::FlowBudget(
                "Failed to record charge".to_string(),
            ));
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
    ) -> Result<bool, GuardError> {
        let auth_result = self.bridge.authorize(token, guard_capability, resource)?;
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

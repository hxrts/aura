//! In-memory flow budget handler for tests.

#![allow(clippy::disallowed_types)]

use async_trait::async_trait;
use aura_authorization::{AuraResult, BiscuitAuthorizationBridge, ContextOp, ResourceScope};
use aura_core::effects::FlowBudgetEffects;
use aura_core::epochs::Epoch;
use aura_core::flow::{FlowBudget, FlowBudgetKey, Receipt};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, Hash32};
use biscuit_auth::Biscuit;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Simple in-memory flow budget handler with authority-scoped accounting.
#[derive(Clone)]
pub struct FlowBudgetHandler {
    authority: AuthorityId,
    default_limit: u64,
    budgets: Arc<Mutex<BTreeMap<FlowBudgetKey, FlowBudget>>>,
    scope_overrides: Arc<Mutex<BTreeMap<ContextId, ResourceScope>>>,
    policy_token: Option<Biscuit>,
    policy_bridge: Option<BiscuitAuthorizationBridge>,
    time_provider: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl std::fmt::Debug for FlowBudgetHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowBudgetHandler")
            .field("authority", &self.authority)
            .field("default_limit", &self.default_limit)
            .field("budgets", &self.budgets)
            .field("scope_overrides", &self.scope_overrides)
            .field("policy_token", &self.policy_token)
            .field("policy_bridge", &self.policy_bridge)
            .field("time_provider", &"<function>")
            .finish()
    }
}

impl FlowBudgetHandler {
    /// Create a handler with a default per-peer budget limit.
    pub fn new(authority: AuthorityId) -> Self {
        Self::with_limit(authority, 1024)
    }

    /// Create a handler with a custom default limit.
    pub fn with_limit(authority: AuthorityId, default_limit: u64) -> Self {
        Self {
            authority,
            default_limit,
            budgets: Arc::new(Mutex::new(BTreeMap::new())),
            scope_overrides: Arc::new(Mutex::new(BTreeMap::new())),
            policy_token: None,
            policy_bridge: None,
            time_provider: Arc::new(|| 0),
        }
    }

    /// Attach a Biscuit policy token and bridge for resource-scope enforcement.
    pub fn with_policy(mut self, token: Biscuit, bridge: BiscuitAuthorizationBridge) -> Self {
        self.policy_token = Some(token);
        self.policy_bridge = Some(bridge);
        self
    }

    /// Provide a time source for Biscuit authorization checks.
    pub fn with_time_provider(mut self, provider: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        self.time_provider = provider;
        self
    }

    /// Override the resource scope used for a specific context.
    pub fn set_scope_for_context(&self, context: ContextId, scope: ResourceScope) {
        if let Ok(mut scopes) = self.scope_overrides.lock() {
            scopes.insert(context, scope);
        }
    }

    fn scope_for_context(&self, context: &ContextId) -> ResourceScope {
        if let Ok(scopes) = self.scope_overrides.lock() {
            if let Some(scope) = scopes.get(context) {
                return scope.clone();
            }
        }

        ResourceScope::Context {
            context_id: *context,
            operation: ContextOp::UpdateParams,
        }
    }

    fn authorize_scope(&self, scope: &ResourceScope) -> AuraResult<()> {
        match (&self.policy_token, &self.policy_bridge) {
            (Some(token), Some(bridge)) => {
                let now = (self.time_provider)();
                let result = bridge
                    .authorize_with_time(token, "flow_charge", scope, Some(now))
                    .map_err(|e| {
                        AuraError::permission_denied(format!("flow budget policy failed: {e}"))
                    })?;
                if result.authorized {
                    Ok(())
                } else {
                    Err(AuraError::permission_denied(
                        "flow budget charge not authorized by Biscuit policy",
                    ))
                }
            }
            _ => Ok(()),
        }
    }

    fn record_charge(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> AuraResult<FlowBudget> {
        let key = FlowBudgetKey::new(*context, *peer);
        let mut budgets = self
            .budgets
            .lock()
            .map_err(|_| AuraError::internal("flow budget state poisoned"))?;

        let budget = budgets
            .entry(key)
            .or_insert_with(|| FlowBudget::new(self.default_limit, Epoch::initial()));

        if !budget.record_charge(cost as u64) {
            return Err(AuraError::budget_exceeded(format!(
                "insufficient flow budget: remaining={}, cost={}",
                budget.remaining(),
                cost
            )));
        }

        Ok(*budget)
    }
}

#[async_trait]
impl FlowBudgetEffects for FlowBudgetHandler {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> AuraResult<Receipt> {
        let scope = self.scope_for_context(context);
        self.authorize_scope(&scope)?;

        let budget = self.record_charge(context, peer, cost)?;

        Ok(Receipt::new(
            *context,
            self.authority,
            *peer,
            budget.epoch,
            cost,
            budget.spent,
            Hash32::default(),
            Vec::new(),
        ))
    }
}

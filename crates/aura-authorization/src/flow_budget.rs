//! Journal-backed flow budget handler.
//!
//! This handler delegates budget persistence to the JournalEffects surface,
//! keeping Layer 2 free of in-memory mutable state.

use async_trait::async_trait;
use aura_core::effects::{FlowBudgetEffects, JournalEffects};
use aura_core::flow::Receipt;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult, Hash32};
use biscuit_auth::Biscuit;
use std::sync::Arc;

use crate::{BiscuitAuthorizationBridge, ContextOp, ResourceScope};
use aura_core::AuthorizationOp;

/// Flow budget handler that persists budgets via JournalEffects.
#[derive(Debug, Clone)]
pub struct JournalBackedFlowBudgetHandler<J: JournalEffects> {
    authority: AuthorityId,
    journal: J,
    policy_token: Option<Biscuit>,
    policy_bridge: Option<BiscuitAuthorizationBridge>,
    time_provider: Option<Arc<dyn Fn() -> u64 + Send + Sync>>,
}

impl<J: JournalEffects> JournalBackedFlowBudgetHandler<J> {
    /// Create a new handler scoped to the given authority.
    pub fn new(authority: AuthorityId, journal: J) -> Self {
        Self {
            authority,
            journal,
            policy_token: None,
            policy_bridge: None,
            time_provider: None,
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
        self.time_provider = Some(provider);
        self
    }

    fn authorize_scope(&self, scope: &ResourceScope) -> AuraResult<()> {
        match (&self.policy_token, &self.policy_bridge) {
            (Some(token), Some(bridge)) => {
                let now = self
                    .time_provider
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("flow budget time provider not configured"))?
                    ();
                let result = bridge
                    .authorize_with_time(token, AuthorizationOp::FlowCharge, scope, Some(now))
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
}

#[async_trait]
impl<J: JournalEffects + Send + Sync> FlowBudgetEffects for JournalBackedFlowBudgetHandler<J> {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> AuraResult<Receipt> {
        let scope = ResourceScope::Context {
            context_id: *context,
            operation: ContextOp::UpdateParams,
        };
        self.authorize_scope(&scope)?;

        let current = self.journal.get_flow_budget(context, peer).await?;
        if current.limit > 0 && !current.can_charge(cost as u64) {
            return Err(AuraError::budget_exceeded(format!(
                "insufficient flow budget: remaining={}, cost={}",
                current.remaining(),
                cost
            )));
        }

        let mut updated = current;
        updated.spent = updated.spent.saturating_add(cost as u64);
        let updated = self
            .journal
            .update_flow_budget(context, peer, &updated)
            .await?;

        Ok(Receipt::new(
            *context,
            self.authority,
            *peer,
            updated.epoch,
            cost,
            updated.spent,
            Hash32::default(),
            Vec::new(),
        ))
    }
}

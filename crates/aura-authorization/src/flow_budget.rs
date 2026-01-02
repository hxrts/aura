//! Journal-backed flow budget handler.
//!
//! This handler delegates budget persistence to the JournalEffects surface,
//! keeping Layer 2 free of in-memory mutable state.

use async_trait::async_trait;
use aura_core::effects::{FlowBudgetEffects, JournalEffects};
use aura_core::flow::{FlowCost, FlowNonce, Receipt, ReceiptSig};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult, Hash32};
use biscuit_auth::Biscuit;
use std::sync::Arc;

use crate::{BiscuitAuthorizationBridge, ContextOp, ResourceScope};
use aura_core::AuthorizationOp;

/// Flow budget handler that persists budgets via JournalEffects.
#[derive(Clone)]
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
                let now = self.time_provider.as_ref().ok_or_else(|| {
                    AuraError::invalid("flow budget time provider not configured")
                })?();
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
        cost: FlowCost,
    ) -> AuraResult<Receipt> {
        let scope = ResourceScope::Context {
            context_id: *context,
            operation: ContextOp::UpdateParams,
        };
        self.authorize_scope(&scope)?;

        let current = self.journal.get_flow_budget(context, peer).await?;
        let mut updated = current;
        if updated.limit > 0 {
            updated
                .record_charge(cost)
                .map_err(|e| AuraError::budget_exceeded(e.to_string()))?;
        } else {
            let cost_value = u64::from(cost);
            updated.spent = updated.spent.checked_add(cost_value).ok_or_else(|| {
                AuraError::invalid("flow budget overflow while recording unbounded spend")
            })?;
        }
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
            FlowNonce::new(updated.spent),
            Hash32::default(),
            ReceiptSig::new(Vec::new())?,
        ))
    }
}

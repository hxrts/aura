use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::effects::{FlowBudgetEffects, JournalEffects};
use aura_core::scope::{AuthorizationOp, ContextOp, ResourceScope};
use aura_core::{AuraError, AuthorityId, ContextId, Hash32};

// Implementation of FlowBudgetEffects
#[async_trait]
impl FlowBudgetEffects for AuraEffectSystem {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &AuthorityId,
        cost: u32,
    ) -> aura_core::AuraResult<aura_core::Receipt> {
        if let Some((token, bridge)) = &self.journal.journal_policy() {
            let scope = ResourceScope::Context {
                context_id: *context,
                operation: ContextOp::UpdateParams,
            };
            let now = self.time_handler.current_timestamp().await?;
            let decision = bridge
                .authorize_with_time(token, AuthorizationOp::FlowCharge, &scope, Some(now))
                .map_err(|e| {
                    AuraError::permission_denied(format!("flow budget policy failed: {e}"))
                })?;
            if !decision.authorized {
                return Err(AuraError::permission_denied(
                    "flow budget charge not authorized by Biscuit policy",
                ));
            }
        }

        let budget = JournalEffects::charge_flow_budget(self, context, peer, cost).await?;
        Ok(aura_core::Receipt::new(
            *context,
            self.authority_id,
            *peer,
            budget.epoch,
            cost,
            budget.spent,
            Hash32::default(),
            Vec::new(),
        ))
    }
}

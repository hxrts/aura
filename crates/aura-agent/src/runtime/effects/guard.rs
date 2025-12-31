use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::scope::{AuthorizationOp, ResourceScope};
use aura_core::AuthorityId;
use aura_protocol::effects::{AuthorizationEffects, LeakageEffects};

// AuthorizationEffects implementation delegating to the handler
#[async_trait]
impl AuthorizationEffects for AuraEffectSystem {
    async fn verify_capability(
        &self,
        capabilities: &aura_core::Cap,
        operation: AuthorizationOp,
        scope: &ResourceScope,
    ) -> Result<bool, aura_core::effects::AuthorizationError> {
        self.authorization_handler
            .verify_capability(capabilities, operation, scope)
            .await
    }

    async fn delegate_capabilities(
        &self,
        source_capabilities: &aura_core::Cap,
        requested_capabilities: &aura_core::Cap,
        target_authority: &AuthorityId,
    ) -> Result<aura_core::Cap, aura_core::effects::AuthorizationError> {
        self.authorization_handler
            .delegate_capabilities(
                source_capabilities,
                requested_capabilities,
                target_authority,
            )
            .await
    }
}

// LeakageEffects implementation delegating to the handler
#[async_trait]
impl LeakageEffects for AuraEffectSystem {
    async fn record_leakage(
        &self,
        event: aura_core::effects::LeakageEvent,
    ) -> aura_core::Result<()> {
        self.leakage_handler.record_leakage(event).await
    }

    async fn get_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
    ) -> aura_core::Result<aura_core::effects::LeakageBudget> {
        self.leakage_handler.get_leakage_budget(context_id).await
    }

    async fn check_leakage_budget(
        &self,
        context_id: aura_core::identifiers::ContextId,
        observer: aura_core::effects::ObserverClass,
        amount: u64,
    ) -> aura_core::Result<bool> {
        self.leakage_handler
            .check_leakage_budget(context_id, observer, amount)
            .await
    }

    async fn get_leakage_history(
        &self,
        context_id: aura_core::identifiers::ContextId,
        since_timestamp: Option<&aura_core::time::PhysicalTime>,
    ) -> aura_core::Result<Vec<aura_core::effects::LeakageEvent>> {
        self.leakage_handler
            .get_leakage_history(context_id, since_timestamp)
            .await
    }
}

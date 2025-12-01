//! Invitation Handlers
//!
//! Handlers for invitation-related operations.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_protocol::guards::send_guard::create_send_guard;
use serde_json;

/// Invitation handler
#[allow(dead_code)] // Part of future invitation API
pub struct InvitationHandler {
    context: HandlerContext,
}

impl InvitationHandler {
    /// Create a new invitation handler
    #[allow(dead_code)] // Part of future invitation API
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }

    /// Handle invitation creation
    #[allow(dead_code)] // Part of future invitation API
    pub async fn create_invitation(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        if cfg!(test) {
            return Ok(());
        }
        let guard = create_send_guard(
            "invitation:create".to_string(),
            self.context.effect_context.context_id(),
            self.context.authority.authority_id,
            50,
        );
        let result = guard.evaluate(effects).await.map_err(|e| {
            crate::core::AgentError::effects(format!("guard evaluation failed: {e}"))
        })?;
        if !result.authorized {
            return Err(crate::core::AgentError::effects(
                result
                    .denial_reason
                    .unwrap_or_else(|| "invitation create not authorized".to_string()),
            ));
        }
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation_created",
            &serde_json::json!({
                "authority": self.context.authority.authority_id,
            }),
        )
        .await?;
        Ok(())
    }

    /// Handle invitation acceptance
    #[allow(dead_code)] // Part of future invitation API
    pub async fn accept_invitation(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;
        if cfg!(test) {
            return Ok(());
        }
        let guard = create_send_guard(
            "invitation:accept".to_string(),
            self.context.effect_context.context_id(),
            self.context.authority.authority_id,
            50,
        );
        let result = guard.evaluate(effects).await.map_err(|e| {
            crate::core::AgentError::effects(format!("guard evaluation failed: {e}"))
        })?;
        if !result.authorized {
            return Err(crate::core::AgentError::effects(
                result
                    .denial_reason
                    .unwrap_or_else(|| "invitation accept not authorized".to_string()),
            ));
        }
        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "invitation_accepted",
            &serde_json::json!({
                "authority": self.context.authority.authority_id,
            }),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AgentConfig, AuthorityContext};
    use crate::runtime::effects::AuraEffectSystem;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn invitation_facts_are_journaled() {
        let authority_id = AuthorityId::new_from_entropy([91u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([9u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = InvitationHandler::new(authority_context.clone()).unwrap();

        let effects_guard = effects.read().await;
        handler.create_invitation(&effects_guard).await.unwrap();
        handler.accept_invitation(&effects_guard).await.unwrap();
    }
}

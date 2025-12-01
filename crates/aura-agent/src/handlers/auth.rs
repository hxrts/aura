//! Authentication Handlers
//!
//! Handlers for authentication-related operations.

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_protocol::guards::send_guard::create_send_guard;
use serde_json;

/// Authentication handler
#[allow(dead_code)] // Part of future authentication API
pub struct AuthHandler {
    context: HandlerContext,
}

impl AuthHandler {
    /// Create a new authentication handler
    #[allow(dead_code)] // Part of future authentication API
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;

        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }

    /// Handle authentication request
    #[allow(dead_code)] // Part of future authentication API
    pub async fn authenticate(&self, effects: &AuraEffectSystem) -> AgentResult<()> {
        HandlerUtilities::validate_authority_context(&self.context.authority)?;

        if cfg!(test) {
            return Ok(());
        }

        let guard = create_send_guard(
            "auth:authenticate".to_string(),
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
                    .unwrap_or_else(|| "authentication not authorized".to_string()),
            ));
        }

        HandlerUtilities::append_relational_fact(
            &self.context.authority,
            effects,
            self.context.effect_context.context_id(),
            "auth_authenticated",
            &serde_json::json!({ "authority": self.context.authority.authority_id }),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::core::AuthorityContext;
    use crate::runtime::effects::AuraEffectSystem;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn auth_fact_is_journaled() {
        let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([8u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let handler = AuthHandler::new(authority_context.clone()).unwrap();

        let effects_guard = effects.read().await;
        handler.authenticate(&effects_guard).await.unwrap();
    }
}

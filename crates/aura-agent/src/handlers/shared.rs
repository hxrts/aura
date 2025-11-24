//! Shared Handler Utilities
//!
//! Common utilities used by domain-specific handlers.

use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::EffectContext;
use aura_core::identifiers::{AuthorityId, ContextId, SessionId};

/// Handler context combining authority context with runtime utilities
pub struct HandlerContext {
    /// Authority context
    pub authority: AuthorityContext,

    /// Effect context for operations
    pub effect_context: EffectContext,
}

impl HandlerContext {
    /// Create a new handler context
    pub fn new(authority: AuthorityContext) -> Self {
        // Create a default context ID for this handler context
        let context_id = ContextId::new();
        let effect_context = EffectContext::new(
            authority.authority_id,
            context_id,
            aura_core::effects::ExecutionMode::Production, // Default
        );

        Self {
            authority,
            effect_context,
        }
    }

    /// Get storage key for this authority
    pub fn authority_storage_key(&self) -> String {
        format!("authority_{}", self.authority.authority_id)
    }

    /// Get storage key for a context
    pub fn context_storage_key(&self, context_id: &ContextId) -> String {
        format!("context_{}", context_id)
    }

    /// Execute operation with reliability (retry + backoff)
    pub async fn with_retry<T, E, F>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>,
    {
        // TODO: Implement actual retry logic when reliability module is available
        // For now, just execute once
        operation()
    }
}

/// Shared handler utilities
pub struct HandlerUtilities;

impl HandlerUtilities {
    /// Create effect context from authority
    pub fn create_effect_context(
        authority_id: AuthorityId,
        _session_id: Option<SessionId>,
    ) -> EffectContext {
        // Create a default context ID
        let context_id = ContextId::new();

        // If we have a specific session ID, we would need to update it
        // For now, the EffectContext creates its own session ID

        EffectContext::new(
            authority_id,
            context_id,
            aura_core::effects::ExecutionMode::Production,
        )
    }

    /// Validate authority context
    pub fn validate_authority_context(context: &AuthorityContext) -> AgentResult<()> {
        // Basic validation - can be extended
        if context.authority_id.to_string().is_empty() {
            return Err(crate::core::AgentError::context("Invalid authority ID"));
        }
        Ok(())
    }

    /// Create storage key for authority data
    pub fn authority_storage_key(authority_id: &AuthorityId) -> String {
        format!("authority_{}", authority_id)
    }

    /// Create storage key for context data
    pub fn context_storage_key(context_id: &ContextId) -> String {
        format!("context_{}", context_id)
    }
}

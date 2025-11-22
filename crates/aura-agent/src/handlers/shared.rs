//! Shared Handler Utilities
//!
//! Common utilities used by domain-specific handlers.

use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::{EffectContext, PersistenceUtils, ReliabilityManager};
use aura_core::identifiers::{AuthorityId, ContextId, SessionId};
use std::collections::HashMap;

/// Handler context combining authority context with runtime utilities
pub struct HandlerContext {
    /// Authority context
    pub authority: AuthorityContext,

    /// Reliability manager for retries and backoff
    pub reliability: ReliabilityManager,

    /// Effect context for operations
    pub effect_context: EffectContext,
}

impl HandlerContext {
    /// Create a new handler context
    pub fn new(authority: AuthorityContext) -> Self {
        let effect_context = EffectContext {
            authority_id: authority.authority_id,
            session_id: authority.session_id,
            metadata: HashMap::new(),
        };

        Self {
            authority,
            reliability: ReliabilityManager::default(),
            effect_context,
        }
    }

    /// Get storage key for this authority
    pub fn authority_storage_key(&self) -> String {
        PersistenceUtils::authority_key(&self.authority.authority_id)
    }

    /// Get storage key for a context
    pub fn context_storage_key(&self, context_id: &ContextId) -> String {
        PersistenceUtils::context_key(context_id)
    }

    /// Execute operation with reliability (retry + backoff)
    pub async fn with_retry<T, E, F>(&self, operation: F) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>,
    {
        self.reliability.with_retry(operation).await
    }
}

/// Shared handler utilities
pub struct HandlerUtilities;

impl HandlerUtilities {
    /// Create effect context from authority
    pub fn create_effect_context(
        authority_id: AuthorityId,
        session_id: Option<SessionId>,
    ) -> EffectContext {
        EffectContext {
            authority_id,
            session_id,
            metadata: HashMap::new(),
        }
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
        PersistenceUtils::authority_key(authority_id)
    }

    /// Create storage key for context data
    pub fn context_storage_key(context_id: &ContextId) -> String {
        PersistenceUtils::context_key(context_id)
    }
}

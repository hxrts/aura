//! Shared Handler Utilities
//!
//! Common utilities used by domain-specific handlers.

use crate::core::{default_context_id_for_authority, AgentResult, AuthorityContext};
use crate::runtime::{AuraEffectSystem, EffectContext};
use aura_core::identifiers::{AuthorityId, ContextId, SessionId};
use aura_core::Hash32;
use aura_journal::FactJournal;
use serde::Serialize;
use serde_json;

/// Handler context combining authority context with runtime utilities
#[derive(Clone)]
pub struct HandlerContext {
    /// Authority context
    #[allow(dead_code)] // Will be used for authority operations
    pub authority: AuthorityContext,

    /// Effect context for operations
    #[allow(dead_code)] // Will be used for effect operations
    pub effect_context: EffectContext,
}

impl HandlerContext {
    /// Create a new handler context
    pub fn new(authority: AuthorityContext) -> Self {
        // Create a default context ID for this handler context
        let context_id = default_context_id_for_authority(authority.authority_id());
        let effect_context = EffectContext::new(
            authority.authority_id(),
            context_id,
            aura_core::effects::ExecutionMode::Production, // Default
        );

        Self {
            authority,
            effect_context,
        }
    }

    /// Get storage key for this authority
    #[allow(dead_code)] // Part of future handler utilities API
    pub fn authority_storage_key(&self) -> String {
        format!("authority_{}", self.authority.authority_id())
    }

    /// Get storage key for a context
    #[allow(dead_code)] // Part of future handler utilities API
    pub fn context_storage_key(&self, context_id: &ContextId) -> String {
        format!("context_{}", context_id)
    }

    /// Execute operation with reliability (retry + backoff)
    #[allow(dead_code)] // Part of future handler utilities API
    pub async fn with_retry<T, E, F>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>,
    {
        let max_attempts = 3;
        for attempt in 0..max_attempts {
            match operation() {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if attempt + 1 == max_attempts {
                        return Err(err);
                    }
                    // Exponential-ish backoff for subsequent attempts; callers should apply the
                    // computed delay via their injected TimeEffects before retrying.
                    let _delay_ms = 10u64 * (1 << attempt);
                }
            }
        }
        // Should never reach here
        operation()
    }
}

/// Shared handler utilities
pub struct HandlerUtilities;

impl HandlerUtilities {
    /// Append a relational fact into the authority-scoped journal.
    pub async fn append_relational_fact<T: Serialize>(
        authority: &AuthorityContext,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        binding_type: &str,
        payload: &T,
    ) -> AgentResult<()> {
        let _ = authority; // Authority is implied by the effect system's configured identity.
        let binding_data = serde_json::to_vec(payload).map_err(|e| {
            crate::core::AgentError::effects(format!("serialize fact payload: {e}"))
        })?;
        effects
            .commit_generic_fact_bytes(context_id, binding_type, binding_data)
            .await
            .map(|_| ())
            .map_err(|e| crate::core::AgentError::effects(format!("commit fact: {e}")))
    }

    /// Append a generic fact (raw bytes) into the authority-scoped journal.
    ///
    /// This is used for domain facts like `InvitationFact` that serialize to bytes
    /// via their own serialization (e.g., `DomainFact::to_bytes()`).
    pub async fn append_generic_fact(
        authority: &AuthorityContext,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> AgentResult<()> {
        let _ = authority; // Authority is implied by the effect system's configured identity.
        effects
            .commit_generic_fact_bytes(context_id, binding_type, binding_data.to_vec())
            .await
            .map(|_| ())
            .map_err(|e| crate::core::AgentError::effects(format!("commit fact: {e}")))
    }

    /// Create effect context from authority
    #[allow(dead_code)] // Part of future handler utilities API
    pub fn create_effect_context(
        authority_id: AuthorityId,
        _session_id: Option<SessionId>,
    ) -> EffectContext {
        // Create a default context ID
        let context_id = default_context_id_for_authority(authority_id);

        // If we have a specific session ID, we would need to update it; by default the
        // EffectContext will allocate a fresh session identifier.

        EffectContext::new(
            authority_id,
            context_id,
            aura_core::effects::ExecutionMode::Production,
        )
    }

    /// Validate authority context
    pub fn validate_authority_context(context: &AuthorityContext) -> AgentResult<()> {
        // Basic validation - can be extended
        if context.authority_id().to_string().is_empty() {
            return Err(crate::core::AgentError::context("Invalid authority ID"));
        }
        Ok(())
    }

    /// Create storage key for authority data
    #[allow(dead_code)] // Part of future handler utilities API
    pub fn authority_storage_key(authority_id: &AuthorityId) -> String {
        format!("authority_{}", authority_id)
    }

    /// Create storage key for context data
    #[allow(dead_code)] // Part of future handler utilities API
    pub fn context_storage_key(context_id: &ContextId) -> String {
        format!("context_{}", context_id)
    }
}

/// Compute the commitment for the relational context journal.
///
/// This normalizes the shared hashing logic used by AMP and rendezvous flows.
pub fn context_commitment_from_journal(
    context_id: ContextId,
    journal: &FactJournal,
) -> AgentResult<Hash32> {
    let mut hasher = aura_core::hash::hasher();
    hasher.update(b"RELATIONAL_CONTEXT_FACTS");
    hasher.update(context_id.as_bytes());
    for fact in journal.facts.iter() {
        let bytes = aura_core::util::serialization::to_vec(fact).map_err(|e| {
            crate::core::AgentError::effects(format!("Serialize context fact: {e}"))
        })?;
        hasher.update(&bytes);
    }
    Ok(Hash32(hasher.finalize()))
}

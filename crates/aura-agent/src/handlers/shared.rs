//! Shared Handler Utilities
//!
//! Common utilities used by domain-specific handlers.

use crate::core::{default_context_id_for_authority, AgentResult, AuthorityContext};
use crate::runtime::{AuraEffectSystem, EffectContext};
use aura_core::types::facts::FactEnvelope;
use aura_core::types::facts::FactTypeId;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{FactContent, RelationalFact};
use aura_journal::FactJournal;
use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::fmt::Display;

/// Handler context combining authority context with runtime utilities
#[derive(Clone)]
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
}

/// Shared handler utilities
pub struct HandlerUtilities;

impl HandlerUtilities {
    /// Append a relational fact into the authority-scoped journal.
    pub async fn append_relational_fact<T: Serialize>(
        authority: &AuthorityContext,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        binding_type: FactTypeId,
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
        binding_type: FactTypeId,
        binding_data: &[u8],
    ) -> AgentResult<()> {
        let _ = authority; // Authority is implied by the effect system's configured identity.
        effects
            .commit_generic_fact_bytes(context_id, binding_type, binding_data.to_vec())
            .await
            .map(|_| ())
            .map_err(|e| crate::core::AgentError::effects(format!("commit fact: {e}")))
    }

    /// Validate authority context
    pub fn validate_authority_context(context: &AuthorityContext) -> AgentResult<()> {
        // Basic validation - can be extended
        if context.authority_id().to_string().is_empty() {
            return Err(crate::core::AgentError::context("Invalid authority ID"));
        }
        Ok(())
    }
}

pub fn map_handler_effect_error(label: &'static str, error: impl Display) -> crate::core::AgentError {
    crate::core::AgentError::effects(format!("{label}: {error}"))
}

pub fn map_handler_time_read_error(error: impl Display) -> crate::core::AgentError {
    map_handler_effect_error("Failed to read time", error)
}

pub fn map_handler_tree_read_error(error: impl Display) -> crate::core::AgentError {
    map_handler_effect_error("Failed to read tree state", error)
}

pub fn resolve_charge_peer<T>(
    commands: &[T],
    fallback: AuthorityId,
    resolver: impl Fn(&T) -> Option<AuthorityId>,
) -> AuthorityId {
    commands.iter().find_map(resolver).unwrap_or(fallback)
}

pub fn build_string_metadata(
    entries: impl IntoIterator<Item = (&'static str, String)>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    for (key, value) in entries {
        metadata.insert(key.to_string(), value);
    }
    metadata
}

pub fn build_transport_metadata(
    content_type: &'static str,
    entries: impl IntoIterator<Item = (&'static str, String)>,
) -> HashMap<String, String> {
    let mut metadata = build_string_metadata(entries);
    metadata.insert("content-type".to_string(), content_type.to_string());
    metadata
}

pub async fn load_relational_fact_envelopes_by_type(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    type_id: &str,
) -> AgentResult<Vec<FactEnvelope>> {
    let facts = effects
        .load_committed_facts(authority)
        .await
        .map_err(|error| crate::core::AgentError::effects(error.to_string()))?;

    Ok(facts
        .into_iter()
        .rev()
        .filter_map(|fact| match fact.content {
            FactContent::Relational(RelationalFact::Generic { envelope, .. })
                if envelope.type_id.as_str() == type_id =>
            {
                Some(envelope)
            }
            _ => None,
        })
        .collect())
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

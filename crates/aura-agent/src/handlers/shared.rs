//! Shared Handler Utilities
//!
//! Common utilities used by domain-specific handlers.

use crate::core::{default_context_id_for_authority, AgentResult, AuthorityContext};
use crate::runtime::{AuraEffectSystem, EffectContext};
use aura_core::types::facts::FactEnvelope;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{FactContent, RelationalFact};
use aura_journal::FactJournal;
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
    /// Append a domain fact (e.g. `InvitationFact`) into the authority-scoped
    /// journal under its own type id and schema version.
    pub async fn append_domain_fact<F: aura_journal::DomainFact>(
        authority: &AuthorityContext,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        fact: &F,
    ) -> AgentResult<()> {
        let _ = authority; // Authority is implied by the effect system's configured identity.
        effects
            .commit_domain_fact(context_id, fact)
            .await
            .map(|_| ())
            .map_err(crate::core::AgentError::from)
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

pub fn map_handler_effect_error(
    label: &'static str,
    error: impl Display,
) -> crate::core::AgentError {
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

fn stamping_failure(
    message: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> crate::core::AgentError {
    crate::core::AgentError::Aura(aura_core::AuraError::Internal {
        message: message.into(),
        source: Some(std::sync::Arc::new(source)),
    })
}

/// Advance the runtime's logical clock past `observed` facts of one
/// order-independent family: the shared stamping step for causal metadata
/// (docs/105_journal.md §4.2.1).
pub async fn advance_clock_past<F: aura_journal::causal_reduction::CausalFact>(
    effects: &AuraEffectSystem,
    observed: &[F],
) -> AgentResult<aura_core::time::LogicalTime> {
    use aura_core::effects::time::LogicalClockEffects;
    let vector = aura_journal::causal_reduction::merged_vector(
        observed.iter().map(|fact| &fact.causal_metadata().clock),
    );
    effects
        .logical_advance(Some(&vector))
        .await
        .map_err(|source| stamping_failure("advance logical clock", source))
}

/// The shared stamping step: advance past `observed`, then derive the new
/// fact's metadata from the family's pure rule at the advanced clock.
pub async fn stamp_after<F: aura_journal::causal_reduction::CausalFact>(
    effects: &AuraEffectSystem,
    observed: &[F],
    causal: impl FnOnce(&aura_core::time::LogicalTime) -> aura_core::time::CausalMetadata,
) -> AgentResult<aura_core::time::CausalMetadata> {
    Ok(causal(&advance_clock_past(effects, observed).await?))
}

/// Every committed fact of `type_id` in `authority`'s journal, decoded.
async fn load_decoded_facts<T, E>(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    type_id: &'static str,
    decode: impl Fn(&aura_core::types::facts::FactEnvelope) -> Result<T, E>,
) -> AgentResult<Vec<T>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    load_relational_fact_envelopes_by_type(effects, authority, type_id)
        .await?
        .iter()
        .map(|envelope| {
            decode(envelope).map_err(|source| stamping_failure("decode committed fact", source))
        })
        .collect()
}

/// Every committed contact fact of `authority`, tagged.
pub async fn load_tagged_contact_facts(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
) -> AgentResult<Vec<aura_relational::TaggedContactFact>> {
    load_decoded_facts(
        effects,
        authority,
        aura_relational::CONTACT_FACT_TYPE_ID,
        |envelope| {
            aura_relational::ContactFact::try_from_envelope(envelope)
                .map(aura_relational::TaggedContactFact::new)
        },
    )
    .await
}

/// Causal metadata for a new contact fact about `key` in `authority`'s
/// contact list, observing every committed contact fact.
pub async fn stamp_contact_causal(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    key: aura_relational::ContactCausalKey,
) -> AgentResult<aura_core::time::CausalMetadata> {
    let observed = load_tagged_contact_facts(effects, authority).await?;
    stamp_after(effects, &observed, |clock| {
        aura_relational::contact_causal(key, &observed, clock)
    })
    .await
}

/// Causal metadata for a new recovery initiation of `kind` in `context_id`,
/// observing every initiation of that family committed in `authority`'s
/// journal.
pub async fn stamp_recovery_initiation_causal(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    context_id: aura_core::types::identifiers::ContextId,
    kind: aura_recovery::RecoveryInitiationKind,
) -> AgentResult<aura_core::time::CausalMetadata> {
    let observed = load_decoded_facts(
        effects,
        authority,
        aura_recovery::RECOVERY_FACT_TYPE_ID,
        aura_recovery::RecoveryFact::try_from_envelope,
    )
    .await?;
    aura_recovery::stamp_recovery_initiation(effects, context_id, kind, &observed)
        .await
        .map_err(|source| stamping_failure("stamp recovery initiation", source))
}

/// Causal metadata for a new terminal outcome (accept, decline, cancel) of
/// `invitation_id`, observing every outcome of it committed in `authority`'s
/// journal.
///
/// The journal scan and stamping state machine is boxed so the accept,
/// decline and cancel flows that await it keep a bounded future size.
pub async fn stamp_invitation_outcome_causal(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    invitation_id: &aura_core::types::identifiers::InvitationId,
) -> AgentResult<aura_core::time::CausalMetadata> {
    Box::pin(async move {
        let observed = load_decoded_facts(
            effects,
            authority,
            aura_invitation::INVITATION_FACT_TYPE_ID,
            aura_invitation::InvitationFact::try_from_envelope,
        )
        .await?;
        aura_invitation::stamp_invitation_outcome(effects, invitation_id, &observed)
            .await
            .map_err(|source| stamping_failure("stamp invitation outcome", source))
    })
    .await
}

/// Causal metadata for a new friendship fact about `key`, observing every
/// friendship fact committed in `authority`'s journal.
pub async fn stamp_friendship_causal(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    key: aura_relational::FriendshipCausalKey,
) -> AgentResult<aura_core::time::CausalMetadata> {
    let observed = load_decoded_facts(
        effects,
        authority,
        aura_relational::FRIENDSHIP_FACT_TYPE_ID,
        |envelope| {
            aura_relational::FriendshipFact::try_from_envelope(envelope)
                .map(aura_relational::TaggedFriendshipFact::new)
        },
    )
    .await?;
    stamp_after(effects, &observed, |clock| {
        aura_relational::friendship_causal(key, &observed, clock)
    })
    .await
}

/// Causal metadata for a new edit or delete of the message `key`, observing
/// every edit and delete committed in `authority`'s journal.
pub async fn stamp_message_revision_causal(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    key: aura_chat::MessageRevisionKey,
) -> AgentResult<aura_core::time::CausalMetadata> {
    let observed: Vec<_> = load_decoded_facts(
        effects,
        authority,
        aura_chat::CHAT_FACT_TYPE_ID,
        aura_chat::ChatFact::try_from_envelope,
    )
    .await?
    .iter()
    .filter_map(aura_chat::MessageRevision::from_fact)
    .collect();
    stamp_after(effects, &observed, |clock| {
        aura_chat::message_revision_causal(key, &observed, clock)
    })
    .await
}

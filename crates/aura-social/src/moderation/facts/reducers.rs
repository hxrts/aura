//! Moderation fact reducers and registration

use super::constants::*;
use super::fact_types::*;
use crate::error::SocialError;
use crate::facts::ModeratorCapability;
use crate::home::Home;
use aura_core::types::facts::FactEnvelope;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};
use std::marker::PhantomData;

/// State-aware authorization context for moderator grant/revoke facts.
///
/// This validator is evaluated against the materialized home state immediately
/// before the fact's emission epoch. The envelope signer must be supplied by
/// the journal/attestation layer because `FactEnvelope` deliberately contains
/// only the canonical domain payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeratorChangeAuthority {
    /// Authority that signed or otherwise attested the fact envelope.
    pub envelope_signer: AuthorityId,
    /// Home owner/creator authority, which may grant or revoke moderators.
    pub owner_authority: AuthorityId,
}

fn validate_moderator_change_authority(
    home: &Home,
    authority: ModeratorChangeAuthority,
    actor_authority: AuthorityId,
    target_authority: AuthorityId,
) -> Result<(), SocialError> {
    if authority.envelope_signer != actor_authority {
        return Err(SocialError::MissingCapability(
            "moderator fact signer must match actor_authority".to_string(),
        ));
    }

    if !home.is_member(&target_authority) {
        return Err(SocialError::not_member(home.home_id));
    }

    if actor_authority == authority.owner_authority {
        return Ok(());
    }

    if home.has_moderator_capability(&actor_authority, ModeratorCapability::GrantModerator) {
        return Ok(());
    }

    Err(SocialError::MissingCapability(
        "moderator GrantModerator capability required".to_string(),
    ))
}

/// Validate a moderator grant fact against pre-emission home state.
pub fn validate_moderator_grant_fact(
    home: &Home,
    authority: ModeratorChangeAuthority,
    fact: &HomeGrantModeratorFact,
) -> Result<(), SocialError> {
    validate_moderator_change_authority(
        home,
        authority,
        fact.actor_authority,
        fact.target_authority,
    )
}

/// Validate a moderator revoke fact against pre-emission home state.
pub fn validate_moderator_revoke_fact(
    home: &Home,
    authority: ModeratorChangeAuthority,
    fact: &HomeRevokeModeratorFact,
) -> Result<(), SocialError> {
    validate_moderator_change_authority(
        home,
        authority,
        fact.actor_authority,
        fact.target_authority,
    )?;

    if !home.is_moderator(&fact.target_authority) {
        return Err(SocialError::not_moderator(home.home_id));
    }

    Ok(())
}

fn reduce_moderation_envelope<T: DomainFact>(
    context_id: ContextId,
    envelope: &FactEnvelope,
    type_id: &'static str,
) -> Option<RelationalBinding> {
    if envelope.type_id.as_str() != type_id {
        return None;
    }

    let _fact = T::from_envelope(envelope)?;
    Some(RelationalBinding {
        binding_type: RelationalBindingType::Generic(type_id.to_string()),
        context_id,
        data: envelope.payload.clone(),
    })
}

struct ModerationFactReducer<T> {
    type_id: &'static str,
    _marker: PhantomData<fn() -> T>,
}

impl<T> ModerationFactReducer<T> {
    const fn new(type_id: &'static str) -> Self {
        Self {
            type_id,
            _marker: PhantomData,
        }
    }
}

impl<T: DomainFact> FactReducer for ModerationFactReducer<T> {
    fn handles_type(&self) -> &'static str {
        self.type_id
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_moderation_envelope::<T>(context_id, envelope, self.type_id)
    }
}

/// Register moderation domain facts with the journal registry.
pub fn register_moderation_facts(registry: &mut FactRegistry) {
    registry.register::<HomeMuteFact>(
        HOME_MUTE_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeMuteFact>::new(
            HOME_MUTE_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeUnmuteFact>(
        HOME_UNMUTE_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeUnmuteFact>::new(
            HOME_UNMUTE_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeBanFact>(
        HOME_BAN_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeBanFact>::new(
            HOME_BAN_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeUnbanFact>(
        HOME_UNBAN_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeUnbanFact>::new(
            HOME_UNBAN_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeKickFact>(
        HOME_KICK_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeKickFact>::new(
            HOME_KICK_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomePinFact>(
        HOME_PIN_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomePinFact>::new(
            HOME_PIN_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeUnpinFact>(
        HOME_UNPIN_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeUnpinFact>::new(
            HOME_UNPIN_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeGrantModeratorFact>(
        HOME_GRANT_MODERATOR_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeGrantModeratorFact>::new(
            HOME_GRANT_MODERATOR_FACT_TYPE_ID,
        )),
    );
    registry.register::<HomeRevokeModeratorFact>(
        HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
        Box::new(ModerationFactReducer::<HomeRevokeModeratorFact>::new(
            HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
        )),
    );
}

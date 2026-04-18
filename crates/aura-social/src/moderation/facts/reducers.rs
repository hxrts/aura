//! Moderation fact reducers and registration

use super::constants::*;
use super::fact_types::*;
use aura_core::types::facts::FactEnvelope;
use aura_core::types::identifiers::ContextId;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};
use std::marker::PhantomData;

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

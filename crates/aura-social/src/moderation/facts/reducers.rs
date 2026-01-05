//! Moderation fact reducers and registration

use super::constants::*;
use super::fact_types::*;
use aura_core::identifiers::ContextId;
use aura_core::types::facts::FactEnvelope;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};

struct HomeMuteFactReducer;

impl FactReducer for HomeMuteFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_MUTE_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_MUTE_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeMuteFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_MUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomeUnmuteFactReducer;

impl FactReducer for HomeUnmuteFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNMUTE_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_UNMUTE_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeUnmuteFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNMUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomeBanFactReducer;

impl FactReducer for HomeBanFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_BAN_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_BAN_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeBanFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_BAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomeUnbanFactReducer;

impl FactReducer for HomeUnbanFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNBAN_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_UNBAN_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeUnbanFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNBAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomeKickFactReducer;

impl FactReducer for HomeKickFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_KICK_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_KICK_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeKickFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_KICK_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomePinFactReducer;

impl FactReducer for HomePinFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_PIN_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_PIN_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomePinFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_PIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

struct HomeUnpinFactReducer;

impl FactReducer for HomeUnpinFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNPIN_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != HOME_UNPIN_FACT_TYPE_ID {
            return None;
        }

        let _fact = HomeUnpinFact::from_envelope(envelope)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNPIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: envelope.payload.clone(),
        })
    }
}

/// Register moderation domain facts with the journal registry.
pub fn register_moderation_facts(registry: &mut FactRegistry) {
    registry.register::<HomeMuteFact>(HOME_MUTE_FACT_TYPE_ID, Box::new(HomeMuteFactReducer));
    registry.register::<HomeUnmuteFact>(HOME_UNMUTE_FACT_TYPE_ID, Box::new(HomeUnmuteFactReducer));
    registry.register::<HomeBanFact>(HOME_BAN_FACT_TYPE_ID, Box::new(HomeBanFactReducer));
    registry.register::<HomeUnbanFact>(HOME_UNBAN_FACT_TYPE_ID, Box::new(HomeUnbanFactReducer));
    registry.register::<HomeKickFact>(HOME_KICK_FACT_TYPE_ID, Box::new(HomeKickFactReducer));
    registry.register::<HomePinFact>(HOME_PIN_FACT_TYPE_ID, Box::new(HomePinFactReducer));
    registry.register::<HomeUnpinFact>(HOME_UNPIN_FACT_TYPE_ID, Box::new(HomeUnpinFactReducer));
}

//! Moderation fact reducers and registration

use super::constants::*;
use super::fact_types::*;
use aura_core::identifiers::ContextId;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};

struct HomeMuteFactReducer;

impl FactReducer for HomeMuteFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_MUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_MUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeMuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_MUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomeUnmuteFactReducer;

impl FactReducer for HomeUnmuteFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNMUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_UNMUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeUnmuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNMUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomeBanFactReducer;

impl FactReducer for HomeBanFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_BAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_BAN_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeBanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_BAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomeUnbanFactReducer;

impl FactReducer for HomeUnbanFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNBAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_UNBAN_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeUnbanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNBAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomeKickFactReducer;

impl FactReducer for HomeKickFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_KICK_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_KICK_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeKickFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_KICK_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomePinFactReducer;

impl FactReducer for HomePinFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_PIN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_PIN_FACT_TYPE_ID {
            return None;
        }

        let fact = HomePinFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_PIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct HomeUnpinFactReducer;

impl FactReducer for HomeUnpinFactReducer {
    fn handles_type(&self) -> &'static str {
        HOME_UNPIN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != HOME_UNPIN_FACT_TYPE_ID {
            return None;
        }

        let fact = HomeUnpinFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(HOME_UNPIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

/// Register moderation domain facts with the journal registry.
pub fn register_moderation_facts(registry: &mut FactRegistry) {
    registry.register::<HomeMuteFact>(HOME_MUTE_FACT_TYPE_ID, Box::new(HomeMuteFactReducer));
    registry
        .register::<HomeUnmuteFact>(HOME_UNMUTE_FACT_TYPE_ID, Box::new(HomeUnmuteFactReducer));
    registry.register::<HomeBanFact>(HOME_BAN_FACT_TYPE_ID, Box::new(HomeBanFactReducer));
    registry.register::<HomeUnbanFact>(HOME_UNBAN_FACT_TYPE_ID, Box::new(HomeUnbanFactReducer));
    registry.register::<HomeKickFact>(HOME_KICK_FACT_TYPE_ID, Box::new(HomeKickFactReducer));
    registry.register::<HomePinFact>(HOME_PIN_FACT_TYPE_ID, Box::new(HomePinFactReducer));
    registry.register::<HomeUnpinFact>(HOME_UNPIN_FACT_TYPE_ID, Box::new(HomeUnpinFactReducer));
}

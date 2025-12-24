//! Moderation fact reducers and registration

use super::constants::*;
use super::fact_types::*;
use aura_core::identifiers::ContextId;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};

struct BlockMuteFactReducer;

impl FactReducer for BlockMuteFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_MUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_MUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockMuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_MUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockUnmuteFactReducer;

impl FactReducer for BlockUnmuteFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_UNMUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_UNMUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockUnmuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_UNMUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockBanFactReducer;

impl FactReducer for BlockBanFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_BAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_BAN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockBanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_BAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockUnbanFactReducer;

impl FactReducer for BlockUnbanFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_UNBAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_UNBAN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockUnbanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_UNBAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockKickFactReducer;

impl FactReducer for BlockKickFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_KICK_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_KICK_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockKickFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_KICK_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockPinFactReducer;

impl FactReducer for BlockPinFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_PIN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_PIN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockPinFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_PIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockUnpinFactReducer;

impl FactReducer for BlockUnpinFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_UNPIN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_UNPIN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockUnpinFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_UNPIN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

/// Register moderation domain facts with the journal registry.
pub fn register_moderation_facts(registry: &mut FactRegistry) {
    registry.register::<BlockMuteFact>(BLOCK_MUTE_FACT_TYPE_ID, Box::new(BlockMuteFactReducer));
    registry
        .register::<BlockUnmuteFact>(BLOCK_UNMUTE_FACT_TYPE_ID, Box::new(BlockUnmuteFactReducer));
    registry.register::<BlockBanFact>(BLOCK_BAN_FACT_TYPE_ID, Box::new(BlockBanFactReducer));
    registry.register::<BlockUnbanFact>(BLOCK_UNBAN_FACT_TYPE_ID, Box::new(BlockUnbanFactReducer));
    registry.register::<BlockKickFact>(BLOCK_KICK_FACT_TYPE_ID, Box::new(BlockKickFactReducer));
    registry.register::<BlockPinFact>(BLOCK_PIN_FACT_TYPE_ID, Box::new(BlockPinFactReducer));
    registry.register::<BlockUnpinFact>(BLOCK_UNPIN_FACT_TYPE_ID, Box::new(BlockUnpinFactReducer));
}

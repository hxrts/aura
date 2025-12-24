//! Moderation domain facts for block-level moderation actions

mod constants;
mod fact_types;
mod reducers;

// Re-export constants
pub use constants::{
    BLOCK_BAN_FACT_TYPE_ID, BLOCK_GRANT_STEWARD_FACT_TYPE_ID, BLOCK_KICK_FACT_TYPE_ID,
    BLOCK_MUTE_FACT_TYPE_ID, BLOCK_PIN_FACT_TYPE_ID, BLOCK_REVOKE_STEWARD_FACT_TYPE_ID,
    BLOCK_UNBAN_FACT_TYPE_ID, BLOCK_UNMUTE_FACT_TYPE_ID, BLOCK_UNPIN_FACT_TYPE_ID,
};

// Re-export fact types
pub use fact_types::{
    BlockBanFact, BlockGrantStewardFact, BlockKickFact, BlockMuteFact, BlockPinFact,
    BlockRevokeStewardFact, BlockUnbanFact, BlockUnmuteFact, BlockUnpinFact,
};

// Re-export registration function
pub use reducers::register_moderation_facts;

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use aura_core::time::PhysicalTime;
    use aura_journal::reduction::RelationalBindingType;
    use aura_journal::{DomainFact, FactRegistry};

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([7u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn moderation_facts_register_with_registry() {
        let mut registry = FactRegistry::new();
        register_moderation_facts(&mut registry);

        assert!(registry.is_registered(BLOCK_MUTE_FACT_TYPE_ID));
        assert!(registry.is_registered(BLOCK_UNMUTE_FACT_TYPE_ID));
        assert!(registry.is_registered(BLOCK_PIN_FACT_TYPE_ID));
        assert!(registry.is_registered(BLOCK_UNPIN_FACT_TYPE_ID));

        let context_id = test_context_id();
        let block_mute = BlockMuteFact {
            context_id,
            channel_id: None,
            muted_authority: test_authority_id(1),
            actor_authority: test_authority_id(2),
            duration_secs: Some(30),
            muted_at: pt(1000),
            expires_at: Some(pt(31000)),
        };

        let binding = registry.reduce_generic(
            block_mute.context_id,
            BLOCK_MUTE_FACT_TYPE_ID,
            &block_mute.to_bytes(),
        );

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(BLOCK_MUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, block_mute.to_bytes());

        let block_unmute = BlockUnmuteFact {
            context_id,
            channel_id: None,
            unmuted_authority: block_mute.muted_authority,
            actor_authority: block_mute.actor_authority,
            unmuted_at: pt(2000),
        };

        let binding = registry.reduce_generic(
            block_unmute.context_id,
            BLOCK_UNMUTE_FACT_TYPE_ID,
            &block_unmute.to_bytes(),
        );

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(BLOCK_UNMUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, block_unmute.to_bytes());
    }
}

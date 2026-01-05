//! Moderation domain facts for home-level moderation actions

mod constants;
mod fact_types;
mod reducers;

// Re-export constants
pub use constants::{
    HOME_BAN_FACT_TYPE_ID, HOME_GRANT_STEWARD_FACT_TYPE_ID, HOME_KICK_FACT_TYPE_ID,
    HOME_MUTE_FACT_TYPE_ID, HOME_PIN_FACT_TYPE_ID, HOME_REVOKE_STEWARD_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID, HOME_UNPIN_FACT_TYPE_ID,
};

// Re-export fact types
pub use fact_types::{
    HomeBanFact, HomeGrantStewardFact, HomeKickFact, HomeMuteFact, HomePinFact,
    HomeRevokeStewardFact, HomeUnbanFact, HomeUnmuteFact, HomeUnpinFact,
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

        assert!(registry.is_registered(HOME_MUTE_FACT_TYPE_ID));
        assert!(registry.is_registered(HOME_UNMUTE_FACT_TYPE_ID));
        assert!(registry.is_registered(HOME_PIN_FACT_TYPE_ID));
        assert!(registry.is_registered(HOME_UNPIN_FACT_TYPE_ID));

        let context_id = test_context_id();
        let home_mute = HomeMuteFact {
            context_id,
            channel_id: None,
            muted_authority: test_authority_id(1),
            actor_authority: test_authority_id(2),
            duration_secs: Some(30),
            muted_at: pt(1000),
            expires_at: Some(pt(31000)),
        };

        let binding = registry.reduce_envelope(home_mute.context_id, &home_mute.to_envelope());

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(HOME_MUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, home_mute.to_bytes());

        let home_unmute = HomeUnmuteFact {
            context_id,
            channel_id: None,
            unmuted_authority: home_mute.muted_authority,
            actor_authority: home_mute.actor_authority,
            unmuted_at: pt(2000),
        };

        let binding = registry.reduce_envelope(home_unmute.context_id, &home_unmute.to_envelope());

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(HOME_UNMUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, home_unmute.to_bytes());
    }
}

//! Moderation domain facts for home-level moderation actions

mod constants;
mod fact_types;
mod reducers;

// Re-export constants
pub use constants::{
    HOME_BAN_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID, HOME_KICK_FACT_TYPE_ID,
    HOME_MUTE_FACT_TYPE_ID, HOME_PIN_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID, HOME_UNPIN_FACT_TYPE_ID,
};

// Re-export fact types
pub use fact_types::{
    HomeBanFact, HomeGrantModeratorFact, HomeKickFact, HomeMuteFact, HomePinFact,
    HomeRevokeModeratorFact, HomeUnbanFact, HomeUnmuteFact, HomeUnpinFact,
};

// Re-export registration and state-aware validation functions
pub use reducers::{
    register_moderation_facts, validate_moderator_grant_fact, validate_moderator_revoke_fact,
    ModeratorChangeAuthority,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{HomeFact, HomeId, HomeMemberFact, ModeratorCapabilities, ModeratorFact};
    use crate::home::Home;
    use aura_core::time::PhysicalTime;
    use aura_core::time::TimeStamp;
    use aura_core::types::identifiers::{AuthorityId, ContextId};
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

    fn ts(ts_ms: u64) -> TimeStamp {
        TimeStamp::PhysicalClock(pt(ts_ms))
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

    #[test]
    fn moderator_grant_validation_rejects_non_moderator_actor() {
        let home_id = HomeId::from_bytes([9u8; 32]);
        let owner = test_authority_id(1);
        let actor = test_authority_id(2);
        let target = test_authority_id(3);
        let home_fact = HomeFact::new(home_id, ts(1));
        let members = vec![
            HomeMemberFact::new(owner, home_id, ts(1)),
            HomeMemberFact::new(actor, home_id, ts(2)),
            HomeMemberFact::new(target, home_id, ts(3)),
        ];
        let home = Home::from_facts(&home_fact, None, &members, &[]);
        let fact = HomeGrantModeratorFact::new_ms(test_context_id(), target, actor, 10);

        let error = validate_moderator_grant_fact(
            &home,
            ModeratorChangeAuthority {
                envelope_signer: actor,
                owner_authority: owner,
            },
            &fact,
        )
        .expect_err("non-moderator actor must not grant moderators");

        assert!(matches!(
            error,
            crate::error::SocialError::MissingCapability(_)
        ));
    }

    #[test]
    fn moderator_grant_validation_requires_envelope_signer_to_match_actor() {
        let home_id = HomeId::from_bytes([10u8; 32]);
        let owner = test_authority_id(1);
        let target = test_authority_id(2);
        let home_fact = HomeFact::new(home_id, ts(1));
        let members = vec![
            HomeMemberFact::new(owner, home_id, ts(1)),
            HomeMemberFact::new(target, home_id, ts(2)),
        ];
        let moderator = ModeratorFact {
            authority_id: owner,
            home_id,
            granted_at: ts(1),
            capabilities: ModeratorCapabilities::full(),
        };
        let home = Home::from_facts(&home_fact, None, &members, &[moderator]);
        let fact = HomeGrantModeratorFact::new_ms(test_context_id(), target, owner, 10);

        let error = validate_moderator_grant_fact(
            &home,
            ModeratorChangeAuthority {
                envelope_signer: test_authority_id(9),
                owner_authority: owner,
            },
            &fact,
        )
        .expect_err("fact signer must match actor_authority");

        assert!(matches!(
            error,
            crate::error::SocialError::MissingCapability(_)
        ));
    }
}

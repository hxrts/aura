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

// Re-export registration function
pub use reducers::register_moderation_facts;

/// The actor a moderation fact claims as its author, or `None` for any other
/// fact type. Ingress compares this with the authenticated sender so a peer
/// cannot author moderation in another authority's name.
pub fn claimed_moderation_actor(
    envelope: &aura_core::types::facts::FactEnvelope,
) -> Option<aura_core::types::identifiers::AuthorityId> {
    use aura_journal::DomainFact;
    match envelope.type_id.as_str() {
        HOME_BAN_FACT_TYPE_ID => HomeBanFact::from_envelope(envelope).map(|f| f.actor_authority),
        HOME_UNBAN_FACT_TYPE_ID => {
            HomeUnbanFact::from_envelope(envelope).map(|f| f.actor_authority)
        }
        HOME_MUTE_FACT_TYPE_ID => HomeMuteFact::from_envelope(envelope).map(|f| f.actor_authority),
        HOME_UNMUTE_FACT_TYPE_ID => {
            HomeUnmuteFact::from_envelope(envelope).map(|f| f.actor_authority)
        }
        HOME_KICK_FACT_TYPE_ID => HomeKickFact::from_envelope(envelope).map(|f| f.actor_authority),
        HOME_PIN_FACT_TYPE_ID => HomePinFact::from_envelope(envelope).map(|f| f.actor_authority),
        HOME_UNPIN_FACT_TYPE_ID => {
            HomeUnpinFact::from_envelope(envelope).map(|f| f.actor_authority)
        }
        HOME_GRANT_MODERATOR_FACT_TYPE_ID => {
            HomeGrantModeratorFact::from_envelope(envelope).map(|f| f.actor_authority)
        }
        HOME_REVOKE_MODERATOR_FACT_TYPE_ID => {
            HomeRevokeModeratorFact::from_envelope(envelope).map(|f| f.actor_authority)
        }
        crate::facts::SOCIAL_FACT_TYPE_ID => {
            match crate::facts::SocialFact::from_envelope(envelope) {
                Some(
                    crate::facts::SocialFact::AccessOverrideSet { actor_id, .. }
                    | crate::facts::SocialFact::AccessLevelCapabilitiesConfigured {
                        actor_id, ..
                    },
                ) => Some(actor_id),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use aura_core::types::identifiers::{AuthorityId, ContextId};
    use aura_journal::reduction::RelationalBindingType;
    use aura_journal::{DomainFact, FactRegistry};

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([7u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn stamp(device: u8) -> aura_core::time::CausalMetadata {
        crate::moderation::governance::test_support::causal(
            device,
            crate::moderation::governance::HomeGovernanceKey::CapabilityConfig,
            &[],
        )
    }

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn claimed_moderation_actor_reads_actor_and_ignores_other_facts() {
        let actor = test_authority_id(2);
        let ban = HomeBanFact::new_ms(
            test_context_id(),
            None,
            test_authority_id(3),
            actor,
            "spam".to_string(),
            1,
            None,
            stamp(1),
        );
        assert_eq!(claimed_moderation_actor(&ban.to_envelope()), Some(actor));

        let mut other = ban.to_envelope();
        other.type_id = aura_core::types::facts::FactTypeId::from("chat:message");
        assert_eq!(claimed_moderation_actor(&other), None);
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
            causal: stamp(2),
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
            causal: stamp(3),
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

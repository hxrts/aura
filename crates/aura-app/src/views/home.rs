//! # Home View State
//!
//! This module contains home state types including moderation functionality
//! (bans, mutes, kicks) that were previously in TUI-only demo code.

mod members;
mod moderation;
mod serde_support;
mod state;

pub use members::{HomeMember, HomeRole};
pub use moderation::{BanRecord, KickRecord, MuteRecord, PinnedMessageMeta};
pub use state::{AddHomeResult, HomeCreationWitness, HomeState, HomesState, RemoveHomeResult};

#[cfg(test)]
mod tests {
    use super::{BanRecord, HomeMember, HomeRole, HomeState, HomesState, MuteRecord};
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use proptest::prelude::*;

    fn role_strategy() -> impl Strategy<Value = HomeRole> {
        prop_oneof![
            Just(HomeRole::Participant),
            Just(HomeRole::Member),
            Just(HomeRole::Moderator),
        ]
    }

    fn home_strategy() -> impl Strategy<Value = Vec<HomeMember>> {
        prop::collection::vec(role_strategy(), 1..40).prop_map(|roles| {
            roles
                .into_iter()
                .enumerate()
                .map(|(index, role)| {
                    let mut entropy = [0u8; 32];
                    entropy[0] = index as u8;
                    HomeMember {
                        id: AuthorityId::new_from_entropy(entropy),
                        name: format!("member-{index}"),
                        role,
                        is_online: true,
                        joined_at: index as u64,
                        last_seen: Some(index as u64),
                        storage_allocated: 0,
                    }
                })
                .collect()
        })
    }

    #[test]
    fn add_home_does_not_implicitly_select() {
        let mut homes = HomesState::new();
        let authority = AuthorityId::new_from_entropy([7u8; 32]);
        let home_id = ChannelId::from_bytes([3u8; 32]);
        let context = ContextId::new_from_entropy([4u8; 32]);
        let home = HomeState::new(home_id, Some("primary".to_string()), authority, 10, context);

        let result = homes.add_home(home);

        assert_eq!(result.home_id, home_id);
        assert!(result.was_first);
        assert_eq!(homes.current_home_id(), None);
    }

    #[test]
    fn remove_home_clears_selection_without_fallback() {
        let authority = AuthorityId::new_from_entropy([1u8; 32]);
        let first_home_id = ChannelId::from_bytes([10u8; 32]);
        let second_home_id = ChannelId::from_bytes([11u8; 32]);
        let first_context = ContextId::new_from_entropy([2u8; 32]);
        let second_context = ContextId::new_from_entropy([3u8; 32]);

        let mut homes = HomesState::new();
        homes.add_home(HomeState::new(
            first_home_id,
            Some("first".to_string()),
            authority,
            1,
            first_context,
        ));
        homes.add_home(HomeState::new(
            second_home_id,
            Some("second".to_string()),
            authority,
            2,
            second_context,
        ));
        homes.select_home(Some(first_home_id));

        let result = homes.remove_home(&first_home_id);

        assert!(result.was_selected);
        assert_eq!(homes.current_home_id(), None);
        assert!(homes.has_home(&second_home_id));
    }

    #[test]
    fn nonempty_homes_json_roundtrip_preserves_ids_counts_and_selection() {
        let creator = AuthorityId::new_from_entropy([31u8; 32]);
        let first = ChannelId::from_bytes([32u8; 32]);
        let second = ChannelId::from_bytes([33u8; 32]);
        let mut homes = HomesState::new();
        homes.add_home(HomeState::new(
            first,
            Some("First".into()),
            creator,
            1,
            ContextId::new_from_entropy([34u8; 32]),
        ));
        let target = AuthorityId::new_from_entropy([36u8; 32]);
        let mut second_home = HomeState::new(
            second,
            Some("Second".into()),
            creator,
            2,
            ContextId::new_from_entropy([35u8; 32]),
        );
        second_home.set_access_override(target, aura_social::AccessLevel::Partial);
        second_home.add_ban(BanRecord {
            authority_id: target,
            reason: "test".into(),
            actor: creator,
            banned_at: 3,
        });
        second_home.add_mute(MuteRecord {
            authority_id: target,
            duration_secs: Some(10),
            muted_at: 3,
            expires_at: Some(13),
            actor: creator,
        });
        homes.add_home(second_home);
        homes.select_home(Some(second));

        let encoded = serde_json::to_string(&homes).unwrap();
        let restored: HomesState = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.iter().count(), 2);
        assert_eq!(restored.current_home_id(), Some(&second));
        assert_eq!(restored.home_state(&first).unwrap().member_count, 1);
        let restored_second = restored.home_state(&second).unwrap();
        assert_eq!(restored_second.name, "Second");
        assert_eq!(
            restored_second.access_override(&target),
            Some(aura_social::AccessLevel::Partial)
        );
        assert_eq!(
            restored_second.ban_list.get(&target).unwrap().reason,
            "test"
        );
        assert_eq!(
            restored_second.mute_list.get(&target).unwrap().expires_at,
            Some(13)
        );
    }

    #[test]
    fn homes_json_rejects_a_map_key_that_differs_from_the_entity_id() {
        let creator = AuthorityId::new_from_entropy([41u8; 32]);
        let home = HomeState::new(
            ChannelId::from_bytes([42u8; 32]),
            Some("Den".into()),
            creator,
            1,
            ContextId::new_from_entropy([43u8; 32]),
        );
        let wrong_id = ChannelId::from_bytes([44u8; 32]);
        let malformed = serde_json::json!({
            "homes": { wrong_id.to_string(): home },
            "current_home_id": null,
        });
        assert!(serde_json::from_value::<HomesState>(malformed).is_err());
    }

    proptest! {
        #[test]
        fn threshold_member_count_matches_roles(members in home_strategy()) {
            let threshold_count = members.iter().filter(|member| member.role.is_threshold_member()).count();
            let participant_count = members.iter().filter(|member| member.role.is_participant()).count();

            prop_assert_eq!(threshold_count + participant_count, members.len());
        }
    }
}

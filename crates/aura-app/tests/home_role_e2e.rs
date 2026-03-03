//! Integration tests for home role semantics and moderator behavior.

use aura_app::views::{HomeRole, HomeState, KickRecord, Resident};
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};

fn authority_id(seed: u8) -> AuthorityId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    AuthorityId::new_from_entropy(bytes)
}

fn channel_id(seed: u8) -> ChannelId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    ChannelId::from_bytes(bytes)
}

fn context_id(seed: u8) -> ContextId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    ContextId::new_from_entropy(bytes)
}

#[test]
fn test_home_creator_is_regular_member() {
    let creator = authority_id(1);
    let home = HomeState::new(
        channel_id(1),
        Some("Neighborhood One".to_string()),
        creator,
        1_700_000_000_000,
        context_id(1),
    );

    assert_eq!(home.my_role, HomeRole::Member);
    let creator_entry = home
        .residents
        .iter()
        .find(|resident| resident.id == creator)
        .expect("creator should exist in residents");
    assert_eq!(creator_entry.role, HomeRole::Member);
}

#[test]
fn test_moderator_can_kick() {
    let creator = authority_id(1);
    let moderator = authority_id(2);
    let participant = authority_id(3);

    let mut home = HomeState::new(
        channel_id(2),
        Some("Moderation Home".to_string()),
        creator,
        1_700_000_000_000,
        context_id(2),
    );

    home.add_resident(Resident {
        id: moderator,
        name: "Bob".to_string(),
        role: HomeRole::Moderator,
        is_online: true,
        joined_at: 1_700_000_000_100,
        last_seen: Some(1_700_000_000_100),
        storage_allocated: HomeState::RESIDENT_ALLOCATION,
    });
    home.add_resident(Resident {
        id: participant,
        name: "Carol".to_string(),
        role: HomeRole::Participant,
        is_online: true,
        joined_at: 1_700_000_000_200,
        last_seen: Some(1_700_000_000_200),
        storage_allocated: HomeState::RESIDENT_ALLOCATION,
    });

    home.my_role = HomeRole::Moderator;
    assert!(home.can_moderate());

    let removed = home.remove_resident(&participant);
    assert!(removed.is_some());

    home.add_kick(KickRecord {
        authority_id: participant,
        channel: home.id,
        reason: "moderator action".to_string(),
        actor: moderator,
        kicked_at: 1_700_000_000_300,
    });

    let latest = home
        .kick_log
        .last()
        .expect("kick log should contain record");
    assert_eq!(latest.authority_id, participant);
    assert_eq!(latest.actor, moderator);
}

#[test]
fn test_member_vs_participant_semantics() {
    assert!(HomeRole::Member.is_threshold_member());
    assert!(HomeRole::Moderator.is_threshold_member());
    assert!(!HomeRole::Participant.is_threshold_member());

    assert!(HomeRole::Participant.is_participant());
    assert!(!HomeRole::Member.is_participant());
}

#[test]
fn test_only_members_can_hold_moderator_designation() {
    assert!(HomeRole::Moderator.is_threshold_member());
    assert!(!HomeRole::Participant.is_threshold_member());
}

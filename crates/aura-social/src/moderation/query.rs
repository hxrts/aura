#![allow(clippy::clone_on_copy)]

//! Query functions for deriving moderation state from journal facts

use super::facts::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_UNBAN_FACT_TYPE_ID,
    HOME_UNMUTE_FACT_TYPE_ID,
};
use super::types::{BanStatus, KickRecord, ModerationScopeKey, MuteStatus};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use std::collections::{HashMap, HashSet};

fn moderation_fact<T: DomainFact>(
    fact: &Fact,
    context_id: &ContextId,
    type_id: &'static str,
) -> Option<T> {
    match &fact.content {
        FactContent::Relational(RelationalFact::Generic {
            context_id: fact_context,
            envelope,
        }) if fact_context == context_id && envelope.type_id.as_str() == type_id => {
            T::from_envelope(envelope)
        }
        _ => None,
    }
}

fn moderation_scope_key(
    authority: AuthorityId,
    channel_id: Option<ChannelId>,
) -> ModerationScopeKey {
    (authority, channel_id)
}

fn remove_if_newer<T>(
    active: &mut HashMap<ModerationScopeKey, T>,
    authority: AuthorityId,
    channel_id: Option<ChannelId>,
    reversal_at_ms: u64,
    active_at_ms: impl Fn(&T) -> u64,
) {
    let key = moderation_scope_key(authority, channel_id);
    if let Some(status) = active.get(&key) {
        if reversal_at_ms >= active_at_ms(status) {
            active.remove(&key);
        }
    }
}

fn remove_orphaned_channel_statuses<T>(
    statuses: &mut HashMap<ModerationScopeKey, T>,
    live_channels: &HashSet<ChannelId>,
    channel_id_of: impl Fn(&T) -> Option<ChannelId>,
) {
    statuses.retain(|_, status| {
        channel_id_of(status)
            .map(|channel_id| live_channels.contains(&channel_id))
            .unwrap_or(true)
    });
}

fn status_applies_to_channel<T>(
    statuses: &HashMap<ModerationScopeKey, T>,
    authority: &AuthorityId,
    channel_id: Option<&ChannelId>,
) -> bool {
    statuses.contains_key(&moderation_scope_key(*authority, None))
        || channel_id.is_some_and(|channel| {
            statuses.contains_key(&moderation_scope_key(*authority, Some(*channel)))
        })
}

/// Query current bans in a context
///
/// Processes HomeBan and HomeUnban facts in order to derive the current set
/// of banned users. Unbans remove bans if they happened after the ban.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (home) to query
/// * `current_time_ms` - Current time for expiration checking (ms since epoch)
///
/// # Returns
/// HashMap mapping `(AuthorityId, Option<ChannelId>)` to BanStatus for all
/// currently banned scopes
pub fn query_current_bans(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
) -> HashMap<ModerationScopeKey, BanStatus> {
    let mut bans: HashMap<ModerationScopeKey, BanStatus> = HashMap::new();

    for fact in facts {
        if let Some(home_ban) =
            moderation_fact::<HomeBanFact>(fact, context_id, HOME_BAN_FACT_TYPE_ID)
        {
            let ban = BanStatus::from_fact(&home_ban);
            bans.insert(
                moderation_scope_key(ban.banned_authority, ban.channel_id),
                ban,
            );
            continue;
        }

        if let Some(home_unban) =
            moderation_fact::<HomeUnbanFact>(fact, context_id, HOME_UNBAN_FACT_TYPE_ID)
        {
            remove_if_newer(
                &mut bans,
                home_unban.unbanned_authority,
                home_unban.channel_id,
                home_unban.unbanned_at_ms(),
                |ban| ban.banned_at_ms,
            );
        }
    }

    bans.retain(|_, ban| !ban.is_expired(current_time_ms));

    bans
}

/// Query current bans in a context, dropping channel-scoped bans whose
/// referenced channels no longer exist.
pub fn query_current_bans_in_live_channels(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
    live_channels: &HashSet<ChannelId>,
) -> HashMap<ModerationScopeKey, BanStatus> {
    let mut bans = query_current_bans(facts, context_id, current_time_ms);
    remove_orphaned_channel_statuses(&mut bans, live_channels, |ban| ban.channel_id);
    bans
}

/// Query current mutes in a context
///
/// Processes HomeMute and HomeUnmute facts in order to derive the current set
/// of muted users. Unmutes remove mutes if they happened after the mute.
/// Also filters out expired mutes based on current time.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (home) to query
/// * `current_time_ms` - Current time for expiration checking (ms since epoch)
///
/// # Returns
/// HashMap mapping `(AuthorityId, Option<ChannelId>)` to MuteStatus for all
/// currently muted scopes
pub fn query_current_mutes(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
) -> HashMap<ModerationScopeKey, MuteStatus> {
    let mut mutes: HashMap<ModerationScopeKey, MuteStatus> = HashMap::new();

    for fact in facts {
        if let Some(home_mute) =
            moderation_fact::<HomeMuteFact>(fact, context_id, HOME_MUTE_FACT_TYPE_ID)
        {
            let mute = MuteStatus::from_fact(&home_mute);
            mutes.insert(
                moderation_scope_key(mute.muted_authority, mute.channel_id),
                mute,
            );
            continue;
        }

        if let Some(home_unmute) =
            moderation_fact::<HomeUnmuteFact>(fact, context_id, HOME_UNMUTE_FACT_TYPE_ID)
        {
            remove_if_newer(
                &mut mutes,
                home_unmute.unmuted_authority,
                home_unmute.channel_id,
                home_unmute.unmuted_at_ms(),
                |mute| mute.muted_at_ms,
            );
        }
    }

    mutes.retain(|_, mute| !mute.is_expired(current_time_ms));

    mutes
}

/// Query current mutes in a context, dropping channel-scoped mutes whose
/// referenced channels no longer exist.
pub fn query_current_mutes_in_live_channels(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
    live_channels: &HashSet<ChannelId>,
) -> HashMap<ModerationScopeKey, MuteStatus> {
    let mut mutes = query_current_mutes(facts, context_id, current_time_ms);
    remove_orphaned_channel_statuses(&mut mutes, live_channels, |mute| mute.channel_id);
    mutes
}

/// Query kick history (audit log) for a context
///
/// Returns all HomeKick facts in chronological order. Kicks are immutable
/// audit log entries and are never removed.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (home) to query
///
/// # Returns
/// Vector of KickRecord in chronological order
pub fn query_kick_history(facts: &[Fact], context_id: &ContextId) -> Vec<KickRecord> {
    let mut kicks = Vec::new();

    for fact in facts {
        if let Some(home_kick) =
            moderation_fact::<HomeKickFact>(fact, context_id, HOME_KICK_FACT_TYPE_ID)
        {
            kicks.push(KickRecord::from_fact(&home_kick));
        }
    }

    kicks
}

/// Check if a user is currently banned in a context
///
/// Convenience function that queries current bans and checks if the given
/// authority is in the banned set.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (home) to check
/// * `authority` - Authority to check for ban status
/// * `current_time_ms` - Current time for expiration checking
/// * `channel_id` - Optional channel to check (None = check home-wide ban)
///
/// # Returns
/// true if the user is currently banned, false otherwise
pub fn is_user_banned(
    facts: &[Fact],
    context_id: &ContextId,
    authority: &AuthorityId,
    current_time_ms: u64,
    channel_id: Option<&ChannelId>,
) -> bool {
    let bans = query_current_bans(facts, context_id, current_time_ms);
    status_applies_to_channel(&bans, authority, channel_id)
}

/// Check if a user is currently muted in a context
///
/// Convenience function that queries current mutes and checks if the given
/// authority is in the muted set.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (home) to check
/// * `authority` - Authority to check for mute status
/// * `current_time_ms` - Current time for expiration checking
/// * `channel_id` - Optional channel to check (None = check home-wide mute)
///
/// # Returns
/// true if the user is currently muted, false otherwise
pub fn is_user_muted(
    facts: &[Fact],
    context_id: &ContextId,
    authority: &AuthorityId,
    current_time_ms: u64,
    channel_id: Option<&ChannelId>,
) -> bool {
    let mutes = query_current_mutes(facts, context_id, current_time_ms);
    status_applies_to_channel(&mutes, authority, channel_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};

    fn create_test_fact(content: RelationalFact, order_index: u64) -> Fact {
        Fact::new(
            OrderTime([order_index as u8; 32]),
            TimeStamp::OrderClock(OrderTime([order_index as u8; 32])),
            FactContent::Relational(content),
        )
    }

    /// Create a test context ID
    fn test_context() -> ContextId {
        ContextId::new_from_entropy([2u8; 32])
    }

    /// Create a test authority ID with a unique identifier
    fn test_authority(id: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([id; 32])
    }

    /// Create a test channel ID with a unique identifier
    fn test_channel(id: u8) -> ChannelId {
        ChannelId::from_bytes([id; 32])
    }

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_query_current_bans_basic() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "test ban".to_string(),
            banned_at: pt(1000),
            expires_at: None,
        };

        let facts = vec![create_test_fact(ban_fact.to_generic(), 0)];

        let bans = query_current_bans(&facts, &context, 2000);
        assert_eq!(bans.len(), 1);
        assert!(bans.contains_key(&(user1, None)));
        assert_eq!(bans[&(user1, None)].reason, "test ban");
    }

    #[test]
    fn test_query_current_bans_with_unban() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "test ban".to_string(),
            banned_at: pt(1000),
            expires_at: None,
        };
        let unban_fact = HomeUnbanFact {
            context_id: context.clone(),
            channel_id: None,
            unbanned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            unbanned_at: pt(2000),
        };

        let facts = vec![
            create_test_fact(ban_fact.to_generic(), 0),
            create_test_fact(unban_fact.to_generic(), 1),
        ];

        let bans = query_current_bans(&facts, &context, 3000);
        assert_eq!(bans.len(), 0, "User should be unbanned");
    }

    #[test]
    fn test_query_current_bans_expired() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "test ban".to_string(),
            banned_at: pt(1000),
            expires_at: Some(pt(2000)), // Expires at 2000ms
        };

        let facts = vec![create_test_fact(ban_fact.to_generic(), 0)];

        // Query before expiration
        let bans = query_current_bans(&facts, &context, 1500);
        assert_eq!(bans.len(), 1, "Ban should be active before expiration");

        // Query after expiration
        let bans = query_current_bans(&facts, &context, 2500);
        assert_eq!(bans.len(), 0, "Ban should be expired");
    }

    #[test]
    fn test_query_current_mutes_with_expiration() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let mute_fact = HomeMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: moderator.clone(),
            duration_secs: Some(60),
            muted_at: pt(1000),
            expires_at: Some(pt(61000)), // 1000ms + 60s = 61000ms
        };

        let facts = vec![create_test_fact(mute_fact.to_generic(), 0)];

        // Query before expiration
        let mutes = query_current_mutes(&facts, &context, 30000);
        assert_eq!(mutes.len(), 1, "Mute should be active before expiration");

        // Query after expiration
        let mutes = query_current_mutes(&facts, &context, 70000);
        assert_eq!(mutes.len(), 0, "Mute should be expired");
    }

    #[test]
    fn test_query_current_mutes_with_unmute() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let mute_fact = HomeMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: moderator.clone(),
            duration_secs: None,
            muted_at: pt(1000),
            expires_at: None,
        };
        let unmute_fact = HomeUnmuteFact {
            context_id: context.clone(),
            channel_id: None,
            unmuted_authority: user1.clone(),
            actor_authority: moderator.clone(),
            unmuted_at: pt(2000),
        };

        let facts = vec![
            create_test_fact(mute_fact.to_generic(), 0),
            create_test_fact(unmute_fact.to_generic(), 1),
        ];

        let mutes = query_current_mutes(&facts, &context, 3000);
        assert_eq!(mutes.len(), 0, "User should be unmuted");
    }

    #[test]
    fn test_query_kick_history() {
        let context = test_context();
        let user1 = test_authority(1);
        let user2 = test_authority(3);
        let moderator = test_authority(2);
        let channel = test_channel(1);

        let kick_fact1 = HomeKickFact {
            context_id: context.clone(),
            channel_id: channel.clone(),
            kicked_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "first kick".to_string(),
            kicked_at: pt(1000),
        };
        let kick_fact2 = HomeKickFact {
            context_id: context.clone(),
            channel_id: channel.clone(),
            kicked_authority: user2.clone(),
            actor_authority: moderator.clone(),
            reason: "second kick".to_string(),
            kicked_at: pt(2000),
        };

        let facts = vec![
            create_test_fact(kick_fact1.to_generic(), 0),
            create_test_fact(kick_fact2.to_generic(), 1),
        ];

        let kicks = query_kick_history(&facts, &context);
        assert_eq!(kicks.len(), 2);
        assert_eq!(kicks[0].kicked_authority, user1);
        assert_eq!(kicks[1].kicked_authority, user2);
        assert_eq!(kicks[0].reason, "first kick");
        assert_eq!(kicks[1].reason, "second kick");
    }

    #[test]
    fn test_is_user_banned() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "test ban".to_string(),
            banned_at: pt(1000),
            expires_at: None,
        };

        let facts = vec![create_test_fact(ban_fact.to_generic(), 0)];

        assert!(is_user_banned(&facts, &context, &user1, 2000, None));

        let user2 = test_authority(3);
        assert!(!is_user_banned(&facts, &context, &user2, 2000, None));
    }

    #[test]
    fn test_is_user_muted() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let mute_fact = HomeMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: moderator.clone(),
            duration_secs: None,
            muted_at: pt(1000),
            expires_at: None,
        };

        let facts = vec![create_test_fact(mute_fact.to_generic(), 0)];

        assert!(is_user_muted(&facts, &context, &user1, 2000, None));

        let user2 = test_authority(3);
        assert!(!is_user_muted(&facts, &context, &user2, 2000, None));
    }

    #[test]
    fn test_channel_specific_ban() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);
        let channel1 = test_channel(1);
        let channel2 = test_channel(2);

        let ban_fact = HomeBanFact {
            context_id: context.clone(),
            channel_id: Some(channel1.clone()),
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "channel-specific ban".to_string(),
            banned_at: pt(1000),
            expires_at: None,
        };

        let facts = vec![create_test_fact(ban_fact.to_generic(), 0)];

        // Should be banned in channel1
        assert!(is_user_banned(
            &facts,
            &context,
            &user1,
            2000,
            Some(&channel1)
        ));

        // Should not be banned in channel2
        assert!(!is_user_banned(
            &facts,
            &context,
            &user1,
            2000,
            Some(&channel2)
        ));
    }

    #[test]
    fn test_multiple_channel_specific_bans_for_same_user_coexist() {
        let context = test_context();
        let user = test_authority(1);
        let moderator = test_authority(2);
        let channel1 = test_channel(1);
        let channel2 = test_channel(2);

        let facts = vec![
            create_test_fact(
                HomeBanFact {
                    context_id: context,
                    channel_id: Some(channel1),
                    banned_authority: user,
                    actor_authority: moderator,
                    reason: "ban one".to_string(),
                    banned_at: pt(1000),
                    expires_at: None,
                }
                .to_generic(),
                0,
            ),
            create_test_fact(
                HomeBanFact {
                    context_id: context,
                    channel_id: Some(channel2),
                    banned_authority: user,
                    actor_authority: moderator,
                    reason: "ban two".to_string(),
                    banned_at: pt(2000),
                    expires_at: None,
                }
                .to_generic(),
                1,
            ),
        ];

        let bans = query_current_bans(&facts, &context, 3000);
        assert_eq!(bans.len(), 2);
        assert_eq!(bans[&(user, Some(channel1))].reason, "ban one");
        assert_eq!(bans[&(user, Some(channel2))].reason, "ban two");
        assert!(is_user_banned(
            &facts,
            &context,
            &user,
            3000,
            Some(&channel1)
        ));
        assert!(is_user_banned(
            &facts,
            &context,
            &user,
            3000,
            Some(&channel2)
        ));
    }

    #[test]
    fn test_query_current_bans_in_live_channels_drops_orphans() {
        let context = test_context();
        let user = test_authority(1);
        let moderator = test_authority(2);
        let live_channel = test_channel(1);
        let deleted_channel = test_channel(2);
        let live_channels = HashSet::from([live_channel]);

        let facts = vec![
            create_test_fact(
                HomeBanFact {
                    context_id: context,
                    channel_id: Some(live_channel),
                    banned_authority: user,
                    actor_authority: moderator,
                    reason: "live".to_string(),
                    banned_at: pt(1000),
                    expires_at: None,
                }
                .to_generic(),
                0,
            ),
            create_test_fact(
                HomeBanFact {
                    context_id: context,
                    channel_id: Some(deleted_channel),
                    banned_authority: user,
                    actor_authority: moderator,
                    reason: "orphan".to_string(),
                    banned_at: pt(2000),
                    expires_at: None,
                }
                .to_generic(),
                1,
            ),
        ];

        let bans = query_current_bans_in_live_channels(&facts, &context, 3000, &live_channels);
        assert_eq!(bans.len(), 1);
        assert!(bans.contains_key(&(user, Some(live_channel))));
        assert!(!bans.contains_key(&(user, Some(deleted_channel))));
    }
}

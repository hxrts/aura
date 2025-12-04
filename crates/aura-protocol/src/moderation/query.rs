#![allow(clippy::clone_on_copy)]

//! Query functions for deriving moderation state from journal facts

use super::facts::{
    BlockBanFact, BlockKickFact, BlockMuteFact, BlockUnbanFact, BlockUnmuteFact,
    BLOCK_BAN_FACT_TYPE_ID, BLOCK_KICK_FACT_TYPE_ID, BLOCK_UNBAN_FACT_TYPE_ID,
};
use super::types::{BanStatus, KickRecord, MuteStatus};
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use std::collections::HashMap;

/// Query current bans in a context
///
/// Processes BlockBan and BlockUnban facts in order to derive the current set
/// of banned users. Unbans remove bans if they happened after the ban.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (block) to query
/// * `current_time_ms` - Current time for expiration checking (ms since epoch)
///
/// # Returns
/// HashMap mapping AuthorityId to BanStatus for all currently banned users
pub fn query_current_bans(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
) -> HashMap<AuthorityId, BanStatus> {
    let mut bans: HashMap<AuthorityId, BanStatus> = HashMap::new();

    for fact in facts {
        match &fact.content {
            FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                binding_type,
                binding_data,
            }) if fact_context == context_id && binding_type == BLOCK_BAN_FACT_TYPE_ID => {
                if let Some(block_ban) = BlockBanFact::from_bytes(binding_data) {
                    let ban = BanStatus {
                        banned_authority: block_ban.banned_authority,
                        actor_authority: block_ban.actor_authority,
                        reason: block_ban.reason,
                        banned_at_ms: block_ban.banned_at_ms,
                        expires_at_ms: block_ban.expires_at_ms,
                        channel_id: block_ban.channel_id,
                    };
                    bans.insert(block_ban.banned_authority, ban);
                }
            }
            FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                binding_type,
                binding_data,
            }) if fact_context == context_id && binding_type == BLOCK_UNBAN_FACT_TYPE_ID => {
                if let Some(block_unban) = BlockUnbanFact::from_bytes(binding_data) {
                    // Remove ban if unban happened after the ban
                    if let Some(ban) = bans.get(&block_unban.unbanned_authority) {
                        if block_unban.unbanned_at_ms >= ban.banned_at_ms {
                            bans.remove(&block_unban.unbanned_authority);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Filter out expired bans
    bans.retain(|_, ban| !ban.is_expired(current_time_ms));

    bans
}

/// Query current mutes in a context
///
/// Processes BlockMute and BlockUnmute facts in order to derive the current set
/// of muted users. Unmutes remove mutes if they happened after the mute.
/// Also filters out expired mutes based on current time.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (block) to query
/// * `current_time_ms` - Current time for expiration checking (ms since epoch)
///
/// # Returns
/// HashMap mapping AuthorityId to MuteStatus for all currently muted users
pub fn query_current_mutes(
    facts: &[Fact],
    context_id: &ContextId,
    current_time_ms: u64,
) -> HashMap<AuthorityId, MuteStatus> {
    let mut mutes: HashMap<AuthorityId, MuteStatus> = HashMap::new();

    for fact in facts {
        match &fact.content {
            FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                binding_type,
                binding_data,
            }) if fact_context == context_id && binding_type == "moderation:block-mute" => {
                if let Some(block_mute) = BlockMuteFact::from_bytes(binding_data) {
                    let mute = MuteStatus {
                        muted_authority: block_mute.muted_authority,
                        actor_authority: block_mute.actor_authority,
                        duration_secs: block_mute.duration_secs,
                        muted_at_ms: block_mute.muted_at_ms,
                        expires_at_ms: block_mute.expires_at_ms,
                        channel_id: block_mute.channel_id,
                    };
                    mutes.insert(mute.muted_authority, mute);
                }
            }
            FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                binding_type,
                binding_data,
            }) if fact_context == context_id && binding_type == "moderation:block-unmute" => {
                if let Some(block_unmute) = BlockUnmuteFact::from_bytes(binding_data) {
                    let unmuted_authority = block_unmute.unmuted_authority;
                    let unmuted_at_ms = block_unmute.unmuted_at_ms;
                    // Remove mute if unmute happened after the mute
                    if let Some(mute) = mutes.get(&unmuted_authority) {
                        if unmuted_at_ms >= mute.muted_at_ms {
                            mutes.remove(&unmuted_authority);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Filter out expired mutes
    mutes.retain(|_, mute| !mute.is_expired(current_time_ms));

    mutes
}

/// Query kick history (audit log) for a context
///
/// Returns all BlockKick facts in chronological order. Kicks are immutable
/// audit log entries and are never removed.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (block) to query
///
/// # Returns
/// Vector of KickRecord in chronological order
pub fn query_kick_history(facts: &[Fact], context_id: &ContextId) -> Vec<KickRecord> {
    let mut kicks = Vec::new();

    for fact in facts {
        if let FactContent::Relational(RelationalFact::Generic {
            context_id: fact_context,
            binding_type,
            binding_data,
        }) = &fact.content
        {
            if fact_context == context_id && binding_type == BLOCK_KICK_FACT_TYPE_ID {
                if let Some(block_kick) = BlockKickFact::from_bytes(binding_data) {
                    kicks.push(KickRecord {
                        kicked_authority: block_kick.kicked_authority,
                        actor_authority: block_kick.actor_authority,
                        channel_id: block_kick.channel_id,
                        reason: block_kick.reason,
                        kicked_at_ms: block_kick.kicked_at_ms,
                    });
                }
            }
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
/// * `context_id` - Context (block) to check
/// * `authority` - Authority to check for ban status
/// * `current_time_ms` - Current time for expiration checking
/// * `channel_id` - Optional channel to check (None = check block-wide ban)
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

    if let Some(ban) = bans.get(authority) {
        if let Some(channel) = channel_id {
            ban.applies_to_channel(channel)
        } else {
            true // If no channel specified, check block-wide ban
        }
    } else {
        false
    }
}

/// Check if a user is currently muted in a context
///
/// Convenience function that queries current mutes and checks if the given
/// authority is in the muted set.
///
/// # Arguments
/// * `facts` - Ordered list of facts from the journal
/// * `context_id` - Context (block) to check
/// * `authority` - Authority to check for mute status
/// * `current_time_ms` - Current time for expiration checking
/// * `channel_id` - Optional channel to check (None = check block-wide mute)
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

    if let Some(mute) = mutes.get(authority) {
        if let Some(channel) = channel_id {
            mute.applies_to_channel(channel)
        } else {
            true // If no channel specified, check block-wide mute
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_core::time::{OrderTime, TimeStamp};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};

    fn create_test_fact(content: RelationalFact, order_index: u64) -> Fact {
        Fact {
            order: OrderTime([order_index as u8; 32]),
            timestamp: TimeStamp::OrderClock(OrderTime([order_index as u8; 32])),
            content: FactContent::Relational(content),
        }
    }

    /// Create a test context ID
    fn test_context() -> ContextId {
        ContextId::default()
    }

    /// Create a test authority ID with a unique identifier
    fn test_authority(id: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([id; 32])
    }

    /// Create a test channel ID with a unique identifier
    fn test_channel(id: u8) -> ChannelId {
        ChannelId::from_bytes([id; 32])
    }

    #[test]
    fn test_query_current_bans_basic() {
        let context = test_context();
        let user1 = test_authority(1);
        let steward = test_authority(2);

        let ban_fact = BlockBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "test ban".to_string(),
            banned_at_ms: 1000,
            expires_at_ms: None,
        };

        let facts = vec![create_test_fact(ban_fact.to_generic(), 0)];

        let bans = query_current_bans(&facts, &context, 2000);
        assert_eq!(bans.len(), 1);
        assert!(bans.contains_key(&user1));
        assert_eq!(bans[&user1].reason, "test ban");
    }

    #[test]
    fn test_query_current_bans_with_unban() {
        let context = test_context();
        let user1 = test_authority(1);
        let steward = test_authority(2);

        let ban_fact = BlockBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "test ban".to_string(),
            banned_at_ms: 1000,
            expires_at_ms: None,
        };
        let unban_fact = BlockUnbanFact {
            context_id: context.clone(),
            channel_id: None,
            unbanned_authority: user1.clone(),
            actor_authority: steward.clone(),
            unbanned_at_ms: 2000,
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
        let steward = test_authority(2);

        let ban_fact = BlockBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "test ban".to_string(),
            banned_at_ms: 1000,
            expires_at_ms: Some(2000), // Expires at 2000ms
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
        let steward = test_authority(2);

        let mute_fact = BlockMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: steward.clone(),
            duration_secs: Some(60),
            muted_at_ms: 1000,
            expires_at_ms: Some(61000), // 1000ms + 60s = 61000ms
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
        let steward = test_authority(2);

        let mute_fact = BlockMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: steward.clone(),
            duration_secs: None,
            muted_at_ms: 1000,
            expires_at_ms: None,
        };
        let unmute_fact = BlockUnmuteFact {
            context_id: context.clone(),
            channel_id: None,
            unmuted_authority: user1.clone(),
            actor_authority: steward.clone(),
            unmuted_at_ms: 2000,
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
        let steward = test_authority(2);
        let channel = test_channel(1);

        let kick_fact1 = BlockKickFact {
            context_id: context.clone(),
            channel_id: channel.clone(),
            kicked_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "first kick".to_string(),
            kicked_at_ms: 1000,
        };
        let kick_fact2 = BlockKickFact {
            context_id: context.clone(),
            channel_id: channel.clone(),
            kicked_authority: user2.clone(),
            actor_authority: steward.clone(),
            reason: "second kick".to_string(),
            kicked_at_ms: 2000,
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
        let steward = test_authority(2);

        let ban_fact = BlockBanFact {
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "test ban".to_string(),
            banned_at_ms: 1000,
            expires_at_ms: None,
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
        let steward = test_authority(2);

        let mute_fact = BlockMuteFact {
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: steward.clone(),
            duration_secs: None,
            muted_at_ms: 1000,
            expires_at_ms: None,
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
        let steward = test_authority(2);
        let channel1 = test_channel(1);
        let channel2 = test_channel(2);

        let ban_fact = BlockBanFact {
            context_id: context.clone(),
            channel_id: Some(channel1.clone()),
            banned_authority: user1.clone(),
            actor_authority: steward.clone(),
            reason: "channel-specific ban".to_string(),
            banned_at_ms: 1000,
            expires_at_ms: None,
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
}

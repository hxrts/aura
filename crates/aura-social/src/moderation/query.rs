#![allow(clippy::clone_on_copy)]

//! Query functions for deriving moderation state from journal facts.
//!
//! Bans and mutes are tagged observed-remove sets and kicks are ordered
//! causally (`super::governance`), so every result is a function of the fact
//! set, independent of journal or arrival order.

use super::facts::{
    HOME_BAN_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_UNBAN_FACT_TYPE_ID,
    HOME_UNMUTE_FACT_TYPE_ID,
};
use super::governance::{
    live_ban_tags, live_mute_tags, sort_causally, HomeGovernanceEvent, TaggedHomeGovernanceEvent,
};
use super::types::{BanStatus, KickRecord, ModerationScopeKey, MuteStatus};
use aura_core::time::CausalTag;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Governance facts of `context_id`, skipping undecodable ones (lenient
/// queries; required callers use `try_is_user_banned_and_muted`).
fn lenient_governance_events(
    facts: &[Fact],
    context_id: &ContextId,
) -> Vec<TaggedHomeGovernanceEvent> {
    facts
        .iter()
        .filter_map(|fact| match &fact.content {
            FactContent::Relational(RelationalFact::Generic {
                context_id: fact_context,
                envelope,
            }) if fact_context == context_id => {
                TaggedHomeGovernanceEvent::try_decode(*fact_context, envelope)
                    .ok()
                    .flatten()
            }
            _ => None,
        })
        .collect()
}

fn moderation_scope_key(
    authority: AuthorityId,
    channel_id: Option<ChannelId>,
) -> ModerationScopeKey {
    (authority, channel_id)
}

/// Live statuses per scope; when several live adds share a scope, the latest
/// in causal order represents it.
fn live_statuses<T>(
    events: &[TaggedHomeGovernanceEvent],
    live: &BTreeSet<CausalTag>,
    status: impl Fn(&HomeGovernanceEvent) -> Option<T>,
    key: impl Fn(&T) -> ModerationScopeKey,
) -> HashMap<ModerationScopeKey, T> {
    let mut ordered: Vec<_> = events
        .iter()
        .filter(|event| live.contains(&event.tag))
        .collect();
    sort_causally(&mut ordered);
    let mut statuses = HashMap::new();
    for event in ordered {
        if let Some(status) = status(&event.event) {
            statuses.insert(key(&status), status);
        }
    }
    statuses
}

fn ban_status(event: &HomeGovernanceEvent) -> Option<BanStatus> {
    match event {
        HomeGovernanceEvent::Ban(fact) => Some(BanStatus::from_fact(fact)),
        _ => None,
    }
}

fn mute_status(event: &HomeGovernanceEvent) -> Option<MuteStatus> {
    match event {
        HomeGovernanceEvent::Mute(fact) => Some(MuteStatus::from_fact(fact)),
        _ => None,
    }
}

fn bans_of(events: &[TaggedHomeGovernanceEvent]) -> HashMap<ModerationScopeKey, BanStatus> {
    let refs: Vec<_> = events.iter().collect();
    live_statuses(events, &live_ban_tags(&refs), ban_status, |ban| {
        moderation_scope_key(ban.banned_authority, ban.channel_id)
    })
}

fn mutes_of(events: &[TaggedHomeGovernanceEvent]) -> HashMap<ModerationScopeKey, MuteStatus> {
    let refs: Vec<_> = events.iter().collect();
    live_statuses(events, &live_mute_tags(&refs), mute_status, |mute| {
        moderation_scope_key(mute.muted_authority, mute.channel_id)
    })
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
/// Bans form a tagged observed-remove set: an unban removes the bans its
/// writer observed (legacy unbans: bans at or before its time). The result
/// does not depend on the order of `facts`.
///
/// # Arguments
/// * `facts` - Facts from the journal, in any order
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
    let mut bans = bans_of(&lenient_governance_events(facts, context_id));
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
/// Mutes form a tagged observed-remove set like bans; expired mutes are
/// filtered by `current_time_ms`. The result does not depend on the order of
/// `facts`.
///
/// # Arguments
/// * `facts` - Facts from the journal, in any order
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
    let mut mutes = mutes_of(&lenient_governance_events(facts, context_id));
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
/// Returns all HomeKick facts in causal order (legacy kicks first, by their
/// recorded time). Kicks are immutable audit log entries and are never removed.
///
/// # Arguments
/// * `facts` - Facts from the journal, in any order
/// * `context_id` - Context (home) to query
///
/// # Returns
/// Vector of KickRecord in causal order
pub fn query_kick_history(facts: &[Fact], context_id: &ContextId) -> Vec<KickRecord> {
    let events = lenient_governance_events(facts, context_id);
    let mut kicks: Vec<_> = events
        .iter()
        .filter(|event| matches!(event.event, HomeGovernanceEvent::Kick(_)))
        .collect();
    sort_causally(&mut kicks);
    kicks
        .into_iter()
        .filter_map(|event| match &event.event {
            HomeGovernanceEvent::Kick(kick) => Some(KickRecord::from_fact(kick)),
            _ => None,
        })
        .collect()
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

/// Required moderation evidence could not be decoded or bounded.
#[derive(Debug, thiserror::Error)]
pub enum RequiredModerationQueryError {
    /// Required fact envelope schema or payload failed.
    #[error("Moderation fact envelope failed: {0}")]
    Envelope(#[from] aura_core::types::facts::FactError),
    /// Canonical binary decoding failed.
    #[error("Moderation binary payload failed: {0}")]
    DagCbor(#[source] aura_core::util::serialization::SerializationError),
    /// Declared JSON decoding failed.
    #[error("Moderation JSON payload failed: {0}")]
    Json(#[source] serde_json::Error),
    /// The payload was stored beneath a different context.
    #[error("Moderation context mismatch: outer {outer}, payload {payload}")]
    ContextMismatch {
        /// Committed wrapper context.
        outer: ContextId,
        /// Decoded domain payload context.
        payload: ContextId,
    },
    /// The journal query exceeds a required bound.
    #[error("Moderation query exceeds {kind:?}: {actual} > {maximum}")]
    Bound {
        /// Exhausted query dimension.
        kind: ModerationQueryBound,
        /// Observed quantity.
        actual: usize,
        /// Required maximum quantity.
        maximum: usize,
    },
}

/// Exhaustive bounded query dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModerationQueryBound {
    /// Journal record inventory.
    Records,
    /// Aggregate relevant domain payload bytes.
    PayloadBytes,
}

impl From<RequiredModerationQueryError> for aura_core::AuraError {
    fn from(error: RequiredModerationQueryError) -> Self {
        let message = error.to_string();
        match error {
            codec @ (RequiredModerationQueryError::DagCbor(_)
            | RequiredModerationQueryError::Json(_)) => Self::Serialization {
                message,
                source: Some(std::sync::Arc::new(codec)),
            },
            validation @ (RequiredModerationQueryError::Envelope(_)
            | RequiredModerationQueryError::ContextMismatch { .. }
            | RequiredModerationQueryError::Bound { .. }) => Self::Invalid {
                message,
                source: Some(std::sync::Arc::new(validation)),
            },
        }
    }
}

pub(crate) fn decode_required_moderation_fact<T: DomainFact + serde::de::DeserializeOwned>(
    envelope: &aura_core::types::facts::FactEnvelope,
    outer: ContextId,
    expected_type: &str,
    schema_version: u16,
) -> Result<T, RequiredModerationQueryError> {
    use aura_core::types::facts::{
        FactEncoding, FactError, FactSchemaCompatibility, MAX_FACT_PAYLOAD_BYTES,
    };
    if envelope.type_id.as_str() != expected_type {
        return Err(FactError::TypeMismatch {
            expected: expected_type.to_owned(),
            actual: envelope.type_id.to_string(),
        }
        .into());
    }
    FactSchemaCompatibility::range(schema_version, schema_version)
        .ensure_supported(envelope.schema_version)?;
    if envelope.payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return Err(FactError::PayloadTooLarge {
            size: envelope.payload.len() as u64,
            max: MAX_FACT_PAYLOAD_BYTES as u64,
        }
        .into());
    }
    let fact: T = match envelope.encoding {
        FactEncoding::DagCbor => aura_core::util::serialization::from_slice(&envelope.payload)
            .map_err(RequiredModerationQueryError::DagCbor)?,
        FactEncoding::Json => {
            serde_json::from_slice(&envelope.payload).map_err(RequiredModerationQueryError::Json)?
        }
    };
    if fact.context_id() != outer {
        return Err(RequiredModerationQueryError::ContextMismatch {
            outer,
            payload: fact.context_id(),
        });
    }
    Ok(fact)
}
/// Derive required ban/mute decisions after validating all relevant moderation
/// evidence, including reversals, without converting corrupt facts into absence.
/// This validates decoding and scoping; journal commit/authentication provenance
/// remains owned by the runtime caller that supplies the facts. The decision
/// is a function of the fact set (observed-remove semantics), not its order.
///
/// # Errors
/// Fails on bounds, unsupported schema, declared-codec failure, or context mismatch.
pub fn try_is_user_banned_and_muted(
    facts: &[Fact],
    context: &ContextId,
    authority: &AuthorityId,
    time_ms: u64,
    channel: Option<&ChannelId>,
) -> Result<(bool, bool), RequiredModerationQueryError> {
    const MAX_RECORDS: usize = 65_536;
    const MAX_BYTES: usize = 16 * 1024 * 1024;
    if facts.len() > MAX_RECORDS {
        return Err(RequiredModerationQueryError::Bound {
            kind: ModerationQueryBound::Records,
            actual: facts.len(),
            maximum: MAX_RECORDS,
        });
    }
    let mut bytes = 0usize;
    let mut events = Vec::new();
    for fact in facts {
        let FactContent::Relational(RelationalFact::Generic {
            context_id,
            envelope,
        }) = &fact.content
        else {
            continue;
        };
        if !matches!(
            envelope.type_id.as_str(),
            HOME_BAN_FACT_TYPE_ID
                | HOME_UNBAN_FACT_TYPE_ID
                | HOME_MUTE_FACT_TYPE_ID
                | HOME_UNMUTE_FACT_TYPE_ID
        ) {
            continue;
        }
        bytes = bytes.checked_add(envelope.payload.len()).ok_or(
            RequiredModerationQueryError::Bound {
                kind: ModerationQueryBound::PayloadBytes,
                actual: usize::MAX,
                maximum: MAX_BYTES,
            },
        )?;
        if bytes > MAX_BYTES {
            return Err(RequiredModerationQueryError::Bound {
                kind: ModerationQueryBound::PayloadBytes,
                actual: bytes,
                maximum: MAX_BYTES,
            });
        }
        let decoded = TaggedHomeGovernanceEvent::try_decode(*context_id, envelope)?;
        if context_id == context {
            events.extend(decoded);
        }
    }
    let mut bans = bans_of(&events);
    let mut mutes = mutes_of(&events);
    bans.retain(|_, status| !status.is_expired(time_ms));
    mutes.retain(|_, status| !status.is_expired(time_ms));
    Ok((
        status_applies_to_channel(&bans, authority, channel),
        status_applies_to_channel(&mutes, authority, channel),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::moderation::facts::{
        HomeBanFact, HomeKickFact, HomeMuteFact, HomeUnbanFact, HomeUnmuteFact,
        HOME_KICK_FACT_TYPE_ID,
    };
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};

    /// Causal metadata of the `n`th write by one writer: writes are causally
    /// ordered by `n`.
    fn c(n: u8) -> aura_core::time::CausalMetadata {
        aura_core::time::CausalMetadata {
            revokes: Vec::new(),
            supersedes: Vec::new(),
            clock: aura_core::time::CausalClock {
                lamport: u64::from(n),
                vector: vec![(
                    aura_core::types::identifiers::DeviceId(uuid::Uuid::from_bytes([1; 16])),
                    u64::from(n),
                )],
            },
        }
    }

    fn revoking(n: u8, revoked: HomeGovernanceEvent) -> aura_core::time::CausalMetadata {
        let revoked = TaggedHomeGovernanceEvent::from_event(revoked).expect("governance fact");
        aura_core::time::CausalMetadata {
            revokes: vec![revoked.tag],
            ..c(n)
        }
    }

    fn create_test_fact(content: RelationalFact, order_index: u64) -> Fact {
        Fact::new(
            OrderTime([order_index as u8; 32]),
            TimeStamp::OrderClock(OrderTime([order_index as u8; 32])),
            FactContent::Relational(content),
        )
    }

    #[test]
    fn required_moderation_rejects_corrupt_ban_and_reversal_without_absence_repair() {
        use aura_core::types::facts::FactEncoding;
        use std::error::Error;
        let context = ContextId::new_from_entropy([231; 32]);
        let subject = AuthorityId::new_from_entropy([232; 32]);
        let ban = HomeBanFact {
            causal: c(1),
            context_id: context,
            channel_id: None,
            banned_authority: subject,
            actor_authority: AuthorityId::new_from_entropy([233; 32]),
            reason: "test".into(),
            banned_at: PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            expires_at: None,
        };
        let original = ban.to_envelope();
        let wrap = |envelope| {
            create_test_fact(
                RelationalFact::Generic {
                    context_id: context,
                    envelope,
                },
                0,
            )
        };
        let valid = wrap(original.clone());
        assert_eq!(
            try_is_user_banned_and_muted(&[valid], &context, &subject, 101, None).unwrap(),
            (true, false)
        );
        let mut json = original.clone();
        json.encoding = FactEncoding::Json;
        json.payload = serde_json::to_vec(&ban).unwrap();
        assert_eq!(
            try_is_user_banned_and_muted(&[wrap(json)], &context, &subject, 101, None).unwrap(),
            (true, false)
        );
        for type_id in [
            HOME_BAN_FACT_TYPE_ID,
            HOME_UNBAN_FACT_TYPE_ID,
            HOME_MUTE_FACT_TYPE_ID,
            HOME_UNMUTE_FACT_TYPE_ID,
        ] {
            let mut corrupt = original.clone();
            corrupt.type_id = aura_core::types::facts::FactTypeId::from(type_id);
            corrupt.encoding = FactEncoding::Json;
            corrupt.payload = b"not-json".to_vec();
            let error =
                try_is_user_banned_and_muted(&[wrap(corrupt)], &context, &subject, 101, None)
                    .unwrap_err();
            assert!(matches!(error, RequiredModerationQueryError::Json(_)));
            assert!(error.source().unwrap().is::<serde_json::Error>());
        }
        let mut schema = original.clone();
        schema.schema_version = 1;
        assert!(matches!(
            try_is_user_banned_and_muted(&[wrap(schema)], &context, &subject, 101, None),
            Err(RequiredModerationQueryError::Envelope(_))
        ));
        let wrong = ContextId::new_from_entropy([234; 32]);
        let mismatch = create_test_fact(
            RelationalFact::Generic {
                context_id: wrong,
                envelope: original.clone(),
            },
            0,
        );
        assert!(matches!(
            try_is_user_banned_and_muted(&[mismatch], &context, &subject, 101, None),
            Err(RequiredModerationQueryError::ContextMismatch { .. })
        ));
        let mut mislabeled = original;
        mislabeled.encoding = FactEncoding::Json;
        assert!(matches!(
            try_is_user_banned_and_muted(&[wrap(mislabeled)], &context, &subject, 101, None),
            Err(RequiredModerationQueryError::Json(_))
        ));
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
            causal: c(2),
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
            causal: c(3),
            context_id: context.clone(),
            channel_id: None,
            banned_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "test ban".to_string(),
            banned_at: pt(1000),
            expires_at: None,
        };
        let unban_fact = HomeUnbanFact {
            causal: revoking(4, HomeGovernanceEvent::Ban(ban_fact.clone())),
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
        let reversed: Vec<_> = facts.iter().rev().cloned().collect();
        assert!(
            query_current_bans(&reversed, &context, 3000).is_empty(),
            "unban arriving before its ban still removes it"
        );
    }

    #[test]
    fn test_query_current_bans_expired() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            causal: c(5),
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
            causal: c(6),
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
            causal: c(7),
            context_id: context.clone(),
            channel_id: None,
            muted_authority: user1.clone(),
            actor_authority: moderator.clone(),
            duration_secs: None,
            muted_at: pt(1000),
            expires_at: None,
        };
        let unmute_fact = HomeUnmuteFact {
            causal: revoking(8, HomeGovernanceEvent::Mute(mute_fact.clone())),
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
            causal: c(9),
            context_id: context.clone(),
            channel_id: channel.clone(),
            kicked_authority: user1.clone(),
            actor_authority: moderator.clone(),
            reason: "first kick".to_string(),
            kicked_at: pt(1000),
        };
        let kick_fact2 = HomeKickFact {
            causal: c(10),
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
        let reversed: Vec<_> = facts.iter().rev().cloned().collect();
        let reordered: Vec<_> = query_kick_history(&reversed, &context)
            .into_iter()
            .map(|kick| kick.kicked_authority)
            .collect();
        assert_eq!(
            reordered,
            vec![user1, user2],
            "history is causal, not arrival order"
        );
    }

    #[test]
    fn test_is_user_banned() {
        let context = test_context();
        let user1 = test_authority(1);
        let moderator = test_authority(2);

        let ban_fact = HomeBanFact {
            causal: c(11),
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
            causal: c(12),
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
            causal: c(13),
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
                    causal: c(14),
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
                    causal: c(15),
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
                    causal: c(16),
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
                    causal: c(17),
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

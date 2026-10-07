//! Home governance facts with order-independent semantics (docs/115 §3.4).
//!
//! Bans, mutes and moderator grants are tagged observed-remove sets: a
//! reversal (unban, unmute, revoke) removes exactly the add tags its writer
//! observed. Access overrides and the capability configuration are
//! multi-value registers: a write supersedes the writes its writer observed,
//! and truly concurrent survivors resolve by an explicit policy (most
//! restrictive) with a causal deterministic tie-break. Every governance fact
//! carries the writer's logical clock, which orders history (kicks) for
//! display. Physical time is kept for user-visible timestamps and expiry only.
//!
//! Membership is an observed-remove set of episodes: each `MemberJoined`
//! starts one (its tag), and a kick or `MemberLeft` revokes exactly the join
//! tags of that member its writer observed. A rejoin the kicker never saw
//! survives the kick in every arrival order, including a reinstalled member
//! pulling the old kick back through home-context sync.
//!
//! The generic rules live in `aura_journal::causal_reduction`; this module
//! binds them to the home governance fact family.

use super::facts::{
    HomeBanFact, HomeGrantModeratorFact, HomeKickFact, HomeMuteFact, HomeRevokeModeratorFact,
    HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID,
};
use super::query::RequiredModerationQueryError;
use crate::facts::{AccessLevel, AccessLevelCapabilityConfig, SocialFact, SOCIAL_FACT_TYPE_ID};
use aura_core::time::{CausalClock, CausalMetadata, CausalTag, LogicalTime, VectorClock};
use aura_core::types::facts::FactEnvelope;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::causal_reduction::{
    causal_cmp, merged_vector, observed_remove_live, register_survivors, CausalFact,
};
use aura_journal::fact::RelationalFact;
use aura_journal::DomainFact;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Tag domain of membership episodes (`MemberJoined`).
const MEMBERSHIP_EPISODE_TAG_DOMAIN: &str = "social:membership-episode";

/// Causal metadata of a join: it revokes and supersedes nothing. Its tag
/// names its membership episode (see `TaggedHomeGovernanceEvent::from_event`).
static JOIN_CAUSAL: CausalMetadata = CausalMetadata {
    revokes: Vec::new(),
    supersedes: Vec::new(),
    clock: CausalClock {
        lamport: 0,
        vector: Vec::new(),
    },
};

/// One decoded home governance fact.
#[derive(Debug, Clone)]
pub enum HomeGovernanceEvent {
    /// Ban add.
    Ban(HomeBanFact),
    /// Ban reversal.
    Unban(HomeUnbanFact),
    /// Mute add.
    Mute(HomeMuteFact),
    /// Mute reversal.
    Unmute(HomeUnmuteFact),
    /// Kick (history entry).
    Kick(HomeKickFact),
    /// Moderator designation add.
    GrantModerator(HomeGrantModeratorFact),
    /// Moderator designation reversal.
    RevokeModerator(HomeRevokeModeratorFact),
    /// Access override register write (`SocialFact::AccessOverrideSet`).
    AccessOverride(SocialFact),
    /// Capability configuration register write
    /// (`SocialFact::AccessLevelCapabilitiesConfigured`).
    CapabilityConfig(SocialFact),
    /// Membership episode add (`SocialFact::MemberJoined`).
    MemberJoined(SocialFact),
    /// Membership episode reversal (`SocialFact::MemberLeft`).
    MemberLeft(SocialFact),
}

/// A decoded governance fact with its causal metadata.
#[derive(Debug, Clone)]
pub struct TaggedHomeGovernanceEvent {
    /// The fact's tag, derived from its canonical content (type id and
    /// encoded payload, which names the author). Never read from the wire.
    pub tag: CausalTag,
    /// The fact's causal metadata.
    pub causal: CausalMetadata,
    /// The decoded fact.
    pub event: HomeGovernanceEvent,
}

/// What a new governance fact is about. It determines which observed facts
/// the new fact revokes or supersedes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeGovernanceKey {
    /// Ban `target` in `channel` (home-wide when `None`).
    Ban {
        /// Banned authority.
        target: AuthorityId,
        /// Channel scope.
        channel: Option<ChannelId>,
    },
    /// Unban `target` in `channel`.
    Unban {
        /// Unbanned authority.
        target: AuthorityId,
        /// Channel scope.
        channel: Option<ChannelId>,
    },
    /// Mute `target` in `channel`.
    Mute {
        /// Muted authority.
        target: AuthorityId,
        /// Channel scope.
        channel: Option<ChannelId>,
    },
    /// Unmute `target` in `channel`.
    Unmute {
        /// Unmuted authority.
        target: AuthorityId,
        /// Channel scope.
        channel: Option<ChannelId>,
    },
    /// Kick `target` from `channel`.
    Kick {
        /// Kicked authority.
        target: AuthorityId,
        /// Channel.
        channel: ChannelId,
    },
    /// Designate `target` a moderator.
    GrantModerator {
        /// Designated authority.
        target: AuthorityId,
    },
    /// Revoke `target`'s moderator designation.
    RevokeModerator {
        /// Authority losing the designation.
        target: AuthorityId,
    },
    /// Set `target`'s access override.
    AccessOverride {
        /// Authority receiving the override.
        target: AuthorityId,
    },
    /// Set the home's capability configuration.
    CapabilityConfig,
    /// `target` leaves the home.
    Leave {
        /// Leaving authority.
        target: AuthorityId,
    },
}

impl HomeGovernanceEvent {
    fn causal(&self) -> Option<&CausalMetadata> {
        match self {
            Self::Ban(f) => Some(&f.causal),
            Self::Unban(f) => Some(&f.causal),
            Self::Mute(f) => Some(&f.causal),
            Self::Unmute(f) => Some(&f.causal),
            Self::Kick(f) => Some(&f.causal),
            Self::GrantModerator(f) => Some(&f.causal),
            Self::RevokeModerator(f) => Some(&f.causal),
            Self::AccessOverride(SocialFact::AccessOverrideSet { causal, .. })
            | Self::CapabilityConfig(SocialFact::AccessLevelCapabilitiesConfigured {
                causal,
                ..
            }) => Some(causal),
            Self::MemberLeft(SocialFact::MemberLeft { causal, .. }) => Some(causal),
            Self::MemberJoined(SocialFact::MemberJoined { .. }) => Some(&JOIN_CAUSAL),
            Self::AccessOverride(_)
            | Self::CapabilityConfig(_)
            | Self::MemberJoined(_)
            | Self::MemberLeft(_) => None,
        }
    }

    /// Authority that authored the fact.
    #[must_use]
    pub fn actor(&self) -> Option<AuthorityId> {
        match self {
            Self::Ban(f) => Some(f.actor_authority),
            Self::Unban(f) => Some(f.actor_authority),
            Self::Mute(f) => Some(f.actor_authority),
            Self::Unmute(f) => Some(f.actor_authority),
            Self::Kick(f) => Some(f.actor_authority),
            Self::GrantModerator(f) => Some(f.actor_authority),
            Self::RevokeModerator(f) => Some(f.actor_authority),
            Self::AccessOverride(SocialFact::AccessOverrideSet { actor_id, .. })
            | Self::CapabilityConfig(SocialFact::AccessLevelCapabilitiesConfigured {
                actor_id,
                ..
            }) => Some(*actor_id),
            Self::MemberJoined(SocialFact::MemberJoined { authority_id, .. })
            | Self::MemberLeft(SocialFact::MemberLeft { authority_id, .. }) => Some(*authority_id),
            Self::AccessOverride(_)
            | Self::CapabilityConfig(_)
            | Self::MemberJoined(_)
            | Self::MemberLeft(_) => None,
        }
    }

    /// Home context of the fact.
    #[must_use]
    pub fn context_id(&self) -> ContextId {
        match self {
            Self::Ban(f) => f.context_id,
            Self::Unban(f) => f.context_id,
            Self::Mute(f) => f.context_id,
            Self::Unmute(f) => f.context_id,
            Self::Kick(f) => f.context_id,
            Self::GrantModerator(f) => f.context_id,
            Self::RevokeModerator(f) => f.context_id,
            Self::AccessOverride(fact)
            | Self::CapabilityConfig(fact)
            | Self::MemberJoined(fact)
            | Self::MemberLeft(fact) => fact.context_id(),
        }
    }

    /// Encode as the committed relational fact.
    #[must_use]
    pub fn to_generic(&self) -> RelationalFact {
        match self {
            Self::Ban(f) => f.to_generic(),
            Self::Unban(f) => f.to_generic(),
            Self::Mute(f) => f.to_generic(),
            Self::Unmute(f) => f.to_generic(),
            Self::Kick(f) => f.to_generic(),
            Self::GrantModerator(f) => f.to_generic(),
            Self::RevokeModerator(f) => f.to_generic(),
            Self::AccessOverride(fact)
            | Self::CapabilityConfig(fact)
            | Self::MemberJoined(fact)
            | Self::MemberLeft(fact) => fact.to_generic(),
        }
    }

    fn ban_add(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::Ban(f) => Some((f.banned_authority, f.channel_id)),
            _ => None,
        }
    }

    fn ban_remove(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::Unban(f) => Some((f.unbanned_authority, f.channel_id)),
            _ => None,
        }
    }

    fn mute_add(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::Mute(f) => Some((f.muted_authority, f.channel_id)),
            _ => None,
        }
    }

    fn mute_remove(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::Unmute(f) => Some((f.unmuted_authority, f.channel_id)),
            _ => None,
        }
    }

    /// Member whose episode this join starts.
    fn join_add(&self) -> Option<AuthorityId> {
        match self {
            Self::MemberJoined(SocialFact::MemberJoined { authority_id, .. }) => {
                Some(*authority_id)
            }
            _ => None,
        }
    }

    /// Member whose observed episodes this kick or leave ends.
    fn episode_end(&self) -> Option<AuthorityId> {
        match self {
            Self::Kick(f) => Some(f.kicked_authority),
            Self::MemberLeft(SocialFact::MemberLeft { authority_id, .. }) => Some(*authority_id),
            _ => None,
        }
    }

    fn grant_add(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::GrantModerator(f) => Some((f.target_authority, None)),
            _ => None,
        }
    }

    fn grant_remove(&self) -> Option<(AuthorityId, Option<ChannelId>)> {
        match self {
            Self::RevokeModerator(f) => Some((f.target_authority, None)),
            _ => None,
        }
    }

    /// Target and level of an access override write.
    #[must_use]
    pub fn override_target(&self) -> Option<(AuthorityId, AccessLevel)> {
        match self {
            Self::AccessOverride(SocialFact::AccessOverrideSet {
                authority_id,
                access_level,
                ..
            }) => Some((*authority_id, *access_level)),
            _ => None,
        }
    }
}

impl TaggedHomeGovernanceEvent {
    /// Decode a committed envelope under its wrapper context. Returns
    /// `Ok(None)` for facts outside the home governance family.
    ///
    /// # Errors
    /// Fails on schema, codec or context mismatch of a governance fact.
    pub fn try_decode(
        outer: ContextId,
        envelope: &FactEnvelope,
    ) -> Result<Option<Self>, RequiredModerationQueryError> {
        let event = match envelope.type_id.as_str() {
            HOME_BAN_FACT_TYPE_ID => HomeGovernanceEvent::Ban(
                HomeBanFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_UNBAN_FACT_TYPE_ID => HomeGovernanceEvent::Unban(
                HomeUnbanFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_MUTE_FACT_TYPE_ID => HomeGovernanceEvent::Mute(
                HomeMuteFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_UNMUTE_FACT_TYPE_ID => HomeGovernanceEvent::Unmute(
                HomeUnmuteFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_KICK_FACT_TYPE_ID => HomeGovernanceEvent::Kick(
                HomeKickFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_GRANT_MODERATOR_FACT_TYPE_ID => HomeGovernanceEvent::GrantModerator(
                HomeGrantModeratorFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            HOME_REVOKE_MODERATOR_FACT_TYPE_ID => HomeGovernanceEvent::RevokeModerator(
                HomeRevokeModeratorFact::try_from_envelope_in_context(envelope, outer)?,
            ),
            SOCIAL_FACT_TYPE_ID => {
                let fact = SocialFact::try_from_envelope(envelope)?;
                let event = match fact {
                    SocialFact::AccessOverrideSet { .. } => {
                        HomeGovernanceEvent::AccessOverride(fact)
                    }
                    SocialFact::AccessLevelCapabilitiesConfigured { .. } => {
                        HomeGovernanceEvent::CapabilityConfig(fact)
                    }
                    SocialFact::MemberJoined { .. } => HomeGovernanceEvent::MemberJoined(fact),
                    SocialFact::MemberLeft { .. } => HomeGovernanceEvent::MemberLeft(fact),
                    _ => return Ok(None),
                };
                if event.context_id() != outer {
                    return Err(RequiredModerationQueryError::ContextMismatch {
                        outer,
                        payload: event.context_id(),
                    });
                }
                event
            }
            _ => return Ok(None),
        };
        Ok(Self::from_event(event))
    }

    /// Wrap a governance fact (`None` for a non-governance social fact). The
    /// tag is the hash of the fact's type id and canonical re-encoding, so it
    /// binds the whole content, author included: a fact cannot claim another
    /// fact's tag, and distinct content never shares a tag. A join's tag is
    /// its membership episode instead (member, context and episode id), so
    /// the inviter's and the invitee's copies of one join are one episode.
    #[must_use]
    pub fn from_event(event: HomeGovernanceEvent) -> Option<Self> {
        let causal = event.causal()?.clone();
        let tag = if let HomeGovernanceEvent::MemberJoined(SocialFact::MemberJoined {
            authority_id,
            context_id,
            episode,
            ..
        }) = &event
        {
            let identity = format!("{authority_id}\0{context_id}\0{episode}");
            CausalTag::from_content(MEMBERSHIP_EPISODE_TAG_DOMAIN, identity.as_bytes())
        } else {
            let RelationalFact::Generic { envelope, .. } = event.to_generic() else {
                return None;
            };
            CausalTag::from_content(envelope.type_id.as_str(), &envelope.payload)
        };
        Some(Self { tag, causal, event })
    }
}

impl CausalFact for TaggedHomeGovernanceEvent {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }

    fn causal_metadata(&self) -> &CausalMetadata {
        &self.causal
    }
}

/// Merged vector clock of the observed governance facts of one home: the
/// `observed` argument for the writer's `LogicalClockEffects::logical_advance`.
#[must_use]
pub fn observed_governance_vector(observed: &[TaggedHomeGovernanceEvent]) -> VectorClock {
    merged_vector(observed.iter().map(|event| &event.causal.clock))
}

/// Causal metadata for a new governance fact about `key`, written by a
/// writer holding `observed` (the governance facts of the same home) at its
/// freshly advanced logical `clock`. A reversal revokes every observed add of
/// the same key; a register write supersedes every observed write of the same
/// key. The new fact's own tag derives from its content on decode.
#[must_use]
pub fn home_governance_causal(
    key: HomeGovernanceKey,
    observed: &[TaggedHomeGovernanceEvent],
    clock: &LogicalTime,
) -> CausalMetadata {
    let tags_where = |matches: &dyn Fn(&HomeGovernanceEvent) -> bool| -> Vec<CausalTag> {
        observed
            .iter()
            .filter(|event| matches(&event.event))
            .map(|event| event.tag)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let (revokes, supersedes) = match key {
        HomeGovernanceKey::Unban { target, channel } => (
            tags_where(&|event| event.ban_add() == Some((target, channel))),
            Vec::new(),
        ),
        HomeGovernanceKey::Unmute { target, channel } => (
            tags_where(&|event| event.mute_add() == Some((target, channel))),
            Vec::new(),
        ),
        HomeGovernanceKey::RevokeModerator { target } => (
            tags_where(&|event| event.grant_add() == Some((target, None))),
            Vec::new(),
        ),
        HomeGovernanceKey::AccessOverride { target } => (
            Vec::new(),
            tags_where(&|event| {
                event
                    .override_target()
                    .is_some_and(|(other, _)| other == target)
            }),
        ),
        HomeGovernanceKey::CapabilityConfig => (
            Vec::new(),
            tags_where(&|event| matches!(event, HomeGovernanceEvent::CapabilityConfig(_))),
        ),
        HomeGovernanceKey::Kick { target, .. } | HomeGovernanceKey::Leave { target } => (
            tags_where(&|event| event.join_add() == Some(target)),
            Vec::new(),
        ),
        HomeGovernanceKey::Ban { .. }
        | HomeGovernanceKey::Mute { .. }
        | HomeGovernanceKey::GrantModerator { .. } => (Vec::new(), Vec::new()),
    };
    CausalMetadata {
        revokes,
        supersedes,
        clock: CausalClock::from_logical(clock),
    }
}

fn live_or_set(
    events: &[&TaggedHomeGovernanceEvent],
    add: impl Fn(&HomeGovernanceEvent) -> Option<(AuthorityId, Option<ChannelId>)>,
    remove: impl Fn(&HomeGovernanceEvent) -> Option<(AuthorityId, Option<ChannelId>)>,
) -> BTreeSet<CausalTag> {
    let adds: Vec<_> = events
        .iter()
        .filter_map(|event| add(&event.event).map(|key| (key, event.tag)))
        .collect();
    let reversals: Vec<_> = events
        .iter()
        .filter_map(|event| remove(&event.event).map(|key| (key, event.causal.revokes.as_slice())))
        .collect();
    observed_remove_live(&adds, &reversals)
}

/// Live ban adds (tags) among `events`.
#[must_use]
pub fn live_ban_tags(events: &[&TaggedHomeGovernanceEvent]) -> BTreeSet<CausalTag> {
    live_or_set(
        events,
        HomeGovernanceEvent::ban_add,
        HomeGovernanceEvent::ban_remove,
    )
}

/// Live mute adds (tags) among `events`.
#[must_use]
pub fn live_mute_tags(events: &[&TaggedHomeGovernanceEvent]) -> BTreeSet<CausalTag> {
    live_or_set(
        events,
        HomeGovernanceEvent::mute_add,
        HomeGovernanceEvent::mute_remove,
    )
}

/// Live moderator grants (tags) among `events`.
#[must_use]
pub fn live_moderator_grant_tags(events: &[&TaggedHomeGovernanceEvent]) -> BTreeSet<CausalTag> {
    live_or_set(
        events,
        HomeGovernanceEvent::grant_add,
        HomeGovernanceEvent::grant_remove,
    )
}

/// Membership of every member with a join among `events`: whether any of its
/// episodes is live. A join starts an episode; a kick or leave ends exactly
/// the episodes (join tags) its writer observed, so a join the kicker never
/// saw (a rejoin, even one concurrent with the kick) survives. Callers pass
/// only authorized kicks. Independent of input order.
#[must_use]
pub fn membership_liveness(events: &[&TaggedHomeGovernanceEvent]) -> BTreeMap<AuthorityId, bool> {
    let adds: Vec<_> = events
        .iter()
        .filter_map(|event| event.event.join_add().map(|member| (member, event.tag)))
        .collect();
    let ends: Vec<_> = events
        .iter()
        .filter_map(|event| {
            event
                .event
                .episode_end()
                .map(|member| (member, event.causal.revokes.as_slice()))
        })
        .collect();
    let live = observed_remove_live(&adds, &ends);
    let mut liveness = BTreeMap::new();
    for (member, tag) in adds {
        *liveness.entry(member).or_insert(false) |= live.contains(&tag);
    }
    liveness
}

/// Effective access overrides: per target, the surviving override writes;
/// concurrent survivors resolve to the most restrictive level, then the later
/// in causal order.
#[must_use]
pub fn resolved_access_overrides(
    events: &[&TaggedHomeGovernanceEvent],
) -> BTreeMap<AuthorityId, AccessLevel> {
    let mut by_target: BTreeMap<AuthorityId, Vec<&TaggedHomeGovernanceEvent>> = BTreeMap::new();
    for event in events {
        if let Some((target, _)) = event.event.override_target() {
            by_target.entry(target).or_default().push(event);
        }
    }
    by_target
        .into_iter()
        .filter_map(|(target, writes)| {
            let survivors = register_survivors(&writes);
            writes
                .into_iter()
                .filter(|write| survivors.contains(&write.tag))
                .filter_map(|write| {
                    write
                        .event
                        .override_target()
                        .map(|(_, level)| (level, write))
                })
                .min_by(|(left_level, left), (right_level, right)| {
                    left_level
                        .cmp(right_level)
                        .then_with(|| causal_cmp(*right, *left))
                })
                .map(|(level, _)| (target, level))
        })
        .collect()
}

/// Effective capability configuration: the surviving configuration writes;
/// concurrent survivors combine by intersection per level (most restrictive).
#[must_use]
pub fn resolved_capability_config(
    events: &[&TaggedHomeGovernanceEvent],
) -> Option<AccessLevelCapabilityConfig> {
    let writes: Vec<_> = events
        .iter()
        .copied()
        .filter(|event| matches!(event.event, HomeGovernanceEvent::CapabilityConfig(_)))
        .collect();
    let survivors = register_survivors(&writes);
    writes
        .into_iter()
        .filter(|write| survivors.contains(&write.tag))
        .filter_map(|write| match &write.event {
            HomeGovernanceEvent::CapabilityConfig(
                SocialFact::AccessLevelCapabilitiesConfigured {
                    full_caps,
                    partial_caps,
                    limited_caps,
                    ..
                },
            ) => Some(AccessLevelCapabilityConfig {
                full: full_caps.iter().cloned().collect(),
                partial: partial_caps.iter().cloned().collect(),
                limited: limited_caps.iter().cloned().collect(),
            }),
            _ => None,
        })
        .reduce(|left, right| AccessLevelCapabilityConfig {
            full: left.full.intersection(&right.full).cloned().collect(),
            partial: left.partial.intersection(&right.partial).cloned().collect(),
            limited: left.limited.intersection(&right.limited).cloned().collect(),
        })
}

/// Sort governance facts into causal display order.
pub fn sort_causally(events: &mut [&TaggedHomeGovernanceEvent]) {
    events.sort_by(|a, b| causal_order(a, b));
}

/// Compare two governance facts in causal display order.
#[must_use]
pub fn causal_order(a: &TaggedHomeGovernanceEvent, b: &TaggedHomeGovernanceEvent) -> Ordering {
    causal_cmp(a, b)
}

/// Deterministic writer clocks for governance reducer tests. Each call is one
/// writer advance on `device` after observing `observed`, as
/// `LogicalClockEffects::logical_advance` returns it.
pub mod test_support {
    use super::*;
    use aura_core::types::identifiers::DeviceId;

    /// The clock `device` reaches after observing `observed`.
    #[must_use]
    pub fn advance(device: u8, observed: &[TaggedHomeGovernanceEvent]) -> LogicalTime {
        let mut vector = observed_governance_vector(observed);
        let id = DeviceId(uuid::Uuid::from_bytes([device; 16]));
        let next = vector.get(&id).copied().unwrap_or(0) + 1;
        vector.insert(id, next);
        LogicalTime {
            vector,
            lamport: next,
        }
    }

    /// Causal metadata for `key` written by `device` after observing `observed`.
    #[must_use]
    pub fn causal(
        device: u8,
        key: HomeGovernanceKey,
        observed: &[TaggedHomeGovernanceEvent],
    ) -> CausalMetadata {
        home_governance_causal(key, observed, &advance(device, observed))
    }

    /// Wrap a constructed fact after a codec round trip.
    ///
    /// # Panics
    /// Panics when the fact does not decode as a governance fact.
    #[must_use]
    pub fn tagged(event: HomeGovernanceEvent) -> TaggedHomeGovernanceEvent {
        let RelationalFact::Generic {
            context_id,
            envelope,
        } = event.to_generic()
        else {
            panic!("governance facts are generic relational facts");
        };
        match TaggedHomeGovernanceEvent::try_decode(context_id, &envelope) {
            Ok(Some(tagged)) => tagged,
            other => panic!("governance fact does not round-trip: {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{causal, tagged};
    use super::*;
    use aura_core::types::identifiers::HomeId;
    use aura_journal::causal_reduction::assert_permutation_invariant;

    fn ctx() -> ContextId {
        ContextId::new_from_entropy([3; 32])
    }

    fn who(byte: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([byte; 32])
    }

    fn ban(
        device: u8,
        actor: u8,
        target: u8,
        at_ms: u64,
        observed: &[TaggedHomeGovernanceEvent],
    ) -> TaggedHomeGovernanceEvent {
        let key = HomeGovernanceKey::Ban {
            target: who(target),
            channel: None,
        };
        tagged(HomeGovernanceEvent::Ban(HomeBanFact::new_ms(
            ctx(),
            None,
            who(target),
            who(actor),
            "r".into(),
            at_ms,
            None,
            causal(device, key, observed),
        )))
    }

    fn unban(
        device: u8,
        actor: u8,
        target: u8,
        at_ms: u64,
        observed: &[TaggedHomeGovernanceEvent],
    ) -> TaggedHomeGovernanceEvent {
        let key = HomeGovernanceKey::Unban {
            target: who(target),
            channel: None,
        };
        tagged(HomeGovernanceEvent::Unban(HomeUnbanFact::new_ms(
            ctx(),
            None,
            who(target),
            who(actor),
            at_ms,
            causal(device, key, observed),
        )))
    }

    fn live_targets(events: &[TaggedHomeGovernanceEvent]) -> BTreeSet<AuthorityId> {
        let refs: Vec<_> = events.iter().collect();
        let live = live_ban_tags(&refs);
        events
            .iter()
            .filter(|event| live.contains(&event.tag))
            .filter_map(|event| event.event.ban_add().map(|(target, _)| target))
            .collect()
    }

    #[test]
    fn ban_unban_ban_converges_in_every_arrival_order() {
        let first = ban(1, 1, 2, 50, &[]);
        // Physical time deliberately runs backwards: it must not matter.
        let lift = unban(1, 1, 2, 10, std::slice::from_ref(&first));
        let unbanned = assert_permutation_invariant(&[first.clone(), lift.clone()], live_targets);
        assert!(unbanned.is_empty());

        let again = ban(1, 1, 2, 5, &[first.clone(), lift.clone()]);
        let banned = assert_permutation_invariant(&[first, lift, again], live_targets);
        assert_eq!(banned, BTreeSet::from([who(2)]));
    }

    #[test]
    fn concurrent_unobserved_ban_survives_unban() {
        let ban_a = ban(1, 1, 2, 1, &[]);
        let ban_b = ban(2, 3, 2, 1, &[]);
        let unban_a = unban(1, 1, 2, 2, std::slice::from_ref(&ban_a));
        let banned = assert_permutation_invariant(&[ban_a, ban_b, unban_a], live_targets);
        assert_eq!(banned, BTreeSet::from([who(2)]));
    }

    #[test]
    fn concurrent_overrides_resolve_most_restrictive_and_sequential_override_wins() {
        let target = who(9);
        let home = HomeId::from_bytes([4; 32]);
        let set = |device: u8, level, observed: &[TaggedHomeGovernanceEvent]| {
            tagged(HomeGovernanceEvent::AccessOverride(
                SocialFact::access_override_set_ms(
                    target,
                    home,
                    ctx(),
                    level,
                    who(1),
                    1,
                    causal(
                        device,
                        HomeGovernanceKey::AccessOverride { target },
                        observed,
                    ),
                ),
            ))
        };
        let partial = set(1, AccessLevel::Partial, &[]);
        let limited = set(2, AccessLevel::Limited, &[]);
        let resolve = |events: &[TaggedHomeGovernanceEvent]| {
            let refs: Vec<_> = events.iter().collect();
            resolved_access_overrides(&refs)
        };
        let concurrent = assert_permutation_invariant(&[partial.clone(), limited.clone()], resolve);
        assert_eq!(concurrent.get(&target), Some(&AccessLevel::Limited));

        let relaxed = set(1, AccessLevel::Partial, &[partial.clone(), limited.clone()]);
        let sequential = assert_permutation_invariant(&[partial, limited, relaxed], resolve);
        assert_eq!(sequential.get(&target), Some(&AccessLevel::Partial));
    }

    #[test]
    fn tags_are_unique_per_writer_advance_and_payload_rejects_other_schemas() {
        let first = ban(1, 1, 2, 1, &[]);
        let second = ban(1, 1, 2, 1, std::slice::from_ref(&first));
        assert_ne!(first.tag, second.tag);
        let RelationalFact::Generic { mut envelope, .. } = second.event.to_generic() else {
            panic!("generic");
        };
        assert_eq!(envelope.schema_version, 3);
        for unsupported in [2, 4] {
            envelope.schema_version = unsupported;
            assert!(TaggedHomeGovernanceEvent::try_decode(ctx(), &envelope).is_err());
        }
    }

    #[test]
    fn tag_is_derived_from_content_so_a_peer_cannot_reuse_another_facts_tag() {
        let real = ban(1, 1, 2, 1, &[]);
        assert_eq!(tagged(real.event.clone()).tag, real.tag);

        // A peer replays the real ban's causal metadata under another author
        // to collide with (and shadow) it: the tag follows the content.
        let HomeGovernanceEvent::Ban(mut forged) = real.event.clone() else {
            panic!("ban");
        };
        forged.actor_authority = who(7);
        let forged = tagged(HomeGovernanceEvent::Ban(forged));
        assert_eq!(forged.causal, real.causal);
        assert_ne!(forged.tag, real.tag);

        // A reversal reusing the ban's causal metadata is a distinct fact and
        // revokes nothing it did not list; the ban stays live in every order.
        let reuse = tagged(HomeGovernanceEvent::Unban(HomeUnbanFact::new_ms(
            ctx(),
            None,
            who(2),
            who(7),
            1,
            real.causal.clone(),
        )));
        assert_ne!(reuse.tag, real.tag);
        let held = assert_permutation_invariant(&[real.clone(), forged.clone(), reuse], |order| {
            let mut log = BTreeMap::new();
            for event in order {
                log.entry(event.tag).or_insert_with(|| event.clone());
            }
            let refs: Vec<_> = log.values().collect();
            let live = live_ban_tags(&refs);
            (log.len(), live)
        });
        assert_eq!(held, (3, BTreeSet::from([real.tag, forged.tag])));
    }
}

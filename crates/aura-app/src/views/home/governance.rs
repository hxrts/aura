//! Pure home governance reduction over the whole governance fact set
//! (docs/115 §3.4, docs/105 order-independent reduction).
//!
//! Late and out-of-order delivery through home-context sync cannot change the
//! result: the reducer recomputes overrides, capability configuration,
//! moderator designations, bans, mutes and the kick history from the full
//! fact set every time, and authorizes each fact against the moderator set
//! reduced from that same set rather than the state at arrival.

use super::members::{HomeMember, HomeRole};
use super::moderation::{BanRecord, KickRecord, MuteRecord};
use super::state::HomeState;
use aura_core::time::CausalTag;
use aura_core::types::identifiers::AuthorityId;
use aura_social::moderation::governance::{
    live_ban_tags, live_moderator_grant_tags, live_mute_tags, membership_liveness,
    resolved_access_overrides, resolved_capability_config, sort_causally, HomeGovernanceEvent,
    TaggedHomeGovernanceEvent,
};
use std::collections::{BTreeMap, BTreeSet};

/// The governance facts held for one home, plus the reducer's bookkeeping:
/// members hidden because they are banned (restored when the ban is lifted).
#[derive(Debug, Clone, Default)]
pub struct HomeGovernanceLog {
    creator: Option<AuthorityId>,
    events: BTreeMap<CausalTag, TaggedHomeGovernanceEvent>,
    banned_members: BTreeMap<AuthorityId, HomeMember>,
}

impl HomeGovernanceLog {
    /// Record the home's creator, its permanent first moderator.
    pub fn set_creator(&mut self, creator: AuthorityId) {
        self.creator = Some(creator);
    }

    /// Add a governance fact; returns false for a fact already held.
    pub fn insert(&mut self, event: TaggedHomeGovernanceEvent) -> bool {
        self.events.insert(event.tag, event).is_none()
    }

    /// Whether the log holds no governance facts.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

fn of_kind<'a>(
    events: &[&'a TaggedHomeGovernanceEvent],
    keep: impl Fn(&HomeGovernanceEvent) -> bool,
) -> Vec<&'a TaggedHomeGovernanceEvent> {
    events
        .iter()
        .copied()
        .filter(|event| keep(&event.event))
        .collect()
}

/// Capability an override or capability-configuration writer must hold, in
/// addition to being a moderator: access governance is governance
/// facilitation, the same authority as designating moderators.
const ACCESS_GOVERNANCE_CAPABILITY: &str = "grant_moderator";

/// Derive threshold-member roles from the authorized moderator designations
/// in `events` under the home's current access state.
///
/// With the creator known (from `HomeCreated`), every threshold member's role
/// is derived: moderator iff creator or holding a live grant. Without it the
/// set of moderators is not known, so only the targets of authorized
/// designations are derived and every other role is kept as materialized; a
/// grant then never demotes a moderator that holds no grant (the creator).
fn apply_moderator_designations(
    home: &mut HomeState,
    creator: Option<AuthorityId>,
    events: &[&TaggedHomeGovernanceEvent],
    own: &AuthorityId,
) {
    let designations = of_kind(events, |event| {
        matches!(
            event,
            HomeGovernanceEvent::GrantModerator(_) | HomeGovernanceEvent::RevokeModerator(_)
        )
    })
    .into_iter()
    .filter(|event| {
        event
            .event
            .actor()
            .is_some_and(|actor| home.actor_may_designate_moderators(&actor))
    })
    .collect::<Vec<_>>();
    let live_grants = live_moderator_grant_tags(&designations);
    let mut moderators: BTreeSet<AuthorityId> = designations
        .iter()
        .filter(|event| live_grants.contains(&event.tag))
        .filter_map(|event| match &event.event {
            HomeGovernanceEvent::GrantModerator(grant) => Some(grant.target_authority),
            _ => None,
        })
        .collect();
    moderators.extend(creator);
    let derived: Option<BTreeSet<AuthorityId>> = match creator {
        Some(_) => None,
        None => Some(
            designations
                .iter()
                .filter_map(|event| match &event.event {
                    HomeGovernanceEvent::GrantModerator(grant) => Some(grant.target_authority),
                    HomeGovernanceEvent::RevokeModerator(revoke) => Some(revoke.target_authority),
                    _ => None,
                })
                .collect(),
        ),
    };
    if creator.is_none() && designations.is_empty() {
        return;
    }
    let is_derived = |id: &AuthorityId| derived.as_ref().is_none_or(|set| set.contains(id));
    let role_for = |id: &AuthorityId| {
        if moderators.contains(id) {
            HomeRole::Moderator
        } else {
            HomeRole::Member
        }
    };
    for member in &mut home.members {
        if member.role.is_threshold_member() && is_derived(&member.id) {
            member.role = role_for(&member.id);
        }
    }
    home.my_role = match home.member(own) {
        Some(member) => member.role,
        None if home.my_role.is_threshold_member() && is_derived(own) => role_for(own),
        None => home.my_role,
    };
}

/// Reduce the governance facts in `log` onto `home` for viewer `own`.
/// Returns whether the home changed. The result depends only on the set of
/// facts in `log` and the home's joined roster, never on insertion order.
pub fn reduce_home_governance(
    home: &mut HomeState,
    log: &mut HomeGovernanceLog,
    own: &AuthorityId,
) -> bool {
    let before = serde_json::to_value(&*home).ok();

    // Authorization uses the full joined roster: restore members hidden by
    // a ban before re-deriving which bans are live.
    for (id, member) in std::mem::take(&mut log.banned_members) {
        if home.member(&id).is_none() {
            home.add_member(member);
        }
    }

    let events: Vec<&TaggedHomeGovernanceEvent> = log.events.values().collect();

    // Access registers and moderator designations depend on each other, so
    // they are reduced in fixed strata, each a function of the fact set:
    // 1. moderators under default access (no overrides, default config);
    // 2. override and configuration writes by those moderators holding the
    //    governance capability, resolved as registers;
    // 3. moderators again under the resolved access state.
    let roster_roles: Vec<(AuthorityId, HomeRole)> = home
        .members
        .iter()
        .map(|member| (member.id, member.role))
        .collect();
    let roster_my_role = home.my_role;
    home.access_level_capabilities = None;
    home.access_overrides.clear();
    apply_moderator_designations(home, log.creator, &events, own);

    let access_writes = of_kind(&events, |event| {
        matches!(
            event,
            HomeGovernanceEvent::AccessOverride(_) | HomeGovernanceEvent::CapabilityConfig(_)
        )
    })
    .into_iter()
    .filter(|event| {
        event
            .event
            .actor()
            .is_some_and(|actor| home.actor_may_moderate(&actor, ACCESS_GOVERNANCE_CAPABILITY))
    })
    .collect::<Vec<_>>();
    let capabilities = resolved_capability_config(&access_writes);
    let overrides = resolved_access_overrides(&access_writes);

    for (id, role) in roster_roles {
        if let Some(member) = home.member_mut(&id) {
            member.role = role;
        }
    }
    home.my_role = roster_my_role;
    home.access_level_capabilities = capabilities;
    home.access_overrides = overrides.into_iter().collect();
    apply_moderator_designations(home, log.creator, &events, own);

    // Moderation acts, authorized against the reduced moderator roster.
    let authorized = |capability: &str, keep: &dyn Fn(&HomeGovernanceEvent) -> bool| {
        of_kind(&events, keep)
            .into_iter()
            .filter(|event| {
                event
                    .event
                    .actor()
                    .is_some_and(|actor| home.actor_may_moderate(&actor, capability))
            })
            .collect::<Vec<_>>()
    };
    let mut bans = authorized("moderate:ban", &|event| {
        matches!(
            event,
            HomeGovernanceEvent::Ban(_) | HomeGovernanceEvent::Unban(_)
        )
    });
    let mut mutes = authorized("moderate:mute", &|event| {
        matches!(
            event,
            HomeGovernanceEvent::Mute(_) | HomeGovernanceEvent::Unmute(_)
        )
    });
    let mut kicks = authorized("moderate:kick", &|event| {
        matches!(event, HomeGovernanceEvent::Kick(_))
    });
    sort_causally(&mut bans);
    sort_causally(&mut mutes);
    sort_causally(&mut kicks);

    let live_bans = live_ban_tags(&bans);
    home.ban_list = bans
        .iter()
        .filter(|event| live_bans.contains(&event.tag))
        .filter_map(|event| match &event.event {
            // Channel-scoped bans are enforced per channel by the runtime
            // query; the home list holds home-wide bans only.
            HomeGovernanceEvent::Ban(ban) if ban.channel_id.is_none() => Some((
                ban.banned_authority,
                BanRecord {
                    authority_id: ban.banned_authority,
                    reason: ban.reason.clone(),
                    actor: ban.actor_authority,
                    banned_at: ban.banned_at_ms(),
                },
            )),
            _ => None,
        })
        .collect();

    let live_mutes = live_mute_tags(&mutes);
    home.mute_list = mutes
        .iter()
        .filter(|event| live_mutes.contains(&event.tag))
        .filter_map(|event| match &event.event {
            HomeGovernanceEvent::Mute(mute) if mute.channel_id.is_none() => Some((
                mute.muted_authority,
                MuteRecord {
                    authority_id: mute.muted_authority,
                    duration_secs: mute.duration_secs,
                    muted_at: mute.muted_at_ms(),
                    expires_at: mute.expires_at_ms(),
                    actor: mute.actor_authority,
                },
            )),
            _ => None,
        })
        .collect();

    home.kick_log = kicks
        .iter()
        .filter_map(|event| match &event.event {
            HomeGovernanceEvent::Kick(kick) => Some(KickRecord {
                authority_id: kick.kicked_authority,
                channel: kick.channel_id,
                reason: kick.reason.clone(),
                actor: kick.actor_authority,
                kicked_at: kick.kicked_at_ms(),
            }),
            _ => None,
        })
        .collect();
    let overflow = home.kick_log.len().saturating_sub(HomeState::MAX_KICK_LOG);
    home.kick_log.drain(..overflow);

    // Membership episodes: a kick or leave ends the joins its writer
    // observed; a member whose every join has ended leaves the roster, and a
    // later (unobserved) join keeps them in it.
    let mut membership = of_kind(&events, |event| {
        matches!(
            event,
            HomeGovernanceEvent::MemberJoined(_) | HomeGovernanceEvent::MemberLeft(_)
        )
    });
    membership.extend(kicks.iter().copied());
    for (id, live) in membership_liveness(&membership) {
        if !live {
            let _ = home.remove_member(&id);
        }
    }

    // Banned members leave the visible roster until their ban is lifted.
    let banned: Vec<AuthorityId> = home.ban_list.keys().copied().collect();
    for id in banned {
        if let Some(member) = home.remove_member(&id) {
            log.banned_members.insert(id, member);
        }
    }

    serde_json::to_value(&*home).ok() != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::{ChannelId, ContextId, HomeId};
    use aura_journal::causal_reduction::assert_permutation_invariant;
    use aura_social::moderation::governance::test_support::{causal, tagged};
    use aura_social::moderation::governance::HomeGovernanceKey;
    use aura_social::moderation::{
        HomeBanFact, HomeGrantModeratorFact, HomeKickFact, HomeMuteFact, HomeRevokeModeratorFact,
        HomeUnbanFact, HomeUnmuteFact,
    };
    use aura_social::{AccessLevel, SocialFact};

    type Event = TaggedHomeGovernanceEvent;

    fn who(byte: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([byte; 32])
    }

    fn ctx() -> ContextId {
        ContextId::new_from_entropy([5; 32])
    }

    fn home_id() -> ChannelId {
        ChannelId::from_bytes([6; 32])
    }

    const OWNER: u8 = 1;
    const MEMBER: u8 = 2;
    const TARGET: u8 = 3;

    fn member(byte: u8, role: HomeRole) -> HomeMember {
        HomeMember {
            id: who(byte),
            name: format!("m{byte}"),
            role,
            is_online: false,
            joined_at: 0,
            last_seen: None,
            storage_allocated: 0,
        }
    }

    /// Owner (creator) plus a threshold member and a participant target.
    fn base_home() -> HomeState {
        let mut home = HomeState::new(home_id(), Some("h".into()), who(OWNER), 0, ctx());
        home.add_member(member(MEMBER, HomeRole::Member));
        home.add_member(member(TARGET, HomeRole::Participant));
        home
    }

    #[derive(Debug, PartialEq)]
    struct Observed {
        roles: Vec<(AuthorityId, HomeRole)>,
        my_role: HomeRole,
        banned: BTreeSet<AuthorityId>,
        muted: BTreeSet<AuthorityId>,
        overrides: BTreeMap<AuthorityId, AccessLevel>,
        kicks: Vec<AuthorityId>,
    }

    fn reduce(events: &[Event], viewer: u8) -> Observed {
        let mut log = HomeGovernanceLog::default();
        log.set_creator(who(OWNER));
        reduce_in(base_home(), log, events, viewer)
    }

    fn reduce_in(
        mut home: HomeState,
        mut log: HomeGovernanceLog,
        events: &[Event],
        viewer: u8,
    ) -> Observed {
        for event in events {
            // The view adds a joining member before reducing governance.
            if let HomeGovernanceEvent::MemberJoined(SocialFact::MemberJoined {
                authority_id,
                ..
            }) = &event.event
            {
                if home.member(authority_id).is_none() {
                    home.add_member(member(
                        authority_id_byte(authority_id),
                        HomeRole::Participant,
                    ));
                }
            }
            log.insert(event.clone());
            // Reduce after every arrival, as the view does per batch.
            reduce_home_governance(&mut home, &mut log, &who(viewer));
        }
        let mut roles: Vec<_> = home.members.iter().map(|m| (m.id, m.role)).collect();
        roles.sort_by_key(|(id, _)| *id);
        Observed {
            roles,
            my_role: home.my_role,
            banned: home.ban_list.keys().copied().collect(),
            muted: home.mute_list.keys().copied().collect(),
            overrides: home
                .access_overrides
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect(),
            kicks: home.kick_log.iter().map(|k| k.authority_id).collect(),
        }
    }

    fn ban(device: u8, actor: u8, observed: &[Event]) -> Event {
        let key = HomeGovernanceKey::Ban {
            target: who(TARGET),
            channel: None,
        };
        tagged(HomeGovernanceEvent::Ban(HomeBanFact::new_ms(
            ctx(),
            None,
            who(TARGET),
            who(actor),
            "r".into(),
            1,
            None,
            causal(device, key, observed),
        )))
    }

    fn unban(device: u8, actor: u8, observed: &[Event]) -> Event {
        let key = HomeGovernanceKey::Unban {
            target: who(TARGET),
            channel: None,
        };
        tagged(HomeGovernanceEvent::Unban(HomeUnbanFact::new_ms(
            ctx(),
            None,
            who(TARGET),
            who(actor),
            1,
            causal(device, key, observed),
        )))
    }

    fn grant(device: u8, target: u8, observed: &[Event]) -> Event {
        grant_by(device, OWNER, target, observed)
    }

    fn grant_by(device: u8, actor: u8, target: u8, observed: &[Event]) -> Event {
        let key = HomeGovernanceKey::GrantModerator {
            target: who(target),
        };
        tagged(HomeGovernanceEvent::GrantModerator(
            HomeGrantModeratorFact::new_ms(
                ctx(),
                who(target),
                who(actor),
                1,
                causal(device, key, observed),
            ),
        ))
    }

    fn revoke(device: u8, target: u8, observed: &[Event]) -> Event {
        let key = HomeGovernanceKey::RevokeModerator {
            target: who(target),
        };
        tagged(HomeGovernanceEvent::RevokeModerator(
            HomeRevokeModeratorFact::new_ms(
                ctx(),
                who(target),
                who(OWNER),
                1,
                causal(device, key, observed),
            ),
        ))
    }

    #[test]
    fn ban_unban_reban_is_order_independent() {
        let b1 = ban(1, OWNER, &[]);
        let u1 = unban(1, OWNER, std::slice::from_ref(&b1));
        let lifted =
            assert_permutation_invariant(&[b1.clone(), u1.clone()], |order| reduce(order, OWNER));
        assert!(lifted.banned.is_empty());
        assert!(
            lifted.roles.iter().any(|(id, _)| *id == who(TARGET)),
            "unban restores member"
        );

        let b2 = ban(1, OWNER, &[b1.clone(), u1.clone()]);
        let banned = assert_permutation_invariant(&[b1, u1, b2], |order| reduce(order, OWNER));
        assert_eq!(banned.banned, BTreeSet::from([who(TARGET)]));
        assert!(!banned.roles.iter().any(|(id, _)| *id == who(TARGET)));
    }

    #[test]
    fn mute_unmute_is_order_independent() {
        let mute_key = HomeGovernanceKey::Mute {
            target: who(TARGET),
            channel: None,
        };
        let mute = tagged(HomeGovernanceEvent::Mute(HomeMuteFact::new_ms(
            ctx(),
            None,
            who(TARGET),
            who(OWNER),
            None,
            1,
            None,
            causal(1, mute_key, &[]),
        )));
        let unmute_key = HomeGovernanceKey::Unmute {
            target: who(TARGET),
            channel: None,
        };
        let unmute = tagged(HomeGovernanceEvent::Unmute(HomeUnmuteFact::new_ms(
            ctx(),
            None,
            who(TARGET),
            who(OWNER),
            1,
            causal(1, unmute_key, std::slice::from_ref(&mute)),
        )));
        let state =
            assert_permutation_invariant(&[mute.clone(), unmute], |order| reduce(order, OWNER));
        assert!(state.muted.is_empty());
        let only_mute = reduce(std::slice::from_ref(&mute), OWNER);
        assert_eq!(only_mute.muted, BTreeSet::from([who(TARGET)]));
    }

    #[test]
    fn grant_revoke_is_order_independent_for_member_and_viewer() {
        let g = grant(1, MEMBER, &[]);
        let r = revoke(1, MEMBER, std::slice::from_ref(&g));
        let regrant = grant(1, MEMBER, &[g.clone(), r.clone()]);
        let revoked =
            assert_permutation_invariant(&[g.clone(), r.clone()], |order| reduce(order, MEMBER));
        assert!(revoked.roles.contains(&(who(MEMBER), HomeRole::Member)));
        assert_eq!(revoked.my_role, HomeRole::Member);
        let granted = assert_permutation_invariant(&[g, r, regrant], |order| reduce(order, MEMBER));
        assert!(granted.roles.contains(&(who(MEMBER), HomeRole::Moderator)));
        assert_eq!(granted.my_role, HomeRole::Moderator);
    }

    #[test]
    fn grant_before_join_takes_effect_when_the_member_joins() {
        let mut home = HomeState::new(home_id(), Some("h".into()), who(OWNER), 0, ctx());
        let mut log = HomeGovernanceLog::default();
        log.set_creator(who(OWNER));
        log.insert(grant(1, MEMBER, &[]));
        reduce_home_governance(&mut home, &mut log, &who(OWNER));
        assert!(home.member(&who(MEMBER)).is_none());
        home.add_member(member(MEMBER, HomeRole::Member));
        reduce_home_governance(&mut home, &mut log, &who(OWNER));
        assert_eq!(home.member(&who(MEMBER)).unwrap().role, HomeRole::Moderator);
    }

    #[test]
    fn ban_by_newly_granted_moderator_is_order_independent() {
        let g = grant(1, MEMBER, &[]);
        // The new moderator bans on its own device after seeing its grant.
        let b = ban(2, MEMBER, std::slice::from_ref(&g));
        let state = assert_permutation_invariant(&[b.clone(), g], |order| reduce(order, OWNER));
        assert_eq!(state.banned, BTreeSet::from([who(TARGET)]));
        // Without the grant the ban is not authorized.
        assert!(reduce(std::slice::from_ref(&b), OWNER).banned.is_empty());
    }

    #[test]
    fn concurrent_overrides_resolve_to_most_restrictive_in_every_order() {
        let set =
            |device: u8, level, observed: &[Event]| override_by(device, OWNER, level, observed);
        let a = set(1, AccessLevel::Partial, &[]);
        let b = set(2, AccessLevel::Limited, &[]);
        let concurrent =
            assert_permutation_invariant(&[a.clone(), b.clone()], |order| reduce(order, OWNER));
        assert_eq!(
            concurrent.overrides.get(&who(TARGET)),
            Some(&AccessLevel::Limited)
        );
        let c = set(1, AccessLevel::Partial, &[a.clone(), b.clone()]);
        let resolved = assert_permutation_invariant(&[a, b, c], |order| reduce(order, OWNER));
        assert_eq!(
            resolved.overrides.get(&who(TARGET)),
            Some(&AccessLevel::Partial)
        );
    }

    /// Task 120: one moderator sets Limited, then Partial, observing the
    /// Limited write; Partial holds in every order.
    #[test]
    fn sequential_overrides_by_one_moderator_keep_the_later_level_in_every_order() {
        let limited = override_by(1, OWNER, AccessLevel::Limited, &[]);
        let partial = override_by(
            1,
            OWNER,
            AccessLevel::Partial,
            std::slice::from_ref(&limited),
        );
        let state = assert_permutation_invariant(&[limited, partial], |order| reduce(order, OWNER));
        assert_eq!(
            state.overrides.get(&who(TARGET)),
            Some(&AccessLevel::Partial)
        );
    }

    #[test]
    fn channel_scoped_ban_stays_out_of_the_home_wide_list() {
        let channel = ChannelId::from_bytes([9; 32]);
        let scoped = tagged(HomeGovernanceEvent::Ban(HomeBanFact::new_ms(
            ctx(),
            Some(channel),
            who(TARGET),
            who(OWNER),
            "r".into(),
            1,
            None,
            causal(
                1,
                HomeGovernanceKey::Ban {
                    target: who(TARGET),
                    channel: Some(channel),
                },
                &[],
            ),
        )));
        let state = reduce(std::slice::from_ref(&scoped), OWNER);
        assert!(state.banned.is_empty());
        assert!(state.roles.iter().any(|(id, _)| *id == who(TARGET)));
    }

    fn override_by(device: u8, actor: u8, level: AccessLevel, observed: &[Event]) -> Event {
        tagged(HomeGovernanceEvent::AccessOverride(
            SocialFact::access_override_set_ms(
                who(TARGET),
                HomeId::from_bytes(*home_id().as_bytes()),
                ctx(),
                level,
                who(actor),
                1,
                causal(
                    device,
                    HomeGovernanceKey::AccessOverride {
                        target: who(TARGET),
                    },
                    observed,
                ),
            ),
        ))
    }

    fn capability_config_by(device: u8, actor: u8, observed: &[Event]) -> Event {
        tagged(HomeGovernanceEvent::CapabilityConfig(
            SocialFact::access_level_capabilities_configured_ms(
                HomeId::from_bytes(*home_id().as_bytes()),
                ctx(),
                vec!["send_message".into()],
                Vec::new(),
                Vec::new(),
                who(actor),
                1,
                causal(device, HomeGovernanceKey::CapabilityConfig, observed),
            ),
        ))
    }

    #[test]
    fn access_writes_by_a_non_moderator_are_ignored_in_every_order() {
        let forged_override = override_by(2, MEMBER, AccessLevel::Limited, &[]);
        let forged_config = capability_config_by(2, MEMBER, &[]);
        let state = assert_permutation_invariant(&[forged_override, forged_config], |order| {
            reduce(order, OWNER)
        });
        assert!(state.overrides.is_empty());
        // The ignored configuration did not strip the owner's moderation.
        assert!(state.roles.contains(&(who(OWNER), HomeRole::Moderator)));
    }

    #[test]
    fn override_by_a_moderator_granted_concurrently_or_later_holds_in_every_order() {
        // Concurrent: neither writer observed the other.
        let g = grant(1, MEMBER, &[]);
        let concurrent = override_by(2, MEMBER, AccessLevel::Partial, &[]);
        let state = assert_permutation_invariant(&[g, concurrent], |order| reduce(order, OWNER));
        assert_eq!(
            state.overrides.get(&who(TARGET)),
            Some(&AccessLevel::Partial)
        );

        // Later: the grant is written after observing the override.
        let early = override_by(2, MEMBER, AccessLevel::Limited, &[]);
        let later_grant = grant(1, MEMBER, std::slice::from_ref(&early));
        let state = assert_permutation_invariant(&[early.clone(), later_grant], |order| {
            reduce(order, OWNER)
        });
        assert_eq!(
            state.overrides.get(&who(TARGET)),
            Some(&AccessLevel::Limited)
        );

        // Without the grant the same override is ignored.
        assert!(reduce(&[early], OWNER).overrides.is_empty());
    }

    #[test]
    fn roles_without_home_created_never_demote_ungranted_moderators() {
        // The view materialized the home (e.g. from an invitation) with the
        // owner as moderator but never saw HomeCreated.
        let materialized = || {
            let mut home = base_home();
            home.designate_creator_moderator(&who(OWNER), &who(MEMBER));
            home
        };
        let g = grant(1, MEMBER, &[]);
        let r = revoke(1, MEMBER, std::slice::from_ref(&g));
        let granted = assert_permutation_invariant(std::slice::from_ref(&g), |order| {
            reduce_in(materialized(), HomeGovernanceLog::default(), order, MEMBER)
        });
        assert!(granted.roles.contains(&(who(OWNER), HomeRole::Moderator)));
        assert!(granted.roles.contains(&(who(MEMBER), HomeRole::Moderator)));
        assert_eq!(granted.my_role, HomeRole::Moderator);

        let revoked = assert_permutation_invariant(&[g, r], |order| {
            reduce_in(materialized(), HomeGovernanceLog::default(), order, MEMBER)
        });
        assert!(revoked.roles.contains(&(who(OWNER), HomeRole::Moderator)));
        assert!(revoked.roles.contains(&(who(MEMBER), HomeRole::Member)));
        assert_eq!(revoked.my_role, HomeRole::Member);
    }

    #[test]
    fn forged_reversal_naming_another_facts_tag_is_ignored() {
        let b = ban(1, OWNER, &[]);
        // A non-moderator lists the real ban's tag as revoked.
        let forged = unban(2, MEMBER, std::slice::from_ref(&b));
        assert!(forged.causal.revokes.contains(&b.tag));
        assert_ne!(forged.tag, b.tag);
        let state = assert_permutation_invariant(&[b, forged], |order| reduce(order, OWNER));
        assert_eq!(state.banned, BTreeSet::from([who(TARGET)]));
    }

    fn authority_id_byte(id: &AuthorityId) -> u8 {
        [OWNER, MEMBER, TARGET]
            .into_iter()
            .find(|byte| who(*byte) == *id)
            .expect("test authority")
    }

    /// `TARGET` joins (one membership episode per join, named by its tag).
    fn join(at_ms: u64) -> Event {
        tagged(HomeGovernanceEvent::MemberJoined(
            SocialFact::member_joined_ms(
                who(TARGET),
                aura_social::HomeId::from_bytes(*home_id().as_bytes()),
                ctx(),
                at_ms,
                "t".into(),
                format!("inv-{at_ms}"),
            ),
        ))
    }

    /// The owner kicks `TARGET` after observing `observed`.
    fn kick(device: u8, observed: &[Event]) -> Event {
        let key = HomeGovernanceKey::Kick {
            target: who(TARGET),
            channel: home_id(),
        };
        tagged(HomeGovernanceEvent::Kick(HomeKickFact::new_ms(
            ctx(),
            home_id(),
            who(TARGET),
            who(OWNER),
            "r".into(),
            1,
            causal(device, key, observed),
        )))
    }

    /// Owner and threshold member; `TARGET` enters only through joins.
    fn roster_without_target(events: &[Event], viewer: u8) -> Observed {
        let mut home = base_home();
        let _ = home.remove_member(&who(TARGET));
        let mut log = HomeGovernanceLog::default();
        log.set_creator(who(OWNER));
        reduce_in(home, log, events, viewer)
    }

    fn has_target(state: &Observed) -> bool {
        state.roles.iter().any(|(id, _)| *id == who(TARGET))
    }

    #[test]
    fn kick_ends_the_observed_episode_in_every_order() {
        let first = join(10);
        let kicked = kick(1, std::slice::from_ref(&first));
        assert!(kicked.causal.revokes.contains(&first.tag));
        let state = assert_permutation_invariant(&[first, kicked], |order| {
            roster_without_target(order, OWNER)
        });
        assert!(!has_target(&state));
        assert_eq!(state.kicks, vec![who(TARGET)]);
    }

    #[test]
    fn rejoin_after_kick_survives_in_every_order() {
        let first = join(10);
        let kicked = kick(1, std::slice::from_ref(&first));
        let rejoin = join(20);
        let state = assert_permutation_invariant(&[first, kicked, rejoin], |order| {
            roster_without_target(order, OWNER)
        });
        assert!(has_target(&state));
        assert_eq!(state.kicks, vec![who(TARGET)]);
    }

    #[test]
    fn rejoin_concurrent_with_kick_survives_and_an_observing_kick_removes() {
        let first = join(10);
        // The kicker never saw the concurrent rejoin.
        let concurrent = join(11);
        let kicked = kick(1, std::slice::from_ref(&first));
        let events = [first.clone(), concurrent.clone(), kicked.clone()];
        let state =
            assert_permutation_invariant(&events, |order| roster_without_target(order, OWNER));
        assert!(has_target(&state));

        // A second kick that observed both episodes ends the membership.
        let again = kick(1, &[first.clone(), concurrent.clone(), kicked.clone()]);
        let state = assert_permutation_invariant(&[first, concurrent, kicked, again], |order| {
            roster_without_target(order, OWNER)
        });
        assert!(!has_target(&state));
    }

    #[test]
    fn reinstalled_member_pulling_old_kick_stays_after_rejoin() {
        // The kicked member reinstalls (empty journal and roster), rejoins,
        // and home-context sync then serves the old join and kick.
        let old_join = join(10);
        let old_kick = kick(1, std::slice::from_ref(&old_join));
        let rejoin = join(30);
        for viewer in [TARGET, OWNER] {
            let state = assert_permutation_invariant(
                &[old_join.clone(), old_kick.clone(), rejoin.clone()],
                |order| roster_without_target(order, viewer),
            );
            assert!(has_target(&state), "viewer {viewer}");
        }
    }

    #[test]
    fn leave_ends_the_observed_episode_and_rejoin_survives() {
        let first = join(10);
        let leave = tagged(HomeGovernanceEvent::MemberLeft(SocialFact::member_left_ms(
            who(TARGET),
            aura_social::HomeId::from_bytes(*home_id().as_bytes()),
            ctx(),
            15,
            causal(
                3,
                HomeGovernanceKey::Leave {
                    target: who(TARGET),
                },
                std::slice::from_ref(&first),
            ),
        )));
        let left = assert_permutation_invariant(&[first.clone(), leave.clone()], |order| {
            roster_without_target(order, OWNER)
        });
        assert!(!has_target(&left));
        let rejoined = assert_permutation_invariant(&[first, leave, join(20)], |order| {
            roster_without_target(order, OWNER)
        });
        assert!(has_target(&rejoined));
    }
}

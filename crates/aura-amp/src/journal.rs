//! AMP journal effects and context journal operations.
//!
//! This module provides the `AmpJournalEffects` trait adapter that bridges
//! Layer 4 AMP operations to Layer 2 journal facts. It handles:
//! - Fetching and building context-scoped fact journals
//! - Inserting relational facts (checkpoints, bumps, policies)
//! - Channel state reduction via journal queries

use crate::{ChannelMembershipFact, ChannelParticipantEvent};
use aura_core::effects::{JournalEffects, OrderClockEffects};
use aura_core::hash::hash;
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::{AuraError, FactValue, Journal, Result};
use aura_journal::{
    fact::{Fact, FactContent, JournalNamespace, RelationalFact},
    reduce_context, ChannelEpochState, DomainFact, FactJournal, ProtocolRelationalFact,
};

/// A successful canonical reduction contained no checkpoint for one channel.
/// Construction remains private to this journal reader. This error identifies
/// a failed read result; it is not permission to mutate canonical state.
/// ```compile_fail
/// use aura_amp::ChannelStateUnavailable;
/// use aura_core::{ContextId, ChannelId};
/// let _ = ChannelStateUnavailable {
///     context: ContextId::new_from_entropy([1; 32]),
///     channel: ChannelId::from_bytes([2; 32]),
/// };
/// ```
#[derive(Debug, Clone, thiserror::Error)]
#[error("channel state not found")]
pub struct ChannelStateUnavailable {
    context: ContextId,
    channel: ChannelId,
}

impl ChannelStateUnavailable {
    pub fn context(&self) -> ContextId {
        self.context
    }
    pub fn channel(&self) -> ChannelId {
        self.channel
    }

    pub fn find<'a>(error: &'a (dyn std::error::Error + 'static)) -> Option<&'a Self> {
        let mut cause = Some(error);
        while let Some(error) = cause {
            if let Some(absence) = error.downcast_ref::<Self>() {
                return Some(absence);
            }
            cause = error.source();
        }
        None
    }
}

// ============================================================================
// AmpJournalEffects Trait
// ============================================================================

/// Protocol-layer journal adapter for AMP.
///
/// This trait extends `JournalEffects` and `OrderClockEffects` to provide
/// AMP-specific operations for managing context journals and relational facts.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait AmpJournalEffects: JournalEffects + OrderClockEffects + Sized {
    /// Fetch the full context journal (fact-based) for reduction.
    async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal>;

    /// Insert a relational fact (AMP checkpoint/bump/policy/evidence).
    async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()>;

    /// Scoped context store wrapper to avoid leaking storage keys.
    fn context_store(&self) -> AmpContextStore<'_, Self>
    where
        Self: Sized,
    {
        AmpContextStore { effects: self }
    }
}

/// Blanket implementation of `AmpJournalEffects` for any type implementing
/// `JournalEffects + OrderClockEffects`.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl<E: JournalEffects + OrderClockEffects> AmpJournalEffects for E {
    async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        let journal = self.get_journal().await?;
        let contents = extract_fact_contents(&journal);
        Ok(build_context_journal(context, contents))
    }

    async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()> {
        insert_context_relational_fact(self, fact).await
    }
}

// ============================================================================
// AmpContextStore
// ============================================================================

/// Focused context journal helper that hides storage keys/serialization.
///
/// This provides a scoped view into the journal for a specific context,
/// avoiding direct manipulation of storage keys.
pub struct AmpContextStore<'a, E: ?Sized + JournalEffects + OrderClockEffects> {
    effects: &'a E,
}

impl<'a, E: ?Sized + JournalEffects + OrderClockEffects> AmpContextStore<'a, E> {
    /// Fetch the context journal for reduction.
    pub async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        let journal = self.effects.get_journal().await?;
        let contents = extract_fact_contents(&journal);
        Ok(build_context_journal(context, contents))
    }

    /// Insert a relational fact into the journal.
    pub async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()> {
        insert_context_relational_fact(self.effects, fact).await
    }
}

// ============================================================================
// Channel State Reduction
// ============================================================================

/// Every channel in this journal that has bootstrap key material, as
/// `(context, channel, bootstrap_id)`. The keys themselves stay in secure storage.
pub async fn list_channel_bootstraps<A: AmpJournalEffects>(
    effects: &A,
) -> Result<Vec<(ContextId, ChannelId, aura_core::Hash32)>> {
    let journal = effects.get_journal().await?;
    let contents = extract_fact_contents(&journal);
    let mut contexts: Vec<ContextId> = contents
        .iter()
        .filter_map(|(_, content)| match content {
            FactContent::Relational(fact) => Some(fact.context_id()),
            _ => None,
        })
        .collect();
    contexts.sort();
    contexts.dedup();
    let mut bootstraps = Vec::new();
    for context in contexts {
        let state =
            reduce_context(&build_context_journal(context, contents.clone())).map_err(|error| {
                AuraError::Internal {
                    message: format!("context reduction failed: {error}"),
                    source: Some(std::sync::Arc::new(error)),
                }
            })?;
        for (channel, epoch_state) in state.channel_epochs {
            if let Some(bootstrap) = epoch_state.bootstrap {
                bootstraps.push((context, channel, bootstrap.bootstrap_id));
            }
        }
    }
    Ok(bootstraps)
}

/// Reduce to AMP channel state for a (context, channel) pair.
///
/// This fetches the context journal and reduces it to extract the current
/// epoch state for the specified channel.
pub async fn get_channel_state<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
) -> Result<ChannelEpochState> {
    let journal = effects.fetch_context_journal(context).await?;
    let state = reduce_context(&journal).map_err(|error| AuraError::Internal {
        message: format!("context reduction failed: {error}"),
        source: Some(std::sync::Arc::new(error)),
    })?;
    state
        .channel_epochs
        .get(&channel)
        .filter(|state| {
            state
                .canonical_checkpoint
                .as_ref()
                .is_some_and(|checkpoint| {
                    checkpoint.context == context
                        && checkpoint.channel == channel
                        && checkpoint.chan_epoch <= state.chan_epoch
                })
        })
        .cloned()
        .ok_or_else(|| AuraError::NotFound {
            message: "channel state not found".to_owned(),
            source: Some(std::sync::Arc::new(ChannelStateUnavailable {
                context,
                channel,
            })),
        })
}

/// Observed staging state, including policies/bootstrap/transitions before a checkpoint.
/// This read does not establish authoritative channel materialization.
pub async fn get_reduced_channel_state<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
) -> Result<ChannelEpochState> {
    let journal = effects.fetch_context_journal(context).await?;
    let state = reduce_context(&journal).map_err(|error| AuraError::Internal {
        message: format!("context reduction failed: {error}"),
        source: Some(std::sync::Arc::new(error)),
    })?;
    state
        .channel_epochs
        .get(&channel)
        .cloned()
        .ok_or_else(|| AuraError::not_found("channel has no reduced AMP state"))
}

/// Reduce the current AMP channel participants for a `(context, channel)` pair.
pub async fn list_channel_participants<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
) -> Result<Vec<AuthorityId>> {
    let _canonical = get_channel_state(effects, context, channel).await?;
    let journal = effects.fetch_context_journal(context).await?;
    Ok(reduce_membership(&journal, context, channel)
        .participants()
        .collect())
}

/// Membership observations reduced from the context journal, for a runtime
/// ingress check of an incoming membership event's author standing.
pub async fn channel_membership_observations<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
) -> Result<crate::channel::ChannelMembershipObservations> {
    let journal = effects.fetch_context_journal(context).await?;
    Ok(reduce_membership(&journal, context, channel))
}

/// Membership fact recording `event` for `participant`, from the observed
/// episodes. A join names its episode (the accepted invitation id, or `None`
/// for the unnamed episode) and is refused when it would not leave the
/// participant a member, i.e. it names an episode a departure already ended
/// (an unnamed rejoin, or a replayed invitation). Opaque order tokens never
/// order a join after a departure. A departure ends every observed episode.
pub async fn channel_membership_event<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
    event: ChannelParticipantEvent,
    episode: Option<String>,
) -> Result<ChannelMembershipFact> {
    let _canonical = get_channel_state(effects, context, channel).await?;
    let journal = effects.fetch_context_journal(context).await?;
    let observed = reduce_membership(&journal, context, channel);
    match event {
        ChannelParticipantEvent::Joined => {
            if observed.episode_ended(participant, episode.as_deref())
                && !observed.contains(participant)
            {
                return Err(AuraError::Invalid {
                    message: "an ended membership episode cannot authorize rejoin".into(),
                    source: Some(std::sync::Arc::new(
                        aura_core::effects::amp::AmpChannelError::RejoinRequiresMembershipEvidence {
                            context,
                            channel,
                            participant,
                        },
                    )),
                });
            }
            let timestamp = ChannelMembershipFact::random_timestamp(effects).await?;
            Ok(match episode {
                Some(episode) => ChannelMembershipFact::joined_episode(
                    context,
                    channel,
                    participant,
                    episode,
                    timestamp,
                ),
                None => ChannelMembershipFact::new(context, channel, participant, event, timestamp),
            })
        }
        ChannelParticipantEvent::Left => Ok(ChannelMembershipFact::departure(
            &observed,
            participant,
            ChannelMembershipFact::random_timestamp(effects).await?,
        )),
    }
}

/// Reject an observed departed or foreign sender without treating an empty
/// post-departure set as permission. Absence of membership observations retains
/// the separate bootstrap/epoch authorization contract, and grants no identity.
pub async fn sender_allowed_by_channel_membership<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
    sender: AuthorityId,
) -> Result<bool> {
    let journal = effects.fetch_context_journal(context).await?;
    let observations = reduce_membership(&journal, context, channel);
    Ok(!observations.has_observations() || observations.contains(sender))
}

fn reduce_membership(
    journal: &FactJournal,
    context: ContextId,
    channel: ChannelId,
) -> crate::channel::ChannelMembershipObservations {
    let mut observations = crate::channel::ChannelMembershipObservations::new(context, channel);

    for fact in journal.iter_facts() {
        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
        else {
            continue;
        };
        let Some(membership) = ChannelMembershipFact::from_envelope(envelope) else {
            continue;
        };
        observations.observe(&membership);
    }

    observations
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Extract the context ID from a relational fact.
pub(crate) fn fact_context(fact: &RelationalFact) -> Result<ContextId> {
    match fact {
        RelationalFact::Protocol(ProtocolRelationalFact::AmpChannelCheckpoint(cp)) => {
            Ok(cp.context)
        }
        RelationalFact::Protocol(ProtocolRelationalFact::AmpProposedChannelEpochBump(b)) => {
            Ok(b.context)
        }
        RelationalFact::Protocol(ProtocolRelationalFact::AmpCommittedChannelEpochBump(b)) => {
            Ok(b.context)
        }
        RelationalFact::Protocol(ProtocolRelationalFact::AmpChannelPolicy(p)) => Ok(p.context),
        RelationalFact::Protocol(ProtocolRelationalFact::AmpChannelBootstrap(b)) => Ok(b.context),
        RelationalFact::Generic {
            context_id,
            envelope,
        } if envelope.type_id.as_str().starts_with("amp-") => Ok(*context_id),
        _ => Err(AuraError::invalid("fact not AMP-context scoped")),
    }
}

async fn insert_context_relational_fact<E: ?Sized + JournalEffects + OrderClockEffects>(
    effects: &E,
    fact: RelationalFact,
) -> Result<()> {
    let context = fact_context(&fact)?;
    let order = effects
        .order_time()
        .await
        .map_err(|e| AuraError::internal(e.to_string()))?;
    let content = FactContent::Relational(fact);
    let bytes =
        serde_json::to_vec(&content).map_err(|e| AuraError::serialization(e.to_string()))?;
    let key = format!("relational:{}:{}", context, hex::encode(order.0));

    let mut delta = Journal::new();
    delta.facts.insert(key, FactValue::Bytes(bytes))?;

    let current = effects.get_journal().await?;
    let merged = effects.merge_facts(current, delta).await?;
    effects.persist_journal(&merged).await?;
    Ok(())
}

/// Extract fact contents from a core journal.
fn extract_fact_contents(journal: &Journal) -> Vec<(Option<OrderTime>, FactContent)> {
    journal
        .read_facts()
        .iter()
        .filter_map(|(key, value)| {
            let content = match value {
                FactValue::Bytes(bytes) => serde_json::from_slice(bytes).ok(),
                FactValue::String(text) => serde_json::from_str(text).ok(),
                FactValue::Nested(nested) => serde_json::to_vec(nested)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
                _ => None,
            };
            content.map(|content| (parse_order_from_key(key.as_str()), content))
        })
        .collect()
}

/// Parse an order time from a journal key suffix.
fn parse_order_from_key(key: &str) -> Option<OrderTime> {
    let suffix = key.rsplit(':').next()?;
    let bytes = hex::decode(suffix).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut order = [0u8; 32];
    order.copy_from_slice(&bytes);
    Some(OrderTime(order))
}

/// Build a context-scoped fact journal from extracted contents.
fn build_context_journal(
    context: ContextId,
    contents: Vec<(Option<OrderTime>, FactContent)>,
) -> FactJournal {
    let mut facts = std::collections::BTreeSet::new();

    for (order_hint, content) in contents {
        if let FactContent::Relational(ref relational) = content {
            if fact_context(relational).ok() != Some(context) {
                continue;
            }

            let bytes = serde_json::to_vec(&content).unwrap_or_default();
            let order = order_hint.unwrap_or_else(|| OrderTime(hash(&bytes)));
            let timestamp = TimeStamp::OrderClock(order.clone());
            facts.insert(Fact::new(order, timestamp, content));
        }
    }

    FactJournal {
        namespace: JournalNamespace::Context(context),
        facts,
    }
}

#[cfg(test)]
mod schema_one_membership_tests {
    use super::*;
    use crate::ChannelParticipantEvent;

    #[test]
    fn departure_wins_reversed_tokens_insertion_and_original_journal_merges() {
        let context =
            ContextId::new_from_entropy(hash(b"aura-amp.schema-one-order-inversion.context"));
        let channel = ChannelId::from_bytes(hash(b"aura-amp.schema-one-order-inversion.channel"));
        let participant =
            AuthorityId::new_from_entropy(hash(b"aura-amp.schema-one-order-inversion.participant"));
        let retained = AuthorityId::new_from_entropy(hash(
            b"aura-amp.schema-one-order-inversion.retained-participant",
        ));
        for (join_token, leave_token) in [([255; 32], [0; 32]), ([0; 32], [255; 32])] {
            let join = ChannelMembershipFact::new(
                context,
                channel,
                participant,
                ChannelParticipantEvent::Joined,
                TimeStamp::OrderClock(OrderTime(join_token)),
            );
            let leave = ChannelMembershipFact::new(
                context,
                channel,
                participant,
                ChannelParticipantEvent::Left,
                TimeStamp::OrderClock(OrderTime(leave_token)),
            );
            let stay = ChannelMembershipFact::new(
                context,
                channel,
                retained,
                ChannelParticipantEvent::Joined,
                TimeStamp::OrderClock(OrderTime([127; 32])),
            );
            let make = |items: &[(&ChannelMembershipFact, [u8; 32])]| {
                let mut journal = Journal::new();
                for (membership, token) in items {
                    let content = FactContent::Relational(membership.to_generic());
                    let bytes = match serde_json::to_vec(&content) {
                        Ok(bytes) => bytes,
                        Err(source) => panic!("encode actual original membership: {source}"),
                    };
                    if let Err(source) = journal.facts.insert(
                        format!("relational:{context}:{}", hex::encode(token)),
                        FactValue::Bytes(bytes),
                    ) {
                        panic!("retain original membership: {source}");
                    }
                }
                journal
            };
            let forward = make(&[
                (&join, join_token),
                (&leave, leave_token),
                (&stay, [127; 32]),
            ]);
            let reverse = make(&[
                (&stay, [127; 32]),
                (&leave, leave_token),
                (&join, join_token),
            ]);
            let joined = make(&[(&join, join_token), (&stay, [127; 32])]);
            let departed = make(&[(&leave, leave_token)]);
            let mut join_merge_leave = joined.clone();
            join_merge_leave.merge(&departed);
            let mut leave_merge_join = departed;
            leave_merge_join.merge(&joined);
            for original in [forward, reverse, join_merge_leave, leave_merge_join] {
                let facts = build_context_journal(context, extract_fact_contents(&original));
                let observed = reduce_membership(&facts, context, channel);
                assert_eq!(observed.participants().collect::<Vec<_>>(), vec![retained]);
                assert!(observed.departed(participant));
                let mut attempted_rejoin = observed;
                assert!(attempted_rejoin.observe(&join));
                assert!(
                    !attempted_rejoin.contains(participant),
                    "schema one has no successor generation witness"
                );
                let foreign = ChannelMembershipFact::new(
                    context,
                    ChannelId::from_bytes(hash(
                        b"aura-amp.schema-one-order-inversion.foreign-channel",
                    )),
                    participant,
                    ChannelParticipantEvent::Joined,
                    TimeStamp::OrderClock(OrderTime([19; 32])),
                );
                assert!(!attempted_rejoin.observe(&foreign));
                let foreign_context = ChannelMembershipFact::new(
                    ContextId::new_from_entropy(hash(
                        b"aura-amp.schema-one-order-inversion.foreign-context",
                    )),
                    channel,
                    participant,
                    ChannelParticipantEvent::Joined,
                    TimeStamp::OrderClock(OrderTime([20; 32])),
                );
                assert!(!attempted_rejoin.observe(&foreign_context));
                assert!(!attempted_rejoin.contains(participant));
            }
        }
    }

    /// An event written for someone else counts only once its author has
    /// standing (an admitted member, e.g. the inviter), in every observation
    /// order; a stranger's join or departure for someone else never counts.
    #[test]
    fn membership_written_for_another_needs_author_standing_in_every_order() {
        let context = ContextId::new_from_entropy(hash(b"aura-amp.standing.context"));
        let channel = ChannelId::from_bytes(hash(b"aura-amp.standing.channel"));
        let inviter = AuthorityId::new_from_entropy(hash(b"aura-amp.standing.inviter"));
        let acceptor = AuthorityId::new_from_entropy(hash(b"aura-amp.standing.acceptor"));
        let stranger = AuthorityId::new_from_entropy(hash(b"aura-amp.standing.stranger"));
        let victim = AuthorityId::new_from_entropy(hash(b"aura-amp.standing.victim"));
        let token = |byte| TimeStamp::OrderClock(OrderTime([byte; 32]));
        let inviter_join = ChannelMembershipFact::new(
            context,
            channel,
            inviter,
            ChannelParticipantEvent::Joined,
            token(1),
        );
        let victim_join = ChannelMembershipFact::new(
            context,
            channel,
            victim,
            ChannelParticipantEvent::Joined,
            token(2),
        );
        let invited_join = ChannelMembershipFact::joined_episode(
            context,
            channel,
            acceptor,
            "inv-1".into(),
            token(3),
        )
        .authored_by(inviter);
        let forged_join = ChannelMembershipFact::new(
            context,
            channel,
            stranger,
            ChannelParticipantEvent::Joined,
            token(4),
        )
        .authored_by(victim)
        .authored_by(stranger);
        let forged_join_for_other = ChannelMembershipFact::joined_episode(
            context,
            channel,
            AuthorityId::new_from_entropy(hash(b"aura-amp.standing.friend")),
            "inv-2".into(),
            token(5),
        )
        .authored_by(stranger);
        let forged_kick = ChannelMembershipFact::new(
            context,
            channel,
            victim,
            ChannelParticipantEvent::Left,
            token(6),
        )
        .authored_by(stranger);
        let facts = [
            &inviter_join,
            &victim_join,
            &invited_join,
            &forged_join_for_other,
            &forged_kick,
        ];
        for rotation in 0..facts.len() {
            for reverse in [false, true] {
                let mut order: Vec<_> = facts
                    .iter()
                    .cycle()
                    .skip(rotation)
                    .take(facts.len())
                    .collect();
                if reverse {
                    order.reverse();
                }
                let mut observed = crate::ChannelMembershipObservations::new(context, channel);
                for fact in order {
                    assert!(observed.observe(fact));
                }
                assert!(observed.contains(acceptor), "inviter-written join counts");
                assert!(observed.contains(victim), "a stranger cannot kick");
                assert!(!observed.departed(victim));
                assert_eq!(
                    observed.participants().collect::<Vec<_>>().len(),
                    3,
                    "a stranger cannot add a friend"
                );
            }
        }
        // A self-written join needs no standing.
        let mut observed = crate::ChannelMembershipObservations::new(context, channel);
        assert!(observed.observe(&forged_join));
        assert!(observed.contains(stranger));
    }

    /// A departure ends the episodes its writer observed; a fresh invitation
    /// episode re-admits the member in any observation order, while a
    /// replayed episode cannot.
    #[test]
    fn fresh_episode_readmits_departed_member_and_replay_does_not() {
        let context = ContextId::new_from_entropy(hash(b"aura-amp.episodes.context"));
        let channel = ChannelId::from_bytes(hash(b"aura-amp.episodes.channel"));
        let member = AuthorityId::new_from_entropy(hash(b"aura-amp.episodes.member"));
        let token = |byte| TimeStamp::OrderClock(OrderTime([byte; 32]));
        let first = ChannelMembershipFact::joined_episode(
            context,
            channel,
            member,
            "inv-first".into(),
            token(200),
        );
        let mut writer = crate::ChannelMembershipObservations::new(context, channel);
        assert!(writer.observe(&first));
        let kick = ChannelMembershipFact::departure(&writer, member, token(100));
        let second = ChannelMembershipFact::joined_episode(
            context,
            channel,
            member,
            "inv-second".into(),
            token(1),
        );
        let observe = |facts: &[&ChannelMembershipFact]| {
            let mut observed = crate::ChannelMembershipObservations::new(context, channel);
            for fact in facts {
                assert!(observed.observe(fact));
            }
            observed
        };
        for order in [
            [&first, &kick, &second],
            [&second, &kick, &first],
            [&kick, &second, &first],
        ] {
            let observed = observe(&order);
            assert!(observed.contains(member), "fresh episode re-admits");
            assert!(!observed.departed(member));
            assert!(observed.episode_ended(member, Some("inv-first")));
            assert!(!observed.episode_ended(member, Some("inv-second")));
        }
        let replay = observe(&[&first, &kick, &first]);
        assert!(replay.departed(member), "a replayed episode stays ended");
        let unnamed = ChannelMembershipFact::new(
            context,
            channel,
            member,
            ChannelParticipantEvent::Joined,
            token(250),
        );
        assert!(observe(&[&first, &kick, &unnamed]).departed(member));
        let leave =
            ChannelMembershipFact::departure(&observe(&[&first, &kick, &second]), member, token(3));
        assert!(observe(&[&first, &kick, &second, &leave]).departed(member));
    }
}

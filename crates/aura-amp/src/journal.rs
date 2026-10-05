//! AMP journal effects and context journal operations.
//!
//! This module provides the `AmpJournalEffects` trait adapter that bridges
//! Layer 4 AMP operations to Layer 2 journal facts. It handles:
//! - Fetching and building context-scoped fact journals
//! - Inserting relational facts (checkpoints, bumps, policies)
//! - Channel state reduction via journal queries

use crate::ChannelMembershipFact;
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

/// Whether original schema-one departure evidence prevents an unversioned rejoin.
/// Opaque order tokens cannot prove that a join supersedes a departure.
pub async fn channel_participant_departed<A: AmpJournalEffects>(
    effects: &A,
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
) -> Result<bool> {
    let _canonical = get_channel_state(effects, context, channel).await?;
    let journal = effects.fetch_context_journal(context).await?;
    Ok(reduce_membership(&journal, context, channel).departed(participant))
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
) -> crate::channel::SchemaOneChannelMembership {
    let mut observations = crate::channel::SchemaOneChannelMembership::new(context, channel);

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
}

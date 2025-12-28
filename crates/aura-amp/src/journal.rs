//! AMP journal effects and context journal operations.
//!
//! This module provides the `AmpJournalEffects` trait adapter that bridges
//! Layer 4 AMP operations to Layer 2 journal facts. It handles:
//! - Fetching and building context-scoped fact journals
//! - Inserting relational facts (checkpoints, bumps, policies)
//! - Channel state reduction via journal queries

use aura_core::effects::{JournalEffects, OrderClockEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::{AuraError, FactValue, Journal, Result};
use aura_journal::{
    fact::{Fact, FactContent, JournalNamespace, RelationalFact},
    reduce_context, ChannelEpochState, FactJournal, ProtocolRelationalFact,
};

// ============================================================================
// AmpJournalEffects Trait
// ============================================================================

/// Protocol-layer journal adapter for AMP.
///
/// This trait extends `JournalEffects` and `OrderClockEffects` to provide
/// AMP-specific operations for managing context journals and relational facts.
#[async_trait::async_trait]
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
#[async_trait::async_trait]
impl<E: JournalEffects + OrderClockEffects> AmpJournalEffects for E {
    async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        let journal = self.get_journal().await?;
        let contents = extract_fact_contents(&journal);
        Ok(build_context_journal(context, contents))
    }

    async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()> {
        let context = fact_context(&fact)?;
        let order = self
            .order_time()
            .await
            .map_err(|e| AuraError::internal(e.to_string()))?;
        let content = FactContent::Relational(fact);
        let bytes =
            serde_json::to_vec(&content).map_err(|e| AuraError::serialization(e.to_string()))?;
        let key = format!("relational:{}:{}", context, hex::encode(order.0));

        let mut delta = Journal::new();
        delta.facts.insert(key, FactValue::Bytes(bytes));

        let merged = self.merge_facts(&self.get_journal().await?, &delta).await?;
        self.persist_journal(&merged).await?;
        Ok(())
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
        let context = fact_context(&fact)?;
        let order = self
            .effects
            .order_time()
            .await
            .map_err(|e| AuraError::internal(e.to_string()))?;
        let content = FactContent::Relational(fact);
        let bytes =
            serde_json::to_vec(&content).map_err(|e| AuraError::serialization(e.to_string()))?;
        let key = format!("relational:{}:{}", context, hex::encode(order.0));

        let mut delta = Journal::new();
        delta.facts.insert(key, FactValue::Bytes(bytes));

        let merged = self
            .effects
            .merge_facts(&self.effects.get_journal().await?, &delta)
            .await?;
        self.effects.persist_journal(&merged).await?;
        Ok(())
    }
}

// ============================================================================
// Channel State Reduction
// ============================================================================

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
    let state = reduce_context(&journal)
        .map_err(|e| AuraError::internal(format!("context reduction failed: {e}")))?;
    state
        .channel_epochs
        .get(&channel)
        .cloned()
        .ok_or_else(|| AuraError::not_found("channel state not found"))
}

// ============================================================================
// Internal Helpers
// ============================================================================

/// Extract the context ID from a relational fact.
pub(crate) fn fact_context(fact: &RelationalFact) -> Result<ContextId> {
    match fact {
        RelationalFact::Protocol(ProtocolRelationalFact::AmpChannelCheckpoint(cp)) => Ok(cp.context),
        RelationalFact::Protocol(ProtocolRelationalFact::AmpProposedChannelEpochBump(b)) => {
            Ok(b.context)
        }
        RelationalFact::Protocol(ProtocolRelationalFact::AmpCommittedChannelEpochBump(b)) => {
            Ok(b.context)
        }
        RelationalFact::Protocol(ProtocolRelationalFact::AmpChannelPolicy(p)) => Ok(p.context),
        RelationalFact::Generic {
            context_id,
            binding_type,
            ..
        } if binding_type.starts_with("amp-") => Ok(*context_id),
        _ => Err(AuraError::invalid("fact not AMP-context scoped")),
    }
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
            content.map(|content| (parse_order_from_key(key), content))
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
            facts.insert(Fact {
                order,
                timestamp,
                content,
            });
        }
    }

    FactJournal {
        namespace: JournalNamespace::Context(context),
        facts,
    }
}

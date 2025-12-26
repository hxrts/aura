#![deny(clippy::dbg_macro)]
#![deny(clippy::todo)]
#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods,
    deprecated
)]
//! # Aura AMP - Layer 4: Authenticated Messaging Protocol
//!
//! This crate provides the complete AMP implementation including:
//! - Journal adapters and reduction helpers
//! - Channel lifecycle management
//! - Transport protocol (send/recv)
//! - Telemetry and observability
//! - Consensus integration for epoch bumps
//! - Choreography annotations for MPST integration
//!
//! These glue Layer 4 orchestration to Layer 2 facts without leaking domain types
//! outward. Backed by core `JournalEffects` and storage effects.

// Submodules
pub mod channel;
pub mod choreography;
pub mod config;
pub mod consensus;
pub mod core;
pub mod prelude;
pub mod transport;
pub mod wire;

// Core dependencies
use aura_consensus::ConsensusId;
use aura_core::effects::{JournalEffects, OrderClockEffects, StorageEffects};
use aura_core::hash::hash;
use aura_core::identifiers::AuthorityId;
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::{AuraError, FactValue, Journal, Result};
use aura_journal::{
    fact::{Fact, FactContent, JournalNamespace, RelationalFact},
    reduce_context, ChannelEpochState, FactJournal,
};
use serde::{Deserialize, Serialize};

// Re-export channel types
pub use channel::{AmpChannelCoordinator, ChannelMembershipFact, ChannelParticipantEvent};

// Re-export transport types
pub use transport::{
    amp_recv, amp_recv_with_receipt, amp_send, commit_bump_with_consensus, emit_proposed_bump,
    prepare_send, validate_header, AmpDelivery, AmpReceipt,
};

// Re-export wire types
pub use wire::{
    deserialize_message as deserialize_amp_message, serialize_message as serialize_amp_message,
    AmpMessage,
};

// Re-export telemetry
pub use transport::{AmpTelemetry, WindowValidationResult, AMP_TELEMETRY};

// Re-export consensus functions
pub use consensus::{
    finalize_amp_bump_with_journal, finalize_amp_bump_with_journal_default,
    run_amp_channel_epoch_bump, run_amp_channel_epoch_bump_default,
};

/// Protocol-layer journal adapter for AMP.
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
        let bytes = serde_json::to_vec(&content).map_err(|e| AuraError::serialization(e.to_string()))?;
        let key = format!("relational:{}:{}", context, hex::encode(order.0));

        let mut delta = Journal::new();
        delta.facts.insert(key, FactValue::Bytes(bytes));

        let merged = self.merge_facts(&self.get_journal().await?, &delta).await?;
        self.persist_journal(&merged).await?;
        Ok(())
    }
}

/// Evidence storage for AMP consensus (non-canonical cache).
///
/// Evidence is not required to reconstruct AMP channel state, so we keep it in
/// StorageEffects behind an explicit trait to avoid conflating it with journal facts.
#[async_trait::async_trait]
pub trait AmpEvidenceEffects: StorageEffects + Sized {
    /// Carry evidence deltas keyed by consensus id.
    async fn merge_evidence_delta(&self, cid: ConsensusId, delta: EvidenceDelta) -> Result<()>;

    /// Retrieve accumulated evidence for a consensus id.
    async fn evidence_for(&self, cid: ConsensusId) -> Result<Option<EvidenceRecord>>;

    /// Insert evidence delta tracking witness participation in consensus.
    async fn insert_evidence_delta(
        &self,
        witness: AuthorityId,
        consensus_id: ConsensusId,
        context: ContextId,
    ) -> Result<()>;

    /// Scoped evidence store wrapper to keep evidence handling separate.
    fn evidence_store(&self) -> AmpEvidenceStore<'_, Self>
    where
        Self: Sized,
    {
        AmpEvidenceStore { effects: self }
    }
}

#[async_trait::async_trait]
impl<E: StorageEffects> AmpEvidenceEffects for E {
    async fn merge_evidence_delta(&self, cid: ConsensusId, delta: EvidenceDelta) -> Result<()> {
        self.evidence_store().merge_delta(cid, delta).await
    }

    async fn evidence_for(&self, cid: ConsensusId) -> Result<Option<EvidenceRecord>> {
        self.evidence_store().current(cid).await
    }

    async fn insert_evidence_delta(
        &self,
        witness: AuthorityId,
        consensus_id: ConsensusId,
        context: ContextId,
    ) -> Result<()> {
        // Create evidence delta recording witness participation
        let evidence_entry = format!("witness:{}:context:{}", witness, context);
        let mut delta = EvidenceDelta::default();
        delta
            .entries
            .insert(hex::encode(consensus_id.0 .0), evidence_entry.into_bytes());

        self.merge_evidence_delta(consensus_id, delta).await
    }
}

/// Reduce to AMP channel state for a (context, channel).
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

/// Minimal evidence CRDT for AMP consensus.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceRecord {
    /// Map of consensus id -> collected evidence bytes (opaque to AMP)
    pub entries: std::collections::BTreeMap<String, Vec<u8>>,
}

impl EvidenceRecord {
    pub fn merge(&mut self, delta: EvidenceDelta) {
        for (cid, bytes) in delta.entries {
            self.entries.insert(cid, bytes);
        }
    }
}

/// Delta type for evidence propagation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceDelta {
    pub entries: std::collections::BTreeMap<String, Vec<u8>>,
}

/// Storage key namespace for AMP evidence records.
/// All AMP evidence keys are prefixed with this to avoid collisions with
/// journal facts (which use `relational:` prefix) or other storage users.
pub const AMP_EVIDENCE_KEY_PREFIX: &str = "amp/evidence/";

fn evidence_key(cid: ConsensusId) -> String {
    format!("{}{}", AMP_EVIDENCE_KEY_PREFIX, hex::encode(cid.0 .0))
}

fn fact_context(fact: &RelationalFact) -> Result<ContextId> {
    match fact {
        RelationalFact::AmpChannelCheckpoint(cp) => Ok(cp.context),
        RelationalFact::AmpProposedChannelEpochBump(b) => Ok(b.context),
        RelationalFact::AmpCommittedChannelEpochBump(b) => Ok(b.context),
        RelationalFact::AmpChannelPolicy(p) => Ok(p.context),
        RelationalFact::Generic {
            context_id,
            binding_type,
            ..
        } if binding_type.starts_with("amp-") => Ok(*context_id),
        _ => Err(AuraError::invalid("fact not AMP-context scoped")),
    }
}

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

/// Focused context journal helper that hides storage keys/serialization.
pub struct AmpContextStore<
    'a,
    E: ?Sized + JournalEffects + OrderClockEffects,
> {
    effects: &'a E,
}

impl<'a, E: ?Sized + JournalEffects + OrderClockEffects> AmpContextStore<'a, E> {
    pub async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        let journal = self.effects.get_journal().await?;
        let contents = extract_fact_contents(&journal);
        Ok(build_context_journal(context, contents))
    }

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

/// Evidence store separated from context journal to avoid accidental coupling.
pub struct AmpEvidenceStore<'a, E: ?Sized + StorageEffects> {
    effects: &'a E,
}

impl<'a, E: ?Sized + StorageEffects> AmpEvidenceStore<'a, E> {
    pub async fn merge_delta(&self, cid: ConsensusId, delta: EvidenceDelta) -> Result<()> {
        let mut record = match self.current(cid).await? {
            Some(record) => record,
            None => EvidenceRecord::default(),
        };
        record.merge(delta);
        let bytes =
            serde_json::to_vec(&record).map_err(|e| AuraError::serialization(e.to_string()))?;
        self.effects
            .store(&evidence_key(cid), bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))
    }

    pub async fn current(&self, cid: ConsensusId) -> Result<Option<EvidenceRecord>> {
        match self.effects.retrieve(&evidence_key(cid)).await {
            Ok(Some(bytes)) => {
                let record: EvidenceRecord = serde_json::from_slice(&bytes)
                    .map_err(|e| AuraError::serialization(e.to_string()))?;
                Ok(Some(record))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::storage(e.to_string())),
        }
    }
}

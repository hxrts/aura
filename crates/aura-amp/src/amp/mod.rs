//! AMP (Aura Messaging Protocol) - Consolidated Protocol Layer
//!
//! This module provides the complete AMP implementation including:
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
pub mod consensus;
pub mod transport;

// Core dependencies
use aura_consensus::ConsensusId;
use aura_core::effects::JournalEffects;
use aura_core::effects::StorageEffects;
use aura_core::hash::hash;
use aura_core::identifiers::AuthorityId;
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_core::{AuraError, Result};
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
    prepare_send, validate_header, AmpDelivery, AmpMessage, AmpReceipt,
};

// Re-export telemetry
pub use transport::{
    AmpFlowTelemetry, AmpMetrics, AmpProtocolStats, AmpReceiveTelemetry, AmpSendTelemetry,
    AmpTelemetry, WindowValidationResult, AMP_TELEMETRY,
};

// Re-export consensus functions
pub use consensus::{
    finalize_amp_bump_with_journal, finalize_amp_bump_with_journal_default,
    run_amp_channel_epoch_bump, run_amp_channel_epoch_bump_default,
};

/// Protocol-layer journal adapter for AMP.
#[async_trait::async_trait]
pub trait AmpJournalEffects:
    JournalEffects + StorageEffects + aura_core::effects::RandomEffects + Sized
{
    /// Fetch the full context journal (fact-based) for reduction.
    async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal>;

    /// Insert a relational fact (AMP checkpoint/bump/policy/evidence).
    async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()>;

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

    /// Scoped context store wrapper to avoid leaking storage keys.
    fn context_store(&self) -> AmpContextStore<'_, Self>
    where
        Self: Sized,
    {
        AmpContextStore { effects: self }
    }

    /// Scoped evidence store wrapper to keep evidence handling separate.
    fn evidence_store(&self) -> AmpEvidenceStore<'_, Self>
    where
        Self: Sized,
    {
        AmpEvidenceStore { effects: self }
    }
}

#[async_trait::async_trait]
impl<E: JournalEffects + StorageEffects + aura_core::effects::RandomEffects> AmpJournalEffects
    for E
{
    async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        match self.retrieve(&context_journal_key(context)).await {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                .map_err(|e| AuraError::serialization(format!("decode AMP journal: {}", e))),
            Ok(None) => Ok(FactJournal {
                namespace: JournalNamespace::Context(context),
                facts: std::collections::BTreeSet::new(),
            }),
            Err(e) => Err(AuraError::storage(e.to_string())),
        }
    }

    async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()> {
        let context = fact_context(&fact)?;
        let mut journal = self.fetch_context_journal(context).await?;
        let random_bytes = self.random_bytes(16).await;
        let ts = TimeStamp::OrderClock(OrderTime(hash(&random_bytes)));
        journal
            .add_fact(Fact {
                order: match &ts {
                    TimeStamp::OrderClock(id) => id.clone(),
                    _ => OrderTime([0u8; 32]),
                },
                timestamp: ts,
                content: FactContent::Relational(fact),
            })
            .map_err(|e| AuraError::invalid(format!("failed to add fact: {}", e)))?;

        let bytes =
            serde_json::to_vec(&journal).map_err(|e| AuraError::serialization(e.to_string()))?;
        self.store(&context_journal_key(context), bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))
    }

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

fn evidence_key(cid: ConsensusId) -> String {
    format!("amp/evidence/{}", hex::encode(cid.0 .0))
}

fn context_journal_key(context: ContextId) -> String {
    format!("amp/context/{}", context)
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

/// Focused context journal helper that hides storage keys/serialization.
pub struct AmpContextStore<
    'a,
    E: ?Sized + JournalEffects + StorageEffects + aura_core::effects::RandomEffects,
> {
    effects: &'a E,
}

impl<'a, E: ?Sized + JournalEffects + StorageEffects + aura_core::effects::RandomEffects>
    AmpContextStore<'a, E>
{
    pub async fn fetch_context_journal(&self, context: ContextId) -> Result<FactJournal> {
        match self.effects.retrieve(&context_journal_key(context)).await {
            Ok(Some(bytes)) => serde_json::from_slice(&bytes)
                .map_err(|e| AuraError::serialization(format!("decode AMP journal: {}", e))),
            Ok(None) => Ok(FactJournal {
                namespace: JournalNamespace::Context(context),
                facts: std::collections::BTreeSet::new(),
            }),
            Err(e) => Err(AuraError::storage(e.to_string())),
        }
    }

    pub async fn insert_relational_fact(&self, fact: RelationalFact) -> Result<()> {
        let context = fact_context(&fact)?;
        let mut journal = self.fetch_context_journal(context).await?;
        let random_bytes = self.effects.random_bytes(16).await;
        let ts = TimeStamp::OrderClock(OrderTime(hash(&random_bytes)));
        journal
            .add_fact(Fact {
                order: match &ts {
                    TimeStamp::OrderClock(id) => id.clone(),
                    _ => OrderTime([0u8; 32]),
                },
                timestamp: ts,
                content: FactContent::Relational(fact),
            })
            .map_err(|e| AuraError::invalid(format!("failed to add fact: {}", e)))?;

        let bytes =
            serde_json::to_vec(&journal).map_err(|e| AuraError::serialization(e.to_string()))?;
        self.effects
            .store(&context_journal_key(context), bytes)
            .await
            .map_err(|e| AuraError::storage(e.to_string()))
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

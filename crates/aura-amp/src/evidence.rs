//! AMP evidence storage and consensus evidence effects.
//!
//! This module provides the `AmpEvidenceEffects` trait adapter that bridges
//! Layer 4 AMP operations to non-canonical evidence storage. Evidence is not
//! required to reconstruct AMP channel state, so we keep it in StorageEffects
//! behind an explicit trait to avoid conflating it with journal facts.

use aura_consensus::ConsensusId;
use aura_core::effects::StorageEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ============================================================================
// Evidence Types
// ============================================================================

/// Minimal evidence CRDT for AMP consensus.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceRecord {
    /// Map of consensus id -> collected evidence bytes (opaque to AMP)
    pub entries: BTreeMap<String, Vec<u8>>,
}

impl EvidenceRecord {
    /// Merge a delta into this record (last-write-wins per key).
    pub fn merge(&mut self, delta: EvidenceDelta) {
        for (cid, bytes) in delta.entries {
            self.entries.insert(cid, bytes);
        }
    }
}

/// Delta type for evidence propagation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceDelta {
    pub entries: BTreeMap<String, Vec<u8>>,
}

// ============================================================================
// Evidence Storage Keys
// ============================================================================

/// Storage key namespace for AMP evidence records.
/// All AMP evidence keys are prefixed with this to avoid collisions with
/// journal facts (which use `relational:` prefix) or other storage users.
pub const AMP_EVIDENCE_KEY_PREFIX: &str = "amp/evidence/";

/// Generate a storage key for a consensus id's evidence.
pub(crate) fn evidence_key(cid: ConsensusId) -> String {
    format!("{}{}", AMP_EVIDENCE_KEY_PREFIX, hex::encode(cid.0 .0))
}

// ============================================================================
// AmpEvidenceEffects Trait
// ============================================================================

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

/// Blanket implementation of `AmpEvidenceEffects` for any type implementing `StorageEffects`.
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
        let evidence_entry = format!("witness:{witness}:context:{context}");
        let mut delta = EvidenceDelta::default();
        delta
            .entries
            .insert(hex::encode(consensus_id.0 .0), evidence_entry.into_bytes());

        self.merge_evidence_delta(consensus_id, delta).await
    }
}

// ============================================================================
// AmpEvidenceStore
// ============================================================================

/// Evidence store separated from context journal to avoid accidental coupling.
///
/// This provides a scoped view into storage for evidence records, keeping
/// evidence handling separate from journal fact operations.
pub struct AmpEvidenceStore<'a, E: ?Sized + StorageEffects> {
    effects: &'a E,
}

impl<'a, E: ?Sized + StorageEffects> AmpEvidenceStore<'a, E> {
    /// Merge an evidence delta into the record for a consensus id.
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

    /// Retrieve the current evidence record for a consensus id.
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

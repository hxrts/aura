//! Ceremony Runner (Category C)
//!
//! Provides a shared API surface for ceremony orchestration in the Layer-6
//! runtime. The runner is the orchestration facade; ceremony-specific logic
//! lives in feature crates and emits facts through the journal.

use super::ceremony_tracker::CeremonyTracker;
use aura_app::core::IntentError;
use aura_app::runtime_bridge::CeremonyKind;
use aura_core::ceremony::{SupersessionReason, SupersessionRecord};
use aura_core::domain::status::CeremonyStatus;
use aura_core::identifiers::CeremonyId;
use aura_core::query::ConsensusId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::time::PhysicalTime;
use aura_core::{DeviceId, Hash32};

/// Inputs required to initiate a ceremony.
#[derive(Debug, Clone)]
pub struct CeremonyInitRequest {
    pub ceremony_id: CeremonyId,
    pub kind: CeremonyKind,
    pub threshold_k: u16,
    pub total_n: u16,
    pub participants: Vec<ParticipantIdentity>,
    pub new_epoch: u64,
    pub enrollment_device_id: Option<DeviceId>,
    pub enrollment_nickname_suggestion: Option<String>,
    pub prestate_hash: Option<Hash32>,
}

/// Optional metadata for a ceremony commit.
#[derive(Debug, Clone, Default)]
pub struct CeremonyCommitMetadata {
    pub committed_at: Option<PhysicalTime>,
    pub consensus_id: Option<ConsensusId>,
}

/// Shared ceremony runner API.
#[derive(Clone)]
pub struct CeremonyRunner {
    tracker: CeremonyTracker,
}

impl CeremonyRunner {
    pub fn new(tracker: CeremonyTracker) -> Self {
        Self { tracker }
    }

    /// Register a new ceremony with prestate binding.
    pub async fn start(&self, request: CeremonyInitRequest) -> Result<(), IntentError> {
        self.tracker
            .register_with_prestate(
                request.ceremony_id,
                request.kind,
                request.threshold_k,
                request.total_n,
                request.participants,
                request.new_epoch,
                request.enrollment_device_id,
                request.enrollment_nickname_suggestion,
                request.prestate_hash,
            )
            .await
    }

    /// Record an acceptance response from a participant.
    pub async fn record_response(
        &self,
        ceremony_id: &CeremonyId,
        participant: ParticipantIdentity,
    ) -> Result<bool, IntentError> {
        self.tracker.mark_accepted(ceremony_id, participant).await
    }

    /// Mark ceremony committed (A3 finalized), with optional metadata.
    pub async fn commit(
        &self,
        ceremony_id: &CeremonyId,
        metadata: CeremonyCommitMetadata,
    ) -> Result<(), IntentError> {
        self.tracker
            .mark_committed_with_metadata(ceremony_id, metadata.committed_at, metadata.consensus_id)
            .await
    }

    /// Abort a ceremony with a human-readable reason.
    pub async fn abort(
        &self,
        ceremony_id: &CeremonyId,
        reason: Option<String>,
    ) -> Result<(), IntentError> {
        self.tracker.mark_failed(ceremony_id, reason).await
    }

    /// Check for ceremonies that would be superseded by a new ceremony.
    pub async fn check_supersession_candidates(
        &self,
        kind: CeremonyKind,
        prestate_hash: Option<&Hash32>,
    ) -> Vec<CeremonyId> {
        self.tracker
            .check_supersession_candidates(kind, prestate_hash)
            .await
    }

    /// Mark a ceremony as superseded by a newer ceremony.
    pub async fn supersede(
        &self,
        old_ceremony_id: &CeremonyId,
        new_ceremony_id: &CeremonyId,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, IntentError> {
        self.tracker
            .supersede(old_ceremony_id, new_ceremony_id, reason, timestamp_ms)
            .await
    }

    /// Fetch status for UI/monitoring.
    pub async fn status(&self, ceremony_id: &CeremonyId) -> Result<CeremonyStatus, IntentError> {
        self.tracker.get_status(ceremony_id).await
    }

    /// Check if a ceremony has timed out.
    pub async fn is_timed_out(&self, ceremony_id: &CeremonyId) -> Result<bool, IntentError> {
        self.tracker.is_timed_out(ceremony_id).await
    }
}

//! Ceremony Runner (Category C)
//!
//! Provides a shared API surface for ceremony orchestration in the Layer-6
//! runtime. The runner is the orchestration facade; ceremony-specific logic
//! lives in feature crates and emits facts through the journal.

use super::ceremony_tracker::{CeremonyFailureReason, CeremonyTerminalOutcome, CeremonyTracker};
use aura_app::runtime_bridge::CeremonyKind;
use aura_core::ceremony::{SupersessionReason, SupersessionRecord};
use aura_core::domain::status::CeremonyStatus;
use aura_core::query::ConsensusId;
use aura_core::threshold::ParticipantIdentity;
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::CeremonyId;
use aura_core::AuraError;
use aura_core::{DeviceId, Hash32};
use aura_protocol::{IngressSource, VerifiedIngressEvidence};

/// Inputs required to initiate a ceremony.
#[derive(Debug, Clone)]
pub struct CeremonyInitRequest {
    pub ceremony_id: CeremonyId,
    pub kind: CeremonyKind,
    pub initiator_id: aura_core::types::identifiers::AuthorityId,
    pub threshold_k: u16,
    pub total_n: u16,
    pub participants: Vec<ParticipantIdentity>,
    pub new_epoch: u64,
    pub enrollment_device_id: Option<DeviceId>,
    pub enrollment_nickname_suggestion: Option<String>,
    pub prestate_hash: Hash32,
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
    pub async fn start(&self, request: CeremonyInitRequest) -> Result<(), AuraError> {
        self.tracker
            .register(
                request.ceremony_id,
                request.kind,
                request.initiator_id,
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

    /// Start the issuer allocation without downgrading its held generation owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "register_owned_device_enrollment",
        family = "runtime_helper"
    )]
    pub(crate) async fn start_owned_device_enrollment(
        &self,
        generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        request: CeremonyInitRequest,
    ) -> Result<(), AuraError> {
        self.tracker
            .register_owned_device_enrollment(generation, request)
            .await
    }

    /// Derives the invitation issuer window from its original registered owner

    /// state, including after durable recovery. A restarted task cannot reset it.
    #[cfg(test)]
    pub(crate) async fn enrollment_window_budget(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<aura_core::TimeoutBudget, AuraError> {
        let state = self.tracker.get(ceremony_id).await?;
        if state.kind != CeremonyKind::DeviceEnrollment
            || state.terminal_outcome.is_some()
            || state.is_superseded
            || state.threshold_k == 0
            || state.threshold_k > state.total_n
        {
            return Err(AuraError::invalid(
                "enrollment issuer window requires an active registration",
            ));
        }
        Ok(state.timeout_budget.clone())
    }

    /// Acquire the actual registered generation's single durable window owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "registered_enrollment_execution_window",
        family = "runtime_helper"
    )]
    pub(crate) async fn registered_enrollment_window(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<super::enrollment_window::EnrollmentWindowCapability, AuraError> {
        let capability = self
            .tracker
            .acquire_registered_enrollment_window(ceremony)
            .await?;
        super::enrollment_window::EnrollmentWindowCapability::registered(capability).await
    }

    /// Record an acceptance response produced by the local owner/runtime.
    pub async fn record_local_response(
        &self,
        ceremony_id: &CeremonyId,
        participant: ParticipantIdentity,
    ) -> Result<bool, AuraError> {
        self.tracker.mark_accepted(ceremony_id, participant).await
    }

    /// Record an acceptance response from a remote participant after ingress
    /// verification has authenticated the source.
    pub async fn record_verified_response<E>(
        &self,
        ceremony_id: &CeremonyId,
        participant: ParticipantIdentity,
        evidence: &E,
    ) -> Result<bool, AuraError>
    where
        E: VerifiedIngressEvidence + ?Sized,
    {
        let source = evidence.ingress_evidence().metadata().source();
        let matches_participant = match (&participant, source) {
            (ParticipantIdentity::Device(device), IngressSource::Device(source_device)) => {
                *device == source_device
            }
            (
                ParticipantIdentity::Guardian(authority),
                IngressSource::Authority(source_authority),
            ) => *authority == source_authority,
            (
                ParticipantIdentity::GroupMember { member, .. },
                IngressSource::Authority(source_authority),
            ) => *member == source_authority,
            _ => false,
        };
        if !matches_participant {
            return Err(AuraError::invalid(
                "verified ingress source does not match ceremony participant",
            ));
        }

        self.tracker.mark_accepted(ceremony_id, participant).await
    }

    /// Consume proof minted by the invitation-owned pinned-key verifier.
    pub(crate) async fn record_verified_enrollment_response(
        &self,
        evidence: crate::handlers::invitation::VerifiedEnrollmentResponse,
    ) -> Result<bool, aura_core::AuraError> {
        self.tracker
            .record_verified_enrollment_response(evidence)
            .await
    }

    /// Mark ceremony committed (A3 finalized), with optional metadata.
    pub async fn commit(
        &self,
        ceremony_id: &CeremonyId,
        metadata: CeremonyCommitMetadata,
    ) -> Result<(), AuraError> {
        self.tracker
            .mark_committed_with_metadata(ceremony_id, metadata.committed_at, metadata.consensus_id)
            .await
    }

    /// Complete the ceremony exactly once with an authoritative result.
    pub async fn complete(
        &self,
        ceremony_id: &CeremonyId,
        outcome: CeremonyTerminalOutcome,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        self.tracker.complete(ceremony_id, outcome).await
    }

    pub(crate) async fn await_enrollment_terminal_outcome(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        self.tracker
            .await_enrollment_terminal_outcome(ceremony)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_cancellation",
        family = "runtime_helper"
    )]
    pub(crate) async fn cancel_verified_enrollment(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<super::ceremony_tracker::VerifiedEnrollmentCancellationCapability, AuraError> {
        self.tracker.cancel_verified_enrollment(issued).await
    }

    /// Return the result published by the ceremony owner, if terminal.
    pub async fn terminal_outcome(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<Option<CeremonyTerminalOutcome>, AuraError> {
        self.tracker.terminal_outcome(ceremony_id).await
    }

    /// Fail with a stable category and retain diagnostic text separately.
    pub async fn fail_with_reason(
        &self,
        ceremony_id: &CeremonyId,
        reason: CeremonyFailureReason,
        detail: Option<String>,
    ) -> Result<(), AuraError> {
        self.tracker
            .fail_with_reason(ceremony_id, reason, detail)
            .await
    }

    /// Abort a ceremony with a human-readable reason.
    pub async fn abort(
        &self,
        ceremony_id: &CeremonyId,
        reason: Option<String>,
    ) -> Result<(), AuraError> {
        self.tracker.mark_failed(ceremony_id, reason).await
    }

    /// Check for ceremonies that would be superseded by a new ceremony.
    pub async fn check_supersession_candidates(
        &self,
        kind: CeremonyKind,
        prestate_hash: &Hash32,
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
    ) -> Result<SupersessionRecord, AuraError> {
        self.tracker
            .supersede(old_ceremony_id, new_ceremony_id, reason, timestamp_ms)
            .await
    }

    /// Fetch status for UI/monitoring.
    pub async fn status(&self, ceremony_id: &CeremonyId) -> Result<CeremonyStatus, AuraError> {
        self.tracker.get_status(ceremony_id).await
    }

    /// Check if a ceremony has timed out.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_rejection",
        family = "runtime_helper"
    )]
    pub(crate) async fn record_verified_enrollment_rejection(
        &self,
        proof: crate::handlers::invitation::enrollment_trust::VerifiedEnrollmentRejectionCapability,
    ) -> Result<aura_app::runtime_bridge::CeremonyTerminalOutcome, AuraError> {
        self.tracker
            .record_verified_enrollment_rejection(proof)
            .await
    }

    pub async fn is_timed_out(&self, ceremony_id: &CeremonyId) -> Result<bool, AuraError> {
        self.tracker.is_timed_out(ceremony_id).await
    }
}

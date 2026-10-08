//! # Guardian Ceremony Tracker
//!
//! Tracks state of in-progress guardian ceremonies across the agent runtime.
//!
//! ## Responsibilities
//!
//! - Register new ceremonies with threshold configuration
//! - Track which guardians have accepted invitations
//! - Determine when threshold is reached for ceremony completion
//! - Provide ceremony status for monitoring
//! - Handle ceremony failures and timeouts
//!
//! ## Architecture
//!
//! The tracker maintains in-memory state of active ceremonies. When guardians
//! accept invitations (via `GuardianBinding` facts in the journal), the ceremony
//! state is updated. Once threshold is reached, the ceremony is marked complete
//! and `commit_guardian_key_rotation()` is triggered.
//!
//! ## Status Types
//!
//! This module uses `TrackedCeremony` for internal runtime state tracking, which
//! can be converted to `CeremonyStatus` (from `aura-core::domain::status`) for
//! UI display and consistency tracking.

use super::state::with_state_mut_validated;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use crate::runtime::{AuraEffectSystem, TaskGroup};
use async_trait::async_trait;
use aura_app::runtime_bridge::CeremonyKind;
pub use aura_app::runtime_bridge::{CeremonyFailureReason, CeremonyTerminalOutcome};
use aura_core::ceremony::{SupersessionReason, SupersessionRecord};
use aura_core::domain::status::{
    CeremonyResponse, CeremonyState as StatusCeremonyState, CeremonyStatus, ParticipantResponse,
    SupersessionReason as StatusSupersessionReason,
};
use aura_core::effects::storage::StorageCoreEffects;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::SecureStorageEffects;
use aura_core::query::ConsensusId;
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow, ParticipantIdentity};
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::{AuthorityId, CeremonyId};
use aura_core::AuraError;
use aura_core::{DeviceId, Hash32};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};

const ENROLLMENT_INDEX_KEY: &str = "ceremony-enrollment-index-v1";

fn enrollment_record_key(id: &CeremonyId) -> String {
    format!("ceremony-enrollment-v1:{id}")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredEnrollmentOutcome {
    ceremony_id: CeremonyId,
    started_at_ms: u64,
    timeout_ms: u64,
    budget: aura_core::TimeoutBudget,
    outcome: Option<CeremonyTerminalOutcome>,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentClock {
    ceremony: CeremonyId,
    subject: AuthorityId,
    device: DeviceId,
    prestate: Hash32,
    epoch: u64,
    budget: aura_core::TimeoutBudget,
}

/// Immutable evidence that this exact allocation was eligible to become live.
/// Absence cannot authorize repair after canonical registration was retained.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentLiveWindow {
    ceremony: CeremonyId,
    subject: AuthorityId,
    device: DeviceId,
    prestate: Hash32,
    epoch: u64,
    started_at_ms: u64,
    deadline_at_ms: u64,
}
impl StoredEnrollmentLiveWindow {
    fn from_state(state: &TrackedCeremony) -> Result<Self, AuraError> {
        Ok(Self {
            ceremony: state.ceremony_id.clone(),
            subject: state.initiator_id,
            device: state
                .enrollment_device_id
                .ok_or_else(|| AuraError::invalid("live enrollment requires actual device"))?,
            prestate: state.prestate_hash,
            epoch: state.new_epoch,
            started_at_ms: state.timeout_budget.started_at_ms(),
            deadline_at_ms: state.timeout_budget.deadline_at_ms(),
        })
    }
}
/// Allocated-only repair authorization; minted after checking actual retained phase.
/// It cannot be serialized or supplied by a caller.
struct PreLiveEnrollmentClockCapability {
    original: StoredEnrollmentClock,
}
fn live_enrollment_window_location(
    ceremony: &CeremonyId,
) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::new(
        "enrollment_window_ever_live_v1",
        ceremony.to_string(),
    )
}

/// Negative terminal CAS evidence, never signing or membership authority.
pub(crate) struct VerifiedEnrollmentCancellationCapability {
    runtime_owner: Arc<crate::runtime::AuraEffectSystem>,
    invitation: aura_core::InvitationId,
    ceremony: CeremonyId,
}
impl VerifiedEnrollmentCancellationCapability {
    pub(crate) fn require_runtime_owner(
        &self,
        effects: &Arc<crate::runtime::AuraEffectSystem>,
    ) -> Result<(), AuraError> {
        if Arc::ptr_eq(&self.runtime_owner, effects) {
            Ok(())
        } else {
            Err(AuraError::Invalid { message: "cancellation publication has another runtime owner".into(), source: Some(Arc::new(crate::handlers::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner)) })
        }
    }

    pub(crate) fn invitation(&self) -> &aura_core::InvitationId {
        &self.invitation
    }
    pub(crate) fn ceremony(&self) -> &CeremonyId {
        &self.ceremony
    }
}

/// Tracks state of guardian ceremonies
/// Execution admission minted only from the tracker's actual registered state.
/// Observed `TrackedCeremony` snapshots cannot construct this token.
pub(super) struct RegisteredEnrollmentWindowCapability {
    tracker: CeremonyTracker,
    state: TrackedCeremony,
    lease: Arc<tokio::sync::OwnedSemaphorePermit>,
    notice_binding: std::sync::OnceLock<Arc<RegisteredEnrollmentNoticeBindingCapability>>,
}

/// Completion observation retains original clock identity without an execution
/// permit. It cannot admit a session, sign, bind notices, or manufacture children.
pub(super) struct HeldIssuerClockObservationCapability {
    tracker: CeremonyTracker,
    state: TrackedCeremony,
}
impl HeldIssuerClockObservationCapability {
    pub(super) fn budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.timeout_budget
    }
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        let original = self.tracker.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::permission_denied("issuer completion lacks persistent effect owner")
        })?;
        if !std::ptr::eq(original.as_ref(), effects) {
            return Err(AuraError::permission_denied(
                "issuer completion effect owner changed",
            ));
        }
        Ok(())
    }
    pub(super) async fn checkpoint(&self) -> Result<(), AuraError> {
        self.tracker
            .checkpoint_enrollment_clock_bound(
                &self.state.ceremony_id,
                Some(RegisteredClockCheckpointAuthority::Completion(self)),
            )
            .await
    }
}
/// Exact issuer identity retained by the original registered window. This
/// runtime-local binding is never decoded from storage or peer wire data.
pub(crate) struct RegisteredEnrollmentNoticeBindingCapability {
    manifest_digest: [u8; 32],
    transcript: Vec<u8>,
    expires_at_ms: u64,
}
impl RegisteredEnrollmentWindowCapability {
    pub(super) fn completion_observation(&self) -> HeldIssuerClockObservationCapability {
        HeldIssuerClockObservationCapability {
            tracker: self.tracker.clone(),
            state: self.state.clone(),
        }
    }
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        let original = self.tracker.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::permission_denied("registered clock has no persistent effect owner")
        })?;
        if !std::ptr::eq(original.as_ref(), effects) {
            return Err(AuraError::permission_denied(
                "registered clock effect owner changed",
            ));
        }
        Ok(())
    }
    // This producer stays in the tracker module; callers cannot construct raw
    // state/lease authority or bypass the original allocation verification.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentNoticeBindingCapability",
        family = "runtime_helper"
    )]
    fn prepare_notice_binding(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<RegisteredEnrollmentNoticeBindingCapability, AuraError> {
        use aura_signature::SecurityTranscript;
        let effects = self.tracker.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("notice binding requires original persistent runtime")
        })?;
        issued.require_runtime_owner(effects.as_ref())?;
        let manifest = issued.manifest();
        if self.state.kind != CeremonyKind::DeviceEnrollment
            || self.state.ceremony_id != manifest.ceremony
            || self.state.initiator_id != manifest.subject
            || self.state.enrollment_device_id != Some(manifest.invitee_device)
            || self.state.new_epoch != manifest.pending_epoch
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "notice manifest has another original registered generation".into(),
                },
            ));
        }
        let transcript = manifest
            .transcript_bytes()
            .map_err(|source| AuraError::Internal {
                message: "encode retained notice manifest identity".into(),
                source: Some(Arc::new(source)),
            })?;
        let digest = aura_core::hash::hash(&transcript);
        if digest != issued.digest()
            || manifest.expires_at_ms <= self.state.timeout_budget.started_at_ms()
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "retained notice digest or original validity changed".into(),
                },
            ));
        }
        Ok(RegisteredEnrollmentNoticeBindingCapability {
            manifest_digest: digest,
            transcript,
            expires_at_ms: manifest.expires_at_ms,
        })
    }
    pub(super) fn bind_issued_notice_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<Arc<RegisteredEnrollmentNoticeBindingCapability>, AuraError> {
        let candidate = self.prepare_notice_binding(issued)?;
        let candidate = Arc::new(candidate);
        let original = self.notice_binding.get_or_init(|| candidate.clone());
        Self::require_same_notice_binding(original, &candidate)?;
        Ok(original.clone())
    }
    fn require_same_notice_binding(
        original: &RegisteredEnrollmentNoticeBindingCapability,
        candidate: &RegisteredEnrollmentNoticeBindingCapability,
    ) -> Result<(), AuraError> {
        if original.manifest_digest != candidate.manifest_digest
            || original.transcript != candidate.transcript
            || original.expires_at_ms != candidate.expires_at_ms
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "registered notice identity cannot be rebound".into(),
                },
            ));
        }
        Ok(())
    }
    pub(super) fn require_issued_notice_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        let candidate = self.prepare_notice_binding(issued)?;
        let original = self.notice_binding.get().ok_or_else(|| {
            AuraError::from(aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                detail: "registered notice identity has not been bound".into(),
            })
        })?;
        Self::require_same_notice_binding(original, &candidate)
    }
    pub(super) fn budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.timeout_budget
    }
    pub(super) fn lease(&self) -> Arc<tokio::sync::OwnedSemaphorePermit> {
        self.lease.clone()
    }
    pub(super) async fn checkpoint(&self) -> Result<(), AuraError> {
        self.tracker
            .checkpoint_enrollment_clock_bound(
                &self.state.ceremony_id,
                Some(RegisteredClockCheckpointAuthority::Execution(self)),
            )
            .await
    }
}

impl CeremonyTracker {
    pub(super) async fn held_issuer_window(
        &self,
        effects: &Arc<crate::runtime::AuraEffectSystem>,
        reservation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
    ) -> Result<RegisteredEnrollmentWindowCapability, AuraError> {
        reservation.require_effects(effects.as_ref())?;
        reservation.require_tracker(self)?;
        let persistence =
            self.shared.persistence.as_ref().ok_or_else(|| {
                AuraError::invalid("held issuer requires original persistent clock")
            })?;
        if !Arc::ptr_eq(persistence, effects) {
            return Err(AuraError::permission_denied(
                "held issuer persistence owner changed",
            ));
        }
        // No enrollment decision acquisition here: the supplied reservation
        // already holds that exact gate. Checkpoints use the distinct write gate.
        let state = self.get(reservation.ceremony_id()).await?;
        reservation.validate_allocated_registration(effects.as_ref(), &state)?;
        self.require_live_enrollment_window(&state).await?;
        if state.terminal_outcome.is_some()
            || state.has_failed
            || state.is_committed
            || state.is_superseded
        {
            return Err(AuraError::permission_denied(
                "held issuer allocation is terminal",
            ));
        }
        let lease = state
            .enrollment_window_lease
            .clone()
            .try_acquire_owned()
            .map_err(registered_window_lease_error)?;
        let capability = RegisteredEnrollmentWindowCapability {
            tracker: self.clone(),
            state,
            lease: Arc::new(lease),
            notice_binding: std::sync::OnceLock::new(),
        };
        capability.checkpoint().await?;
        Ok(capability)
    }
}

/// Original allocation observation custody for cancellation preparation only.
/// This does not acquire an execution permit or authorize VM/session admission.
pub(super) struct CancellationClockObservationCapability {
    tracker: CeremonyTracker,
    state: TrackedCeremony,
    signed_expiry_ms: u64,
}
pub(super) enum RegisteredCancellationPreparationCapability {
    Active(Box<CancellationClockObservationCapability>),
    Decided(VerifiedEnrollmentCancellationCapability),
}
/// Original execution custody for a negative notice only. This cannot admit the
/// request/response enrollment protocol or restore pending signing material.
pub(crate) struct RegisteredCancelledNoticeCapability {
    observation: CancellationClockObservationCapability,
    lease: Arc<tokio::sync::OwnedSemaphorePermit>,
    cancelled: VerifiedEnrollmentCancellationCapability,
    manifest_digest: [u8; 32],
}
impl RegisteredCancelledNoticeCapability {
    pub(super) fn into_parts(
        self,
    ) -> (
        CancellationClockObservationCapability,
        Arc<tokio::sync::OwnedSemaphorePermit>,
        VerifiedEnrollmentCancellationCapability,
        [u8; 32],
    ) {
        (
            self.observation,
            self.lease,
            self.cancelled,
            self.manifest_digest,
        )
    }
}
enum RegisteredClockCheckpointAuthority<'a> {
    Execution(&'a RegisteredEnrollmentWindowCapability),
    Completion(&'a HeldIssuerClockObservationCapability),
    Cancellation(&'a CancellationClockObservationCapability),
}
impl CancellationClockObservationCapability {
    pub(super) fn require_effects(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        let owner = self.tracker.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("cancellation observation lost its persistent runtime")
        })?;
        if !std::ptr::eq(owner.as_ref(), effects) {
            return Err(AuraError::Invalid {
                message: "cancellation observation belongs to another runtime".into(),
                source: Some(Arc::new(crate::handlers::invitation::enrollment_trust::EnrollmentVerifierError::RuntimeOwner)),
            });
        }
        Ok(())
    }
    pub(super) fn budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.timeout_budget
    }
    pub(super) fn signed_expiry_ms(&self) -> u64 {
        self.signed_expiry_ms
    }
    pub(super) async fn checkpoint(&self) -> Result<(), AuraError> {
        self.tracker
            .checkpoint_enrollment_clock_bound(
                &self.state.ceremony_id,
                Some(RegisteredClockCheckpointAuthority::Cancellation(self)),
            )
            .await
    }
}

#[derive(Clone)]
pub struct CeremonyTracker {
    /// Time effects for deterministic simulation support
    time: Arc<dyn PhysicalTimeEffects>,
    shared: Arc<CeremonyTrackerShared>,
}

/// Structural lease admission; busy is idempotent, closed is a permanent fault.
#[derive(Debug, thiserror::Error)]
pub(crate) enum RegisteredEnrollmentWindowAdmissionError {
    #[error("registered enrollment window already has its execution owner")]
    AlreadyOwned {
        #[source]
        source: tokio::sync::TryAcquireError,
    },
    #[error("registered enrollment execution lease is closed")]
    Closed {
        #[source]
        source: tokio::sync::TryAcquireError,
    },
}
pub(crate) fn registered_window_lease_error(source: tokio::sync::TryAcquireError) -> AuraError {
    AuraError::Internal {
        message: "admit registered enrollment execution lease".into(),
        source: Some(Arc::new(match source {
            tokio::sync::TryAcquireError::NoPermits => {
                RegisteredEnrollmentWindowAdmissionError::AlreadyOwned { source }
            }
            tokio::sync::TryAcquireError::Closed => {
                RegisteredEnrollmentWindowAdmissionError::Closed { source }
            }
        })),
    }
}

pub(crate) fn registered_enrollment_window_already_owned(error: &AuraError) -> bool {
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(source) = current {
        if matches!(
            source.downcast_ref::<RegisteredEnrollmentWindowAdmissionError>(),
            Some(RegisteredEnrollmentWindowAdmissionError::AlreadyOwned { .. })
        ) {
            return true;
        }
        current = source.source();
    }
    false
}

struct CeremonyTrackerShared {
    state: RwLock<CeremonyTrackerState>,
    /// Authoritative lifecycle state for runtime health.
    lifecycle: RwLock<ServiceHealth>,
    /// Owned cleanup tasks for ceremony timeout maintenance.
    cleanup_tasks: RwLock<Option<TaskGroup>>,
    persistence: Option<Arc<AuraEffectSystem>>,
    persistence_guard: Mutex<()>,
    #[cfg(test)]
    clock_checkpoint_fault: Mutex<Option<AuraError>>,
    enrollment_decision_gate: Mutex<()>,
    terminal_changed: tokio::sync::Notify,
    /// Each cancelled-notice owner publishes whether it signed and released
    /// the cancelled pending generation. Retained, so cancel retries observe it.
    cancelled_generation_settlements: Mutex<HashMap<CeremonyId, Result<(), AuraError>>>,
    cancelled_generation_settled: tokio::sync::Notify,
}

#[derive(Debug, Default)]
struct CeremonyTrackerState {
    ceremonies: HashMap<CeremonyId, TrackedCeremony>,
    // Opaque capability is never restored by deserializing persisted records.
    enrollment_responses:
        HashMap<CeremonyId, crate::handlers::invitation::VerifiedEnrollmentResponse>,
    retired_enrollment_ids: HashSet<CeremonyId>,
    /// Supersession records for audit trail
    supersession_records: Vec<SupersessionRecord>,
}

impl CeremonyTrackerState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for (id, proof) in &self.enrollment_responses {
            let Some(state) = self.ceremonies.get(id) else {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    "orphan enrollment response",
                ));
            };
            if state.kind != CeremonyKind::DeviceEnrollment
                || proof.ceremony_id() != id
                || proof.subject() != state.initiator_id
                || Some(proof.device_id()) != state.enrollment_device_id
                || proof.pending_epoch() != state.new_epoch
                || !state
                    .accepted_participants
                    .contains(&ParticipantIdentity::device(proof.device_id()))
            {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    "enrollment response binding drift",
                ));
            }
        }
        for (ceremony_id, state) in &self.ceremonies {
            if state.timeout_budget.started_at_ms() != state.started_at.ts_ms
                || u64::try_from(state.timeout.as_millis()).ok()
                    != Some(state.timeout_budget.timeout_ms())
            {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    "original timeout window drift",
                ));
            }
            if state.threshold_k == 0 {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!("ceremony {} has zero threshold", ceremony_id),
                ));
            }
            if state.threshold_k > state.total_n {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} threshold {} exceeds total {}",
                        ceremony_id, state.threshold_k, state.total_n
                    ),
                ));
            }
            if state.total_n as usize != state.participants.len() {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} total_n {} does not match participant count {}",
                        ceremony_id,
                        state.total_n,
                        state.participants.len()
                    ),
                ));
            }
            if !state.accepted_participants.is_subset(&state.participants) {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} has accepted participants not in participant list",
                        ceremony_id
                    ),
                ));
            }
            if state.is_committed && state.has_failed {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!("ceremony {} cannot be committed and failed", ceremony_id),
                ));
            }
            if state.terminal_outcome
                != (if state.is_committed {
                    Some(CeremonyTerminalOutcome::Committed)
                } else if state.has_failed {
                    Some(CeremonyTerminalOutcome::Failed(
                        state
                            .failure_reason
                            .unwrap_or(CeremonyFailureReason::RuntimeFailed),
                    ))
                } else {
                    None
                })
            {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} terminal outcome does not match state",
                        ceremony_id
                    ),
                ));
            }
            if state.is_superseded && state.is_committed {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} cannot be superseded and committed",
                        ceremony_id
                    ),
                ));
            }
            if state.is_committed && state.accepted_participants.len() < state.threshold_k as usize
            {
                return Err(super::invariant::InvariantViolation::new(
                    "CeremonyTracker",
                    format!(
                        "ceremony {} committed without reaching threshold",
                        ceremony_id
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// Internal state of a tracked ceremony.
///
/// This struct holds runtime-specific data for ceremony tracking. For UI display
/// and consistency tracking, convert to `CeremonyStatus` using `to_status()`.
#[derive(Debug, Clone)]
pub struct TrackedCeremony {
    /// Unique ceremony identifier
    pub ceremony_id: CeremonyId,

    /// Ceremony kind
    pub kind: CeremonyKind,

    /// Authority that initiated the ceremony
    pub initiator_id: AuthorityId,

    /// Threshold required for completion (k)
    pub threshold_k: u16,

    /// Total number of participants (n)
    pub total_n: u16,

    /// Participants invited to participate
    pub participants: HashSet<ParticipantIdentity>,

    /// Participants who have accepted
    pub accepted_participants: HashSet<ParticipantIdentity>,

    /// New epoch for the key rotation
    pub new_epoch: u64,

    /// Device being enrolled (DeviceEnrollment ceremonies only).
    pub enrollment_device_id: Option<DeviceId>,

    /// Nickname suggestion for the enrolling device (DeviceEnrollment ceremonies only).
    ///
    /// Stored here to be embedded in `DeviceLeafMetadata` when enrollment completes.
    pub enrollment_nickname_suggestion: Option<String>,

    /// When the ceremony was initiated
    pub started_at: PhysicalTime,

    /// Whether the ceremony has failed
    pub has_failed: bool,

    /// Whether the ceremony has been committed (key rotation activated)
    pub is_committed: bool,

    /// Whether the ceremony has been superseded by another ceremony
    pub is_superseded: bool,

    /// ID of the ceremony that supersedes this one (if superseded)
    pub superseded_by: Option<CeremonyId>,

    /// IDs of ceremonies that this ceremony supersedes
    pub supersedes: Vec<CeremonyId>,

    /// Agreement mode (A1/A2/A3) for the ceremony lifecycle
    pub agreement_mode: AgreementMode,

    /// Optional error message if failed
    pub error_message: Option<String>,

    /// Stable terminal result; set once by the ceremony owner.
    pub terminal_outcome: Option<CeremonyTerminalOutcome>,

    /// Stable failure classification, separate from diagnostic text.
    pub failure_reason: Option<CeremonyFailureReason>,

    /// Timeout duration (30 seconds default)
    pub timeout: Duration,

    /// Original owner window; clones retain observation and expiration.
    pub(crate) timeout_budget: aura_core::TimeoutBudget,
    pub(crate) enrollment_window_lease: Arc<tokio::sync::Semaphore>,

    /// Prestate hash at ceremony initiation (for supersession detection)
    pub prestate_hash: Hash32,

    /// Timestamp when committed (if committed)
    pub committed_at: Option<PhysicalTime>,

    /// Consensus ID when committed (if committed)
    pub committed_consensus_id: Option<ConsensusId>,
}

impl TrackedCeremony {
    /// Convert to `CeremonyStatus` for UI display and consistency tracking.
    pub fn to_status(&self) -> CeremonyStatus {
        // Helper to create a zero physical time (for cases where we don't have the actual time)
        let zero_time = PhysicalTime {
            ts_ms: 0,
            uncertainty: None,
        };
        let zero_consensus_id = ConsensusId::new([0; 32]);

        // Convert internal state to StatusCeremonyState enum
        let state = if self.is_committed {
            StatusCeremonyState::Committed {
                consensus_id: self.committed_consensus_id.unwrap_or(zero_consensus_id),
                committed_at: self.committed_at.clone().unwrap_or(zero_time.clone()),
            }
        } else if self.is_superseded {
            StatusCeremonyState::Superseded {
                by: self
                    .superseded_by
                    .clone()
                    .unwrap_or_else(|| CeremonyId::new("unknown")),
                reason: StatusSupersessionReason::NewerRequest,
            }
        } else if self.has_failed {
            StatusCeremonyState::Aborted {
                reason: self
                    .error_message
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
                aborted_at: zero_time.clone(),
            }
        } else if self.accepted_participants.len() >= self.threshold_k as usize {
            StatusCeremonyState::Committing
        } else if !self.accepted_participants.is_empty() {
            StatusCeremonyState::PendingEpoch {
                pending_epoch: aura_core::types::Epoch::new(self.new_epoch),
                required_responses: self.threshold_k,
                received_responses: self.accepted_participants.len() as u16,
            }
        } else {
            StatusCeremonyState::Preparing
        };

        // Convert accepted participants to responses (guardians only for status)
        // Devices and group members are tracked internally but don't map cleanly to AuthorityId
        let responses: Vec<ParticipantResponse> = self
            .accepted_participants
            .iter()
            .filter_map(|p| match p {
                ParticipantIdentity::Guardian(auth_id) => Some(ParticipantResponse {
                    participant: *auth_id,
                    response: CeremonyResponse::Accept,
                    responded_at: zero_time.clone(), // We don't track individual response times
                }),
                ParticipantIdentity::Device(_device_id) => {
                    // Skip devices for status - tracked internally
                    None
                }
                ParticipantIdentity::GroupMember { .. } => {
                    // Skip group members for status - tracked internally
                    None
                }
            })
            .collect();

        // Get committed agreement if applicable
        let committed_agreement = if self.is_committed {
            Some(aura_core::domain::Agreement::Finalized {
                consensus_id: self.committed_consensus_id.unwrap_or(zero_consensus_id),
            })
        } else {
            None
        };

        CeremonyStatus {
            ceremony_id: self.ceremony_id.clone(),
            state,
            responses,
            prestate_hash: self.prestate_hash,
            committed_agreement,
        }
    }
}

/// One lock hierarchy: actual physical generation, then tracker decision.
/// Tree custody may only be acquired after this composite owner exists.
/// The token is neither clonable nor deserializable and releases in reverse order.
pub(crate) struct EnrollmentGenerationDecisionCapability<'a> {
    tracker: &'a CeremonyTracker,
    _decision: tokio::sync::MutexGuard<'a, ()>,
    generation: crate::runtime::effects::EnrollmentGenerationCustodyCapability<'a>,
}
impl EnrollmentGenerationDecisionCapability<'_> {
    pub(crate) fn tracker(&self) -> CeremonyTracker {
        self.tracker.clone()
    }
    pub(crate) fn require_tracker(&self, tracker: &CeremonyTracker) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.tracker.shared, &tracker.shared) {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }
    pub(crate) fn require_effects(
        &self,
        effects: &crate::runtime::effects::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.generation.require_effects(effects)
    }
    pub(crate) fn generation(
        &self,
    ) -> &crate::runtime::effects::EnrollmentGenerationCustodyCapability<'_> {
        &self.generation
    }
}

/// Holds the decision gate from verified admission through activation publication.
/// Registry invalidation, timeout and cancellation cannot win while this owner acts.
pub(crate) struct EnrollmentActivationCapability<'a> {
    tracker: &'a CeremonyTracker,
    ceremony_id: CeremonyId,
    custody: EnrollmentGenerationDecisionCapability<'a>,
}

impl EnrollmentActivationCapability<'_> {
    pub(crate) fn require_effects(
        &self,
        effects: &crate::runtime::effects::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.custody.require_effects(effects)
    }
    pub(crate) fn generation(
        &self,
    ) -> &crate::runtime::effects::EnrollmentGenerationCustodyCapability<'_> {
        self.custody.generation()
    }

    pub(crate) fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony_id
    }
    pub(crate) fn require_tracker(&self, tracker: &CeremonyTracker) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.tracker.shared, &tracker.shared) {
            return Err(AuraError::invalid(
                "activation capability belongs to another runtime tracker",
            ));
        }
        Ok(())
    }
    pub(crate) async fn require_generation(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<(), AuraError> {
        let state = self.tracker.get(&self.ceremony_id).await?;
        let proof = self
            .tracker
            .verified_enrollment_response(&self.ceremony_id)
            .await?;
        if state.initiator_id != authority
            || state.new_epoch != epoch
            || proof.subject() != authority
            || proof.pending_epoch() != epoch
            || proof.ceremony_id() != &self.ceremony_id
            || state.terminal_outcome.is_some()
        {
            return Err(AuraError::invalid(
                "activation lease does not authorize the signing generation",
            ));
        }
        Ok(())
    }

    pub(crate) async fn commit(self) -> Result<(), aura_core::AuraError> {
        self.tracker
            .complete_under_decision_gate(&self.ceremony_id, CeremonyTerminalOutcome::Committed)
            .await
            .map(|_| ())
            .map_err(|error| aura_core::AuraError::Internal {
                message: "publish enrollment activation".into(),
                source: Some(std::sync::Arc::new(error)),
            })
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentTerminalDecision {
    version: u16,
    record: StoredEnrollmentOutcome,
    prestate: Hash32,
    accepted: Vec<ParticipantIdentity>,
}
fn terminal_decision_location(ceremony: &CeremonyId) -> aura_core::effects::SecureStorageLocation {
    aura_core::effects::SecureStorageLocation::new(
        "device_enrollment_terminal_decision_v1",
        ceremony.to_string(),
    )
}

/// A failed durable decision owns retirement until required secure deletion finishes.
/// Private fields and a held decision guard prevent callers from manufacturing cleanup.
pub(crate) struct EnrollmentRetirementCapability<'a> {
    effects: &'a crate::runtime::AuraEffectSystem,
    _generation: tokio::sync::MutexGuard<'a, ()>,
    ceremony: CeremonyId,
    prestate: Hash32,
    authority: AuthorityId,
    epoch: u64,
    package_digest: [u8; 32],
    config_digest: [u8; 32],
    _decision: tokio::sync::MutexGuard<'a, ()>,
}
impl EnrollmentRetirementCapability<'_> {
    pub(crate) fn effects(&self) -> &crate::runtime::AuraEffectSystem {
        self.effects
    }
    pub(crate) fn ceremony(&self) -> &CeremonyId {
        &self.ceremony
    }
    pub(crate) fn prestate(&self) -> Hash32 {
        self.prestate
    }
    pub(crate) fn binding(&self) -> (AuthorityId, u64, [u8; 32], [u8; 32]) {
        (
            self.authority,
            self.epoch,
            self.package_digest,
            self.config_digest,
        )
    }
}
impl CeremonyTracker {
    pub(crate) fn require_same_owner(&self, actual: &CeremonyTracker) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.shared, &actual.shared) {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "physical_generation_custody",
        capability_type = EnrollmentGenerationDecisionCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn acquire_enrollment_generation_decision<'a>(
        &'a self,
        effects: &'a crate::runtime::effects::AuraEffectSystem,
    ) -> Result<EnrollmentGenerationDecisionCapability<'a>, AuraError> {
        self.retain_enrollment_generation_decision(
            effects.acquire_enrollment_generation_custody().await,
        )
        .await
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationCustodyCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn retain_enrollment_generation_decision<'a>(
        &'a self,
        generation: crate::runtime::effects::EnrollmentGenerationCustodyCapability<'a>,
    ) -> Result<EnrollmentGenerationDecisionCapability<'a>, AuraError> {
        let actual = self.shared.persistence.as_ref().ok_or_else(|| {
            crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            )
        })?;
        generation.require_effects(actual.as_ref())?;
        let decision = self.shared.enrollment_decision_gate.lock().await;
        Ok(EnrollmentGenerationDecisionCapability {
            tracker: self,
            _decision: decision,
            generation,
        })
    }

    pub(crate) async fn restore_retired_orphan_registration(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<(), AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("orphan observation requires original owner storage")
        })?;
        let mut registration = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(effects, ceremony).await
            .map_err(|error| AuraError::Internal { message: "restore original failed allocation".into(), source: Some(std::sync::Arc::new(error)) })?;
        effects
            .require_retired_orphan_registration(&registration)
            .await?;
        let outcome = CeremonyTerminalOutcome::Failed(CeremonyFailureReason::RuntimeFailed);
        self.persist_first_enrollment_terminal_decision(&registration, outcome)
            .await?;
        registration.has_failed = true;
        registration.terminal_outcome = Some(outcome);
        registration.failure_reason = Some(CeremonyFailureReason::RuntimeFailed);
        registration.error_message =
            Some("enrollment issuance did not commit its invitation".into());
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                if let Some(existing) = tracker.ceremonies.get(ceremony) {
                    if existing.prestate_hash != registration.prestate_hash || existing.is_committed
                    {
                        return Err(AuraError::invalid(
                            "orphan failure contradicts existing generation",
                        ));
                    }
                }
                tracker.enrollment_responses.remove(ceremony);
                tracker.ceremonies.insert(ceremony.clone(), registration);
                Ok::<(), AuraError>(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Publish a cancelled-notice owner's one sign-and-release result.
    pub(crate) async fn publish_cancelled_generation_settlement(
        &self,
        ceremony: &CeremonyId,
        result: Result<(), AuraError>,
    ) {
        self.shared
            .cancelled_generation_settlements
            .lock()
            .await
            .insert(ceremony.clone(), result);
        self.shared.cancelled_generation_settled.notify_waiters();
    }

    /// Await the cancelled-notice owner's sign-and-release result.
    pub(crate) async fn await_cancelled_generation_settlement(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<(), AuraError> {
        loop {
            let changed = self.shared.cancelled_generation_settled.notified();
            tokio::pin!(changed);
            // Register before checking, so a settlement in between is not lost.
            changed.as_mut().enable();
            if let Some(result) = self
                .shared
                .cancelled_generation_settlements
                .lock()
                .await
                .get(ceremony)
                .cloned()
            {
                return result;
            }
            changed.await;
        }
    }

    pub(crate) async fn retire_failed_enrollment_generation(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<(), AuraError> {
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("retirement requires durable owner storage"))?;
        // Issuance also owns the generation writer before it enters the registry.
        let generation_guard = effects.enrollment_retirement_generation_guard().await;
        let decision = self.shared.enrollment_decision_gate.lock().await;
        let current = self.get(ceremony).await?;

        if current.kind != CeremonyKind::DeviceEnrollment
            || !matches!(
                current.terminal_outcome,
                Some(CeremonyTerminalOutcome::Failed(_))
            )
            || effects
                .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                    "device_enrollment_activation_v1",
                    ceremony.to_string(),
                ))
                .await?
        {
            return Err(AuraError::invalid(
                "enrollment is not eligible for generation retirement",
            ));
        }
        let durable = self
            .retained_terminal_decision(ceremony)
            .await?
            .ok_or_else(|| AuraError::invalid("missing durable failed decision"))?;
        if durable.prestate != current.prestate_hash
            || durable.record.outcome != current.terminal_outcome
        {
            return Err(AuraError::invalid(
                "retirement decision differs from original generation",
            ));
        }
        let (package_digest, config_digest) =
            crate::handlers::invitation::enrollment_trust::failed_generation_binding(
                effects,
                ceremony,
                current.initiator_id,
                current.new_epoch,
                current.prestate_hash,
            )
            .await
            .map_err(|error| AuraError::Internal {
                message: "load owned failed generation".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        let lease = EnrollmentRetirementCapability {
            effects,
            _generation: generation_guard,
            ceremony: ceremony.clone(),
            prestate: current.prestate_hash,
            authority: current.initiator_id,
            epoch: current.new_epoch,
            package_digest,
            config_digest,
            _decision: decision,
        };
        effects.retire_pinned_enrollment_generation(&lease).await
    }

    fn timeout_for_kind(kind: CeremonyKind) -> Duration {
        match kind {
            // Guardians and other devices approve on their own screens, as a
            // person does for enrollment; 60 s timed out in practice (run 147).
            CeremonyKind::GuardianRotation
            | CeremonyKind::DeviceRotation
            | CeremonyKind::Recovery => Duration::from_secs(600),
            // Enrollment waits for a person to import the code on the new device.
            CeremonyKind::DeviceEnrollment => Duration::from_millis(
                aura_invitation::enrollment_manifest::ENROLLMENT_ALLOCATION_TIMEOUT_MS,
            ),
            CeremonyKind::DeviceRemoval => Duration::from_secs(45),
            CeremonyKind::OtaActivation => Duration::from_secs(90),
            // Invitations wait on a human accepting on another device.
            CeremonyKind::Invitation => Duration::from_secs(600),
            CeremonyKind::RendezvousSecureChannel => Duration::from_secs(20),
        }
    }

    fn initial_mode_for_kind(kind: CeremonyKind) -> AgreementMode {
        match kind {
            CeremonyKind::GuardianRotation => {
                policy_for(CeremonyFlow::GuardianSetupRotation).initial_mode()
            }
            CeremonyKind::DeviceRotation => {
                policy_for(CeremonyFlow::DeviceMfaRotation).initial_mode()
            }
            CeremonyKind::DeviceEnrollment => {
                policy_for(CeremonyFlow::DeviceEnrollment).initial_mode()
            }
            CeremonyKind::DeviceRemoval => policy_for(CeremonyFlow::DeviceRemoval).initial_mode(),
            CeremonyKind::Recovery => policy_for(CeremonyFlow::RecoveryExecution).initial_mode(),
            CeremonyKind::Invitation => policy_for(CeremonyFlow::Invitation).initial_mode(),
            CeremonyKind::RendezvousSecureChannel => {
                policy_for(CeremonyFlow::RendezvousSecureChannel).initial_mode()
            }
            CeremonyKind::OtaActivation => policy_for(CeremonyFlow::OtaActivation).initial_mode(),
        }
    }
    /// Create a new ceremony tracker
    ///
    /// # Arguments
    /// * `time` - Time effects for deterministic simulation support
    pub fn new(time: Arc<dyn PhysicalTimeEffects>) -> Self {
        Self::new_with_optional_storage(time, None)
    }

    /// Create a production tracker whose enrollment results survive runtime restarts.
    pub fn new_with_storage(
        time: Arc<dyn PhysicalTimeEffects>,
        effects: Arc<AuraEffectSystem>,
    ) -> Self {
        Self::new_with_optional_storage(time, Some(effects))
    }

    fn new_with_optional_storage(
        time: Arc<dyn PhysicalTimeEffects>,
        persistence: Option<Arc<AuraEffectSystem>>,
    ) -> Self {
        Self {
            time,
            shared: Arc::new(CeremonyTrackerShared {
                state: RwLock::new(CeremonyTrackerState::default()),
                lifecycle: RwLock::new(ServiceHealth::NotStarted),
                cleanup_tasks: RwLock::new(None),
                persistence,
                persistence_guard: Mutex::new(()),
                #[cfg(test)]
                clock_checkpoint_fault: Mutex::new(None),
                enrollment_decision_gate: Mutex::new(()),
                terminal_changed: tokio::sync::Notify::new(),
                cancelled_generation_settlements: Mutex::new(HashMap::new()),
                cancelled_generation_settled: tokio::sync::Notify::new(),
            }),
        }
    }

    async fn retained_terminal_decision(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<Option<StoredEnrollmentTerminalDecision>, AuraError> {
        let Some(effects) = &self.shared.persistence else {
            return Ok(None);
        };
        let location = terminal_decision_location(ceremony);
        if !effects.secure_exists(&location).await? {
            return Ok(None);
        }
        let bytes = effects
            .secure_retrieve(
                &location,
                &[aura_core::effects::SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 1_048_576 {
            return Err(AuraError::invalid("oversized enrollment terminal decision"));
        }
        let raw: StoredEnrollmentTerminalDecision =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode enrollment first terminal decision".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if raw.version != 1
            || &raw.record.ceremony_id != ceremony
            || raw.record.outcome.is_none()
            || raw.accepted.len() > 1024
        {
            return Err(AuraError::invalid("invalid enrollment terminal decision"));
        }
        Ok(Some(raw))
    }

    async fn persist_first_enrollment_terminal_decision(
        &self,
        current: &TrackedCeremony,
        outcome: CeremonyTerminalOutcome,
    ) -> Result<(), AuraError> {
        if current.kind != CeremonyKind::DeviceEnrollment {
            return Ok(());
        }
        let Some(effects) = &self.shared.persistence else {
            return Ok(());
        };
        if let Some(existing) = self
            .retained_terminal_decision(&current.ceremony_id)
            .await?
        {
            return if existing.record.outcome == Some(outcome)
                && existing.prestate == current.prestate_hash
            {
                Ok(())
            } else {
                Err(AuraError::invalid(
                    "contradictory retained enrollment terminal decision",
                ))
            };
        }
        let mut ordered = current
            .accepted_participants
            .iter()
            .map(|participant| {
                serde_json::to_vec(participant)
                    .map(|bytes| (bytes, participant.clone()))
                    .map_err(|error| AuraError::Internal {
                        message: "encode accepted inventory".into(),
                        source: Some(std::sync::Arc::new(error)),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        ordered.sort_by(|a, b| a.0.cmp(&b.0));
        let raw = StoredEnrollmentTerminalDecision {
            version: 1,
            prestate: current.prestate_hash,
            accepted: ordered.into_iter().map(|(_, identity)| identity).collect(),
            record: StoredEnrollmentOutcome {
                ceremony_id: current.ceremony_id.clone(),
                started_at_ms: current.started_at.ts_ms,
                timeout_ms: u64::try_from(current.timeout.as_millis())
                    .map_err(|_| AuraError::invalid("enrollment timeout overflow"))?,
                budget: current.timeout_budget.clone(),
                outcome: Some(outcome),
            },
        };
        if raw.accepted.len() > 1024 {
            return Err(AuraError::invalid("oversized accepted inventory"));
        }
        let bytes = serde_json::to_vec(&raw).map_err(|error| AuraError::Internal {
            message: "encode enrollment terminal decision".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        effects
            .secure_store_immutable(
                &terminal_decision_location(&current.ceremony_id),
                &bytes,
                &[aura_core::effects::SecureStorageCapability::Write],
            )
            .await?;
        // Atomic publication may have lost to another valid decision. Decode
        // the retained original, rather than authorizing from our proposed bytes.
        let retained = self
            .retained_terminal_decision(&current.ceremony_id)
            .await?
            .ok_or_else(|| AuraError::invalid("terminal decision disappeared after publication"))?;
        if retained.record.outcome != raw.record.outcome
            || retained.prestate != current.prestate_hash
            || retained.record.started_at_ms != current.started_at.ts_ms
            || retained.record.timeout_ms != raw.record.timeout_ms
        {
            return Err(AuraError::invalid(
                "contradictory retained enrollment terminal decision",
            ));
        }
        Ok(())
    }

    pub(crate) fn require_enrollment_clock_storage(&self) -> Result<(), AuraError> {
        self.shared
            .persistence
            .as_ref()
            .map(|_| ())
            .ok_or_else(|| AuraError::invalid("durable enrollment clock requires secure storage"))
    }

    async fn require_live_enrollment_window(
        &self,
        state: &TrackedCeremony,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("live window requires durable storage"))?;
        let bytes = effects
            .secure_retrieve(
                &live_enrollment_window_location(&state.ceremony_id),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid(
                "oversized live enrollment window binding",
            ));
        }
        let live: StoredEnrollmentLiveWindow =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Internal {
                message: "decode live enrollment allocation binding".into(),
                source: Some(Arc::new(source)),
            })?;
        if live != StoredEnrollmentLiveWindow::from_state(state)? {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "retained live enrollment allocation changed".into(),
                },
            ));
        }
        Ok(())
    }

    /// The held allocation publishes its original clock observation before live eligibility.
    async fn admit_allocated_enrollment_clock(
        &self,
        generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        state: &mut TrackedCeremony,
        now: &PhysicalTime,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            )
        })?;
        generation.validate_allocated_registration(effects, state)?;
        let _write = self.shared.persistence_guard.lock().await;
        self.restore_enrollment_clock(state).await?;
        let observed = state.timeout_budget.remaining_at(now);
        let checkpoint = StoredEnrollmentClock {
            ceremony: state.ceremony_id.clone(),
            subject: state.initiator_id,
            device: state
                .enrollment_device_id
                .ok_or_else(|| AuraError::invalid("allocation lacks physical device"))?,
            prestate: state.prestate_hash,
            epoch: state.new_epoch,
            budget: state.timeout_budget.clone(),
        };
        let bytes = serde_json::to_vec(&checkpoint).map_err(|source| AuraError::Serialization {
            message: "encode owned allocation clock eligibility".into(),
            source: Some(Arc::new(source)),
        })?;
        effects
            .secure_store(
                &SecureStorageLocation::new("enrollment_clock_v1", state.ceremony_id.to_string()),
                &bytes,
                &[SecureStorageCapability::Write],
            )
            .await?;
        observed.map(|_| ()).map_err(AuraError::from)
    }

    async fn prepare_initial_enrollment_clock(
        &self,
        state: &mut TrackedCeremony,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{SecureStorageEffects, SecureStorageLocation};
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("initial window requires durable storage"))?;
        let clock_location =
            SecureStorageLocation::new("enrollment_clock_v1", state.ceremony_id.to_string());
        if effects
            .secure_exists(&live_enrollment_window_location(&state.ceremony_id))
            .await?
        {
            self.require_live_enrollment_window(state).await?;
            // Once eligible to become live, loss of the clock cannot allocate a new one.
            return self.restore_enrollment_clock(state).await;
        }
        if effects.secure_exists(&clock_location).await? {
            return self.restore_enrollment_clock(state).await;
        }
        // A canonical registration record is also evidence of reaching the live
        // boundary, including conservative migration of records predating the marker.
        if self
            .stored_enrollment_outcome(&state.ceremony_id)
            .await?
            .is_some()
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "previously registered enrollment lost its retained clock".into(),
                },
            ));
        }
        let capability = PreLiveEnrollmentClockCapability {
            original: StoredEnrollmentClock {
                ceremony: state.ceremony_id.clone(),
                subject: state.initiator_id,
                device: state
                    .enrollment_device_id
                    .ok_or_else(|| AuraError::invalid("allocated window requires actual device"))?,
                prestate: state.prestate_hash,
                epoch: state.new_epoch,
                budget: state.timeout_budget.clone(),
            },
        };
        self.retain_pre_live_enrollment_clock(capability).await?;
        self.restore_enrollment_clock(state).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "retain_pre_live_enrollment_clock",
        capability_type = PreLiveEnrollmentClockCapability,
        family = "runtime_helper"
    )]
    async fn retain_pre_live_enrollment_clock(
        &self,
        capability: PreLiveEnrollmentClockCapability,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("allocated window requires durable storage"))?;
        #[cfg(test)]
        if let Some(source) = self.shared.clock_checkpoint_fault.lock().await.take() {
            return Err(source);
        }
        let bytes =
            serde_json::to_vec(&capability.original).map_err(|source| AuraError::Internal {
                message: "retain original pre-live enrollment clock".into(),
                source: Some(Arc::new(source)),
            })?;
        effects
            .secure_create_mutable(
                &SecureStorageLocation::new(
                    "enrollment_clock_v1",
                    capability.original.ceremony.to_string(),
                ),
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        Ok(())
    }

    /// Seal the live boundary before registry insertion. Old canonical records
    /// may acquire this marker only while their original secure clock exists.
    async fn retain_live_enrollment_window(
        &self,
        state: &TrackedCeremony,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("live window requires durable storage"))?;
        let allocated = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(effects, &state.ceremony_id)
        .await.map_err(|source| AuraError::Internal { message: "require original allocated window before live admission".into(), source: Some(Arc::new(source)) })?;
        if StoredEnrollmentLiveWindow::from_state(&allocated)?
            != StoredEnrollmentLiveWindow::from_state(state)?
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "live window differs from retained original allocation".into(),
                },
            ));
        }
        let mut retained = state.clone();
        self.restore_enrollment_clock(&mut retained).await?;
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&retained.timeout_budget)
            .map_err(AuraError::from)?;
        let marker = StoredEnrollmentLiveWindow::from_state(state)?;
        let bytes = serde_json::to_vec(&marker).map_err(|source| AuraError::Internal {
            message: "encode live enrollment allocation binding".into(),
            source: Some(Arc::new(source)),
        })?;
        effects
            .secure_store_immutable(
                &live_enrollment_window_location(&state.ceremony_id),
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        self.require_live_enrollment_window(state).await
    }

    #[cfg(test)]
    pub(crate) async fn fail_next_cancellation_checkpoint_for_test(&self, source: AuraError) {
        *self.shared.clock_checkpoint_fault.lock().await = Some(source);
    }

    /// Persist the latest owner observation before a required step continues.
    /// This gate is separate from the enrollment decision gate; callers holding
    /// that gate may checkpoint without recursively acquiring it.
    pub(crate) async fn checkpoint_enrollment_clock(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<(), AuraError> {
        self.checkpoint_enrollment_clock_bound(ceremony, None).await
    }
    async fn checkpoint_enrollment_clock_bound(
        &self,
        ceremony: &CeremonyId,
        expected: Option<RegisteredClockCheckpointAuthority<'_>>,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("durable enrollment window requires secure storage")
        })?;
        let _write = self.shared.persistence_guard.lock().await;
        #[cfg(test)]
        if let Some(source) = self.shared.clock_checkpoint_fault.lock().await.take() {
            return Err(source);
        }
        // Read after acquiring the write gate, so an older suspended writer
        // cannot overwrite a newer shared high-water snapshot.
        let registry = self.shared.state.read().await;
        let state = registry
            .ceremonies
            .get(ceremony)
            .ok_or_else(|| AuraError::invalid("enrollment clock owner is not registered"))?;
        self.require_live_enrollment_window(state).await?;
        let mut durable = state.clone();
        self.restore_enrollment_clock(&mut durable).await?;
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&durable.timeout_budget)
            .map_err(AuraError::from)?;
        if let Some(expected) = expected {
            let original = match expected {
                RegisteredClockCheckpointAuthority::Execution(capability) => &capability.state,
                RegisteredClockCheckpointAuthority::Completion(capability) => &capability.state,
                RegisteredClockCheckpointAuthority::Cancellation(capability) => &capability.state,
            };
            let mut retained = original.clone();
            self.restore_enrollment_clock(&mut retained).await?;
            if state.ceremony_id != original.ceremony_id
                || state.initiator_id != original.initiator_id
                || state.enrollment_device_id != original.enrollment_device_id
                || state.prestate_hash != original.prestate_hash
                || state.new_epoch != original.new_epoch
                || state.timeout_budget.started_at_ms() != original.timeout_budget.started_at_ms()
                || state.timeout_budget.deadline_at_ms() != original.timeout_budget.deadline_at_ms()
                || !state
                    .timeout_budget
                    .shares_observation_owner_with(&original.timeout_budget)
                || !Arc::ptr_eq(
                    &state.enrollment_window_lease,
                    &original.enrollment_window_lease,
                )
            {
                return Err(AuraError::from(
                    aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                        detail: "registered enrollment window allocation changed".into(),
                    },
                ));
            }
        }

        if state.kind != CeremonyKind::DeviceEnrollment
            || state.timeout_budget.started_at_ms() != state.started_at.ts_ms
        {
            return Err(AuraError::invalid(
                "enrollment checkpoint owner binding mismatch",
            ));
        }
        let snapshot = StoredEnrollmentClock {
            ceremony: ceremony.clone(),
            subject: state.initiator_id,
            device: state
                .enrollment_device_id
                .ok_or_else(|| AuraError::invalid("enrollment clock has no device binding"))?,
            prestate: state.prestate_hash,
            epoch: state.new_epoch,
            budget: state.timeout_budget.clone(),
        };
        let bytes = serde_json::to_vec(&snapshot).map_err(|source| AuraError::Internal {
            message: "encode enrollment clock checkpoint".into(),
            source: Some(Arc::new(source)),
        })?;
        effects
            .secure_store(
                &SecureStorageLocation::new("enrollment_clock_v1", ceremony.to_string()),
                &bytes,
                &[SecureStorageCapability::Write],
            )
            .await
    }

    async fn restore_enrollment_clock(
        &self,
        registration: &mut TrackedCeremony,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("durable enrollment clock requires storage"))?;
        let bytes = effects
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "enrollment_clock_v1",
                    registration.ceremony_id.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized enrollment clock checkpoint"));
        }
        let record: StoredEnrollmentClock =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Internal {
                message: "decode retained enrollment clock".into(),
                source: Some(Arc::new(source)),
            })?;
        if record.ceremony != registration.ceremony_id
            || record.subject != registration.initiator_id
            || Some(record.device) != registration.enrollment_device_id
            || record.prestate != registration.prestate_hash
            || record.epoch != registration.new_epoch
            || record.budget.started_at_ms() != registration.timeout_budget.started_at_ms()
            || record.budget.deadline_at_ms() != registration.timeout_budget.deadline_at_ms()
        {
            return Err(AuraError::invalid(
                "retained enrollment clock generation mismatch",
            ));
        }
        registration.timeout_budget = record.budget;
        Ok(())
    }

    async fn observe_ceremony_window(
        &self,
        state: &TrackedCeremony,
        now: &PhysicalTime,
    ) -> Result<bool, AuraError> {
        let observation = state.timeout_budget.remaining_at(now);
        if state.kind == CeremonyKind::DeviceEnrollment && self.shared.persistence.is_some() {
            self.checkpoint_enrollment_clock(&state.ceremony_id).await?;
        }
        match observation {
            Ok(_) => Ok(false),
            Err(aura_core::TimeoutBudgetError::DeadlineExceeded { .. }) => Ok(true),
            Err(source) => Err(AuraError::from(source)),
        }
    }

    async fn persist_enrollment(&self, ceremony_id: &CeremonyId) -> Result<(), AuraError> {
        let state = self.get(ceremony_id).await?;
        self.persist_enrollment_snapshot(&state).await
    }

    async fn persist_enrollment_snapshot(&self, state: &TrackedCeremony) -> Result<(), AuraError> {
        let ceremony_id = &state.ceremony_id;
        let Some(effects) = &self.shared.persistence else {
            return Ok(());
        };
        if state.kind != CeremonyKind::DeviceEnrollment {
            return Ok(());
        }
        let record = StoredEnrollmentOutcome {
            ceremony_id: ceremony_id.clone(),
            started_at_ms: state.started_at.ts_ms,
            timeout_ms: u64::try_from(state.timeout.as_millis())
                .map_err(|_| AuraError::invalid("ceremony timeout overflow"))?,
            budget: state.timeout_budget.clone(),
            outcome: state.terminal_outcome,
        };
        let _guard = self.shared.persistence_guard.lock().await;
        let mut ids: Vec<CeremonyId> = effects
            .retrieve(ENROLLMENT_INDEX_KEY)
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "load enrollment index".into(),
                source: Some(std::sync::Arc::new(error)),
            })?
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(|error| aura_core::AuraError::Internal {
                message: "decode enrollment index".into(),
                source: Some(std::sync::Arc::new(error)),
            })?
            .unwrap_or_default();
        if !ids.contains(ceremony_id) {
            ids.push(ceremony_id.clone());
        }
        effects
            .store(
                &enrollment_record_key(ceremony_id),
                serde_json::to_vec(&record).map_err(|error| aura_core::AuraError::Internal {
                    message: "enrollment persistence codec".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?,
            )
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "store enrollment outcome".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        effects
            .store(
                ENROLLMENT_INDEX_KEY,
                serde_json::to_vec(&ids).map_err(|error| aura_core::AuraError::Internal {
                    message: "enrollment persistence codec".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?,
            )
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "store enrollment index".into(),
                source: Some(std::sync::Arc::new(error)),
            })
    }

    /// Enumerate enrollment results retained in durable runtime storage.
    pub async fn list_device_enrollment_ceremonies(&self) -> Result<Vec<CeremonyId>, AuraError> {
        let Some(effects) = &self.shared.persistence else {
            return Ok(self
                .shared
                .state
                .read()
                .await
                .ceremonies
                .values()
                .filter(|ceremony| ceremony.kind == CeremonyKind::DeviceEnrollment)
                .map(|ceremony| ceremony.ceremony_id.clone())
                .collect());
        };
        let bytes = effects
            .retrieve(ENROLLMENT_INDEX_KEY)
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "load enrollment index".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        bytes
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(|error| aura_core::AuraError::Internal {
                message: "decode enrollment index".into(),
                source: Some(std::sync::Arc::new(error)),
            })
            .map(|ids| ids.unwrap_or_default())
    }

    async fn stored_enrollment_outcome(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<Option<StoredEnrollmentOutcome>, AuraError> {
        if let Some(decision) = self.retained_terminal_decision(ceremony_id).await? {
            if decision.record.outcome == Some(CeremonyTerminalOutcome::Committed) {
                let effects = self
                    .shared
                    .persistence
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("missing receipt storage"))?;
                crate::handlers::invitation::enrollment_trust::recover_verified_response_receipt(
                    effects,
                    ceremony_id,
                )
                .await
                .map_err(|error| AuraError::Crypto {
                    message: "reverify first committed decision".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
            }
            return Ok(Some(decision.record));
        }
        let Some(effects) = &self.shared.persistence else {
            return Ok(None);
        };
        let bytes = effects
            .retrieve(&enrollment_record_key(ceremony_id))
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "load enrollment outcome".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        let record: Option<StoredEnrollmentOutcome> = bytes
            .map(|bytes| serde_json::from_slice(&bytes))
            .transpose()
            .map_err(|error| aura_core::AuraError::Internal {
                message: "decode enrollment outcome".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if record
            .as_ref()
            .is_some_and(|record| record.outcome == Some(CeremonyTerminalOutcome::Committed))
        {
            crate::handlers::invitation::enrollment_trust::recover_verified_response_receipt(
                effects,
                ceremony_id,
            )
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "reverify committed enrollment receipt".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        }
        Ok(record)
    }

    fn spawn_timeout_cleanup_task(
        &self,
        tasks: TaskGroup,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
    ) {
        const CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

        let tracker = self.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let _cleanup_task_handle = tasks.spawn_try_interval_until_named(
            "ceremony.timeout_cleanup",
            time_effects.clone(),
            CLEANUP_INTERVAL,
            move || {
                let tracker = tracker.clone();
                async move {
                    let cleaned = tracker.cleanup_timed_out().await?;
                    if cleaned > 0 {
                        tracing::debug!(
                            event = "runtime.service.ceremony.cleanup",
                            cleaned,
                            "Cleaned timed-out ceremonies"
                        );
                    }
                    Ok(true)
                }
            },
        );
        #[cfg(target_arch = "wasm32")]
        let _cleanup_task_handle = tasks.spawn_local_try_interval_until_named(
            "ceremony.timeout_cleanup",
            time_effects,
            CLEANUP_INTERVAL,
            move || {
                let tracker = tracker.clone();
                async move {
                    let cleaned = tracker.cleanup_timed_out().await?;
                    if cleaned > 0 {
                        tracing::debug!(
                            event = "runtime.service.ceremony.cleanup",
                            cleaned,
                            "Cleaned timed-out ceremonies"
                        );
                    }
                    Ok(true)
                }
            },
        );
    }

    /// Register a new ceremony with explicit prestate hash for supersession tracking.
    #[allow(clippy::too_many_arguments)]
    pub async fn register(
        &self,
        ceremony_id: CeremonyId,
        kind: CeremonyKind,
        initiator_id: AuthorityId,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
        enrollment_nickname_suggestion: Option<String>,
        prestate_hash: Hash32,
    ) -> Result<(), AuraError> {
        self.register_with_generation(
            None,
            ceremony_id,
            kind,
            initiator_id,
            threshold_k,
            total_n,
            participants,
            new_epoch,
            enrollment_device_id,
            enrollment_nickname_suggestion,
            prestate_hash,
        )
        .await
    }

    /// Persistent enrollment allocation requires its still-held physical owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "register_with_generation",
        capability_type = EnrollmentGenerationReservation,
        family = "runtime_helper"
    )]
    pub(crate) async fn register_owned_device_enrollment(
        &self,
        generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        request: super::ceremony_runner::CeremonyInitRequest,
    ) -> Result<(), AuraError> {
        if request.kind != CeremonyKind::DeviceEnrollment || self.shared.persistence.is_none() {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        }
        self.register_with_generation(
            Some(generation),
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

    #[allow(clippy::too_many_arguments)]
    async fn register_with_generation(
        &self,
        generation: Option<&crate::runtime::effects::EnrollmentGenerationReservation<'_>>,
        ceremony_id: CeremonyId,
        kind: CeremonyKind,
        initiator_id: AuthorityId,
        threshold_k: u16,
        total_n: u16,
        participants: Vec<ParticipantIdentity>,
        new_epoch: u64,
        enrollment_device_id: Option<DeviceId>,
        enrollment_nickname_suggestion: Option<String>,
        prestate_hash: Hash32,
    ) -> Result<(), AuraError> {
        if kind == CeremonyKind::DeviceEnrollment
            && self.shared.persistence.is_some()
            && generation.is_none()
        {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        }
        let _decision = match generation {
            Some(owned) => {
                owned.require_tracker(self)?;
                None
            }
            None => Some(self.shared.enrollment_decision_gate.lock().await),
        };
        if kind == CeremonyKind::DeviceEnrollment
            && self.shared.persistence.is_some()
            && self
                .retained_terminal_decision(&ceremony_id)
                .await?
                .is_some()
        {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
            ));
        }
        if kind == CeremonyKind::DeviceEnrollment {
            if let Some(effects) = &self.shared.persistence {
                if effects
                    .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                        "device_enrollment_orphan_retirement_v1",
                        ceremony_id.to_string(),
                    ))
                    .await?
                {
                    return Err(AuraError::invalid(
                        "retired allocation ceremony cannot be registered",
                    ));
                }

                if crate::handlers::invitation::enrollment_trust::has_response_receipt(
                    effects,
                    &ceremony_id,
                )
                .await
                .map_err(|error| aura_core::AuraError::Internal {
                    message: "read enrollment response receipt".into(),
                    source: Some(std::sync::Arc::new(error)),
                })? {
                    return Err(AuraError::invalid(
                        "enrollment ceremony ID already has a durable decision",
                    ));
                }
            }
        }
        {
            let registry = self.shared.state.read().await;
            if registry.ceremonies.contains_key(&ceremony_id)
                || registry.retired_enrollment_ids.contains(&ceremony_id)
            {
                return Err(AuraError::invalid(
                    "enrollment ceremony generation already consumed",
                ));
            }
        }
        let participants_set: HashSet<_> = participants.into_iter().collect();
        if participants_set.len() != total_n as usize {
            return Err(AuraError::invalid(format!(
                "Ceremony {} participant count {} does not match total_n {}",
                ceremony_id,
                participants_set.len(),
                total_n
            )));
        }

        // Get current time from injected effect for deterministic simulation support
        let now = self
            .time
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::Internal {
                message: "Failed to get current time".into(),
                source: Some(std::sync::Arc::new(e)),
            })?;

        let mut state = TrackedCeremony {
            ceremony_id: ceremony_id.clone(),
            kind,
            initiator_id,
            threshold_k,
            total_n,
            participants: participants_set,
            accepted_participants: HashSet::new(),
            new_epoch,
            enrollment_device_id,
            enrollment_nickname_suggestion,
            started_at: now.clone(),
            has_failed: false,
            is_committed: false,
            is_superseded: false,
            superseded_by: None,
            supersedes: Vec::new(),
            agreement_mode: Self::initial_mode_for_kind(kind),
            error_message: None,
            terminal_outcome: None,
            failure_reason: None,
            timeout: Self::timeout_for_kind(kind),
            enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(
                &now,
                Self::timeout_for_kind(kind),
            )
            .map_err(AuraError::from)?,
            prestate_hash,
            committed_at: None,
            committed_consensus_id: None,
        };

        if state.kind == CeremonyKind::DeviceEnrollment {
            if let Some(effects) = &self.shared.persistence {
                state = crate::handlers::invitation::enrollment_trust::persist_allocated_enrollment_registration(effects, generation.ok_or_else(|| crate::runtime::effects::held_registration_error(crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner))?, &state)
            .await.map_err(|source| AuraError::Internal { message: "retain original allocated enrollment registration".into(), source: Some(Arc::new(source)) })?;
                self.prepare_initial_enrollment_clock(&mut state).await?;
                self.admit_allocated_enrollment_clock(
                    generation.ok_or_else(|| {
                        crate::runtime::effects::held_registration_error(
                            crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
                        )
                    })?,
                    &mut state,
                    &now,
                )
                .await?;
            }
        }
        self.persist_enrollment_snapshot(&state).await?;
        if state.kind == CeremonyKind::DeviceEnrollment && self.shared.persistence.is_some() {
            self.retain_live_enrollment_window(&state).await?;
        }
        let result = with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                if tracker.retired_enrollment_ids.contains(&ceremony_id) {
                    return Err(AuraError::invalid(
                        "retired enrollment ceremony ID cannot be reused",
                    ));
                }
                if tracker.ceremonies.contains_key(&ceremony_id) {
                    return Err(AuraError::invalid(format!(
                        "Ceremony {} already registered",
                        ceremony_id
                    )));
                }
                tracker.ceremonies.insert(ceremony_id.clone(), state);
                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await;

        if result.is_ok() {
            tracing::info!(
                ceremony_id = %ceremony_id,
                threshold_k,
                total_n,
                "Ceremony registered"
            );
        }

        result
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentGenerationCapability",
        family = "runtime_helper"
    )]
    pub(super) async fn acquire_registered_enrollment_generation_window(
        &self,
        generation: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
    ) -> Result<RegisteredEnrollmentWindowCapability, AuraError> {
        generation.require_tracker(self)?;
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("registered generation requires its actual persistent runtime")
        })?;
        generation.require_effects(effects.as_ref())?;
        let crate::handlers::invitation::InvitationType::DeviceEnrollment { ceremony_id, .. } =
            &generation.canonical_invitation().invitation_type
        else {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::InvitationKind,
            ));
        };
        self.acquire_registered_enrollment_window_inner(ceremony_id, generation)
            .await
    }

    async fn acquire_registered_enrollment_window_inner(
        &self,
        ceremony: &CeremonyId,
        generation: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
    ) -> Result<RegisteredEnrollmentWindowCapability, AuraError> {
        self.require_enrollment_clock_storage()?;
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let state = self.shared.state.read().await;
        let registered = state
            .ceremonies
            .get(ceremony)
            .ok_or_else(|| AuraError::invalid("enrollment ceremony is not registered"))?;
        if registered.kind != CeremonyKind::DeviceEnrollment
            || registered.terminal_outcome.is_some()
            || registered.is_superseded
            || registered.threshold_k == 0
            || registered.threshold_k > registered.total_n
            || registered.enrollment_device_id.is_none()
            || registered.timeout_budget.started_at_ms() != registered.started_at.ts_ms
            || u64::try_from(registered.timeout.as_millis()).ok()
                != Some(registered.timeout_budget.timeout_ms())
        {
            return Err(AuraError::invalid(
                "enrollment execution requires the original active registered generation",
            ));
        }
        {
            let canonical = generation.canonical_invitation();
            let crate::handlers::invitation::InvitationType::DeviceEnrollment {
                setup_binding: Some(binding),
                ..
            } = &canonical.invitation_type
            else {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                ));
            };
            let effects = self
                .shared
                .persistence
                .as_ref()
                .ok_or_else(|| AuraError::invalid("missing original generation runtime"))?;
            let retained = crate::handlers::invitation::enrollment_trust::verify_generation_registration_binding(effects.as_ref(), registered.initiator_id, registered.new_epoch, &registered.ceremony_id, registered.prestate_hash, &canonical.invitation_id, binding.digest).await.map_err(|source| AuraError::Internal { message: "verify original canonical registered execution owner".into(), source: Some(Arc::new(source)) })?;
            let encode = |invitation: &crate::handlers::invitation::Invitation| {
                serde_json::to_vec(invitation).map_err(|source| AuraError::Serialization {
                    message: "compare original canonical registered invitation".into(),
                    source: Some(Arc::new(source)),
                })
            };
            if encode(&retained)? != encode(canonical)? {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                ));
            }
        }
        let lease = registered
            .enrollment_window_lease
            .clone()
            .try_acquire_owned()
            .map_err(registered_window_lease_error)?;
        let registered = registered.clone();
        drop(state);
        let mut retained = registered.clone();
        self.require_live_enrollment_window(&registered).await?;
        self.restore_enrollment_clock(&mut retained).await?;
        Ok(RegisteredEnrollmentWindowCapability {
            tracker: self.clone(),
            state: registered,
            lease: Arc::new(lease),
            notice_binding: std::sync::OnceLock::new(),
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_cancellation_observation",
        capability_type = RegisteredCancellationPreparationCapability,
        family = "runtime_helper"
    )]
    pub(super) async fn prepare_original_cancellation_observation(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<RegisteredCancellationPreparationCapability, AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("cancellation observation requires persistent runtime owner")
        })?;
        issued.require_runtime_owner(effects)?;
        let manifest = issued.manifest();
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let registry = self.shared.state.read().await;
        let state = registry.ceremonies.get(&manifest.ceremony).ok_or_else(|| {
            AuraError::invalid("original cancellation allocation is not registered")
        })?;
        if state.kind != CeremonyKind::DeviceEnrollment
            || state.initiator_id != manifest.subject
            || state.enrollment_device_id != Some(manifest.invitee_device)
            || state.new_epoch != manifest.pending_epoch
            || state.is_superseded
        {
            return Err(AuraError::invalid(
                "cancellation observation belongs to another generation",
            ));
        }
        self.require_live_enrollment_window(state).await?;
        let mut durable = state.clone();
        self.restore_enrollment_clock(&mut durable).await?;
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&durable.timeout_budget)?;
        if let Some(outcome) = &state.terminal_outcome {
            if *outcome != CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled) {
                return Err(AuraError::invalid(
                    "cancellation has another terminal decision",
                ));
            }
            return Ok(RegisteredCancellationPreparationCapability::Decided(
                VerifiedEnrollmentCancellationCapability {
                    runtime_owner: effects.clone(),
                    invitation: manifest.invitation.clone(),
                    ceremony: manifest.ceremony.clone(),
                },
            ));
        }
        Ok(RegisteredCancellationPreparationCapability::Active(
            Box::new(CancellationClockObservationCapability {
                tracker: self.clone(),
                state: state.clone(),
                signed_expiry_ms: manifest.expires_at_ms,
            }),
        ))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredCancelledNoticeCapability",
        family = "runtime_helper"
    )]
    pub(super) async fn acquire_cancelled_notice(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<RegisteredCancelledNoticeCapability, AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("cancelled notice requires persistent runtime ownership")
        })?;
        issued.require_runtime_owner(effects.as_ref())?;
        let manifest = issued.manifest();
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let registry = self.shared.state.read().await;
        let state = registry
            .ceremonies
            .get(&manifest.ceremony)
            .ok_or_else(|| AuraError::invalid("cancelled notice allocation is not registered"))?;
        let expected = CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled);
        if state.kind != CeremonyKind::DeviceEnrollment
            || state.terminal_outcome != Some(expected)
            || state.initiator_id != manifest.subject
            || state.enrollment_device_id != Some(manifest.invitee_device)
            || state.new_epoch != manifest.pending_epoch
            || state.is_superseded
        {
            return Err(AuraError::invalid(
                "notice requires exact original cancelled generation",
            ));
        }
        let durable_decision = self
            .retained_terminal_decision(&manifest.ceremony)
            .await?
            .ok_or_else(|| {
                AuraError::invalid("cancelled notice has no immutable first decision")
            })?;
        if durable_decision.prestate != state.prestate_hash
            || durable_decision.record.outcome != Some(expected)
            || durable_decision.record.started_at_ms != state.started_at.ts_ms
            || durable_decision.record.timeout_ms != state.timeout_budget.timeout_ms()
        {
            return Err(AuraError::invalid(
                "cancelled notice first decision changed allocation",
            ));
        }
        self.require_live_enrollment_window(state).await?;
        let mut retained = state.clone();
        self.restore_enrollment_clock(&mut retained).await?;
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&retained.timeout_budget)?;
        // Exactly one finite sender may own this original allocation. No wait
        // while holding decision custody and no fresh execution semaphore.
        let lease = state
            .enrollment_window_lease
            .clone()
            .try_acquire_owned()
            .map_err(registered_window_lease_error)?;
        Ok(RegisteredCancelledNoticeCapability {
            observation: CancellationClockObservationCapability {
                tracker: self.clone(),
                state: state.clone(),
                signed_expiry_ms: manifest.expires_at_ms,
            },
            lease: Arc::new(lease),
            cancelled: VerifiedEnrollmentCancellationCapability {
                runtime_owner: effects.clone(),
                invitation: manifest.invitation.clone(),
                ceremony: manifest.ceremony.clone(),
            },
            manifest_digest: issued.digest(),
        })
    }

    pub async fn get(&self, ceremony_id: &CeremonyId) -> Result<TrackedCeremony, AuraError> {
        let state = self.shared.state.read().await;

        state
            .ceremonies
            .get(ceremony_id)
            .cloned()
            .ok_or_else(|| AuraError::not_found(format!("Ceremony {} not found", ceremony_id)))
    }

    /// Get ceremony status for UI display
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// The ceremony status for UI display
    pub async fn get_status(&self, ceremony_id: &CeremonyId) -> Result<CeremonyStatus, AuraError> {
        self.get(ceremony_id).await.map(|c| c.to_status())
    }

    /// Mark a guardian as having accepted the invitation
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    /// * `guardian_id` - The guardian who accepted
    ///
    /// # Returns
    /// True if threshold is now reached
    /// Record the invitation verifier's sealed proof for the exact enrolling device.
    pub(crate) async fn record_verified_enrollment_response(
        &self,
        evidence: crate::handlers::invitation::VerifiedEnrollmentResponse,
    ) -> Result<bool, aura_core::AuraError> {
        let ceremony_id = evidence.ceremony_id().clone();
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let current =
            self.get(&ceremony_id)
                .await
                .map_err(|error| aura_core::AuraError::Internal {
                    message: "read enrollment ceremony".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
        if current.kind != CeremonyKind::DeviceEnrollment
            || current.initiator_id != evidence.subject()
            || current.enrollment_device_id != Some(evidence.device_id())
            || current.new_epoch != evidence.pending_epoch()
        {
            return Err(aura_core::AuraError::invalid(
                "verified enrollment response generation mismatch",
            ));
        }
        if current.terminal_outcome.is_some()
            && !self
                .shared
                .state
                .read()
                .await
                .enrollment_responses
                .contains_key(&ceremony_id)
        {
            return Err(aura_core::AuraError::invalid(
                "cannot admit first enrollment response after terminal decision",
            ));
        }
        if let Some(effects) = &self.shared.persistence {
            crate::handlers::invitation::enrollment_trust::persist_verified_response_receipt(
                effects, &evidence, &current,
            )
            .await
            .map_err(|error| aura_core::AuraError::Storage {
                message: "persist verified enrollment response receipt".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        }
        let result = with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                if let Some(existing) = tracker.enrollment_responses.get(&ceremony_id) {
                    if existing.invitation_id() != evidence.invitation_id()
                        || existing.setup_digest() != evidence.setup_digest()
                        || existing.subject() != evidence.subject()
                        || existing.device_id() != evidence.device_id()
                        || existing.pending_epoch() != evidence.pending_epoch()
                        || existing.acceptance().acceptor_id != evidence.acceptance().acceptor_id
                        || existing.acceptance().manifest_digest
                            != evidence.acceptance().manifest_digest
                    {
                        return Err(AuraError::invalid(
                            "conflicting enrollment response; first verified proof is retained",
                        ));
                    }
                    let state = tracker
                        .ceremonies
                        .get(&ceremony_id)
                        .ok_or_else(|| AuraError::invalid("orphan enrollment response"))?;
                    return Ok(state.accepted_participants.len() >= usize::from(state.threshold_k));
                }
                let state = tracker
                    .ceremonies
                    .get_mut(&ceremony_id)
                    .ok_or_else(|| AuraError::invalid("unknown enrollment ceremony"))?;
                if state.kind != CeremonyKind::DeviceEnrollment
                    || state.initiator_id != evidence.subject()
                    || state.enrollment_device_id != Some(evidence.device_id())
                    || state.new_epoch != evidence.pending_epoch()
                    || state.terminal_outcome.is_some()
                {
                    return Err(AuraError::invalid(
                        "verified enrollment response binding mismatch",
                    ));
                }
                let participant = ParticipantIdentity::device(evidence.device_id());
                if !state.participants.contains(&participant) {
                    return Err(AuraError::invalid(
                        "verified enrollment device is not invited",
                    ));
                }
                state.accepted_participants.insert(participant);
                let ready = state.accepted_participants.len() >= usize::from(state.threshold_k);
                tracker
                    .enrollment_responses
                    .insert(ceremony_id.clone(), evidence);
                Ok(ready)
            },
            |tracker| tracker.validate(),
        )
        .await
        .map_err(|error| aura_core::AuraError::Internal {
            message: "record verified enrollment response".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        Ok(result)
    }

    /// Restore an active registration only from secure raw receipt evidence
    /// reverified under the original retained setup verifier. This restores
    /// ceremony ownership; pending signing-generation restoration is separate.
    pub(crate) async fn restore_verified_enrollment_registration(
        &self,
        ceremony_id: &CeremonyId,
        signing: &crate::runtime::services::threshold_signing::ThresholdSigningService,
    ) -> Result<bool, aura_core::AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        if self
            .shared
            .state
            .read()
            .await
            .ceremonies
            .contains_key(ceremony_id)
        {
            return Ok(false);
        }
        let effects =
            self.shared.persistence.as_ref().ok_or_else(|| {
                aura_core::AuraError::invalid("no durable enrollment receipt owner")
            })?;
        let (proof, mut registration) =
            if crate::handlers::invitation::enrollment_trust::has_response_receipt(
                effects,
                ceremony_id,
            )
            .await
            .map_err(|error| aura_core::AuraError::Internal {
                message: "recover enrollment evidence".into(),
                source: Some(std::sync::Arc::new(error)),
            })? {
                let (proof, registration) = crate::handlers::invitation::enrollment_trust::recover_verified_response_receipt(effects, ceremony_id)
                .await.map_err(|error| aura_core::AuraError::Internal { message: "recover enrollment evidence".into(), source: Some(std::sync::Arc::new(error)) })?;
                (Some(proof), registration)
            } else {
                let registration = crate::handlers::invitation::enrollment_trust::recover_pending_enrollment_registration(effects, ceremony_id)
                .await.map_err(|error| aura_core::AuraError::Internal { message: "recover enrollment evidence".into(), source: Some(std::sync::Arc::new(error)) })?;
                (None, registration)
            };
        let outcome = self
            .stored_enrollment_outcome(ceremony_id)
            .await
            .map_err(|error| aura_core::AuraError::Storage {
                message: "load enrollment first terminal decision".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if let Some(decision) = self.retained_terminal_decision(ceremony_id).await? {
            let accepted: HashSet<_> = decision.accepted.iter().cloned().collect();
            if decision.prestate != registration.prestate_hash
                || accepted.len() != decision.accepted.len()
                || !accepted.is_subset(&registration.participants)
                || decision.record.started_at_ms != registration.started_at.ts_ms
                || decision.record.timeout_ms
                    != u64::try_from(registration.timeout.as_millis())
                        .map_err(|_| AuraError::invalid("restore timeout overflow"))?
            {
                return Err(AuraError::invalid(
                    "terminal enrollment generation mismatch",
                ));
            }
            registration.accepted_participants = accepted;
        }
        if let Some(record) = outcome.as_ref() {
            if record.budget.started_at_ms() != registration.started_at.ts_ms
                || record.budget.timeout_ms() != record.timeout_ms
                || record.budget.timeout_ms() != registration.timeout_budget.timeout_ms()
            {
                return Err(AuraError::invalid("retained timeout window mismatch"));
            }
            registration.timeout_budget = record.budget.clone();
        }
        if let Some(outcome) = outcome.and_then(|record| record.outcome) {
            registration.terminal_outcome = Some(outcome);
            match outcome {
                CeremonyTerminalOutcome::Committed => {
                    registration.is_committed = true;
                    registration.agreement_mode = AgreementMode::ConsensusFinalized;
                }
                CeremonyTerminalOutcome::Failed(reason) => {
                    registration.has_failed = true;
                    registration.failure_reason = Some(reason);
                }
            }
        }
        if registration.is_committed
            && registration.accepted_participants.len() < usize::from(registration.threshold_k)
        {
            return Err(aura_core::AuraError::invalid(
                "committed receipt lacks durable participant progress",
            ));
        }
        self.restore_enrollment_clock(&mut registration).await?;
        self.retain_live_enrollment_window(&registration).await?;
        if registration.terminal_outcome.is_none() {
            crate::handlers::invitation::enrollment_trust::restore_pending_signing_generation(
                effects,
                signing,
                ceremony_id,
                registration.initiator_id,
                registration.new_epoch,
                registration.prestate_hash,
            )
            .await
            .map_err(|error| AuraError::Crypto {
                message: "restore exact pending signing generation before registration".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        }
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                tracker.ceremonies.insert(ceremony_id.clone(), registration);
                if let Some(proof) = proof {
                    tracker
                        .enrollment_responses
                        .insert(ceremony_id.clone(), proof);
                }
                Ok::<bool, AuraError>(true)
            },
            |tracker| tracker.validate(),
        )
        .await
        .map_err(|error| aura_core::AuraError::Internal {
            message: "restore enrollment registry generation".into(),
            source: Some(std::sync::Arc::new(error)),
        })
    }

    /// A response count never substitutes for ceremony-bound enrollment proof.
    pub(crate) async fn require_verified_enrollment_response(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<(), AuraError> {
        let tracker = self.shared.state.read().await;
        let state = tracker
            .ceremonies
            .get(ceremony_id)
            .ok_or_else(|| AuraError::invalid("unknown enrollment ceremony"))?;
        if state.kind != CeremonyKind::DeviceEnrollment {
            return Ok(());
        }
        let proof = tracker
            .enrollment_responses
            .get(ceremony_id)
            .ok_or_else(|| {
                AuraError::invalid(
                    "missing verified enrollment response; reverify retained proof after restart",
                )
            })?;
        if proof.ceremony_id() != ceremony_id
            || proof.subject() != state.initiator_id
            || Some(proof.device_id()) != state.enrollment_device_id
            || proof.pending_epoch() != state.new_epoch
        {
            return Err(AuraError::invalid(
                "verified enrollment response binding mismatch",
            ));
        }
        Ok(())
    }

    pub async fn mark_accepted(
        &self,
        ceremony_id: &CeremonyId,
        participant: ParticipantIdentity,
    ) -> Result<bool, AuraError> {
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    AuraError::not_found(format!("Ceremony {} not found", ceremony_id))
                })?;

                if state.terminal_outcome.is_some() {
                    return Err(AuraError::invalid(format!(
                        "Ceremony {} already has a terminal outcome",
                        ceremony_id
                    )));
                }

                if state.kind == CeremonyKind::DeviceEnrollment
                    && state
                        .enrollment_device_id
                        .map(ParticipantIdentity::device)
                        .as_ref()
                        == Some(&participant)
                {
                    return Err(AuraError::invalid(
                        "enrolling device requires sealed verified enrollment response",
                    ));
                }

                // Check if participant is part of this ceremony
                if !state.participants.contains(&participant) {
                    return Err(AuraError::invalid(format!(
                        "Participant {:?} not part of ceremony {}",
                        participant, ceremony_id
                    )));
                }

                // Check if already accepted
                if state.accepted_participants.contains(&participant) {
                    tracing::debug!(
                        ceremony_id = %ceremony_id,
                        "Participant already accepted (idempotent)"
                    );
                    return Ok(state.accepted_participants.len() >= state.threshold_k as usize);
                }

                // Add to accepted list
                state.accepted_participants.insert(participant.clone());

                let threshold_reached =
                    state.accepted_participants.len() >= state.threshold_k as usize;
                if threshold_reached {
                    state.agreement_mode = AgreementMode::CoordinatorSoftSafe;
                }

                tracing::info!(
                    ceremony_id = %ceremony_id,
                    accepted = state.accepted_participants.len(),
                    threshold = state.threshold_k,
                    threshold_reached,
                    "Participant accepted ceremony"
                );

                Ok(threshold_reached)
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Mark a ceremony as committed (key rotation activated), with optional metadata.
    pub async fn mark_committed_with_metadata(
        &self,
        ceremony_id: &CeremonyId,
        committed_at: Option<PhysicalTime>,
        consensus_id: Option<ConsensusId>,
    ) -> Result<(), AuraError> {
        self.complete(ceremony_id, CeremonyTerminalOutcome::Committed)
            .await?;
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    AuraError::not_found(format!("Ceremony {} not found", ceremony_id))
                })?;

                if let Some(committed_at) = committed_at {
                    state.committed_at = Some(committed_at);
                }
                if let Some(consensus_id) = consensus_id {
                    state.committed_consensus_id = Some(consensus_id);
                }

                tracing::info!(
                    ceremony_id = %ceremony_id,
                    accepted = state.accepted_participants.len(),
                    threshold = state.threshold_k,
                    "Ceremony committed"
                );

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Mark a ceremony as committed (key rotation activated).
    ///
    /// This is only called after threshold is reached and `commit_key_rotation` succeeds.
    pub async fn mark_committed(&self, ceremony_id: &CeremonyId) -> Result<(), AuraError> {
        self.complete(ceremony_id, CeremonyTerminalOutcome::Committed)
            .await
            .map(|_| ())
    }

    /// Set the sole terminal outcome. Repeating the same outcome is idempotent;
    /// a conflicting outcome is rejected without mutating the original result.
    pub(crate) async fn verified_enrollment_response(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<crate::handlers::invitation::VerifiedEnrollmentResponse, aura_core::AuraError> {
        self.shared
            .state
            .read()
            .await
            .enrollment_responses
            .get(ceremony_id)
            .cloned()
            .ok_or_else(|| aura_core::AuraError::invalid("missing enrollment response capability"))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_activation",
        capability_type = EnrollmentActivationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn begin_enrollment_activation(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<Option<EnrollmentActivationCapability<'_>>, aura_core::AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            )
        })?;
        let custody = self
            .acquire_enrollment_generation_decision(effects.as_ref())
            .await?;
        let prepared = if let Some(effects) = &self.shared.persistence {
            effects
                .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                    "device_enrollment_activation_v1",
                    ceremony_id.to_string(),
                ))
                .await?
        } else {
            false
        };
        let now =
            self.time
                .physical_time()
                .await
                .map_err(|error| aura_core::AuraError::Internal {
                    message: "read activation admission time".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
        let current_snapshot = self.get(ceremony_id).await?;
        if current_snapshot.terminal_outcome.is_none() {
            let effects = self.shared.persistence.as_ref().ok_or_else(|| {
                AuraError::invalid("activation requires durable generation ownership")
            })?;
            effects
                .require_registered_enrollment_profile(
                    current_snapshot.initiator_id,
                    current_snapshot.new_epoch,
                    ceremony_id,
                    current_snapshot.prestate_hash,
                )
                .await?;
        }
        let _expired = self
            .observe_ceremony_window(&current_snapshot, &now)
            .await?;
        let state = self.shared.state.read().await;

        let current = state
            .ceremonies
            .get(ceremony_id)
            .ok_or_else(|| aura_core::AuraError::invalid("unknown enrollment activation"))?;
        if current.kind != CeremonyKind::DeviceEnrollment {
            return Err(aura_core::AuraError::invalid(
                "activation lease requires enrollment",
            ));
        }
        if current.terminal_outcome == Some(CeremonyTerminalOutcome::Committed) {
            return Ok(None);
        }
        if current.terminal_outcome.is_some() || current.is_superseded {
            return Err(aura_core::AuraError::invalid(
                "enrollment activation has terminated",
            ));
        }
        let expired = current.timeout_budget.remaining_at(&now);
        if let Err(source) = &expired {
            if !matches!(
                source,
                aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
            ) {
                return Err(AuraError::from(source.clone()));
            }
        }
        if !prepared {
            expired.map_err(|source| AuraError::Internal {
                message: "enrollment activation deadline elapsed".into(),
                source: Some(Arc::new(source)),
            })?;
        }
        let proof = state
            .enrollment_responses
            .get(ceremony_id)
            .ok_or_else(|| aura_core::AuraError::invalid("missing enrollment activation proof"))?;
        if proof.ceremony_id() != ceremony_id
            || proof.subject() != current.initiator_id
            || Some(proof.device_id()) != current.enrollment_device_id
            || proof.pending_epoch() != current.new_epoch
            || current.threshold_k == 0
            || current.accepted_participants.len() < usize::from(current.threshold_k)
        {
            return Err(aura_core::AuraError::invalid(
                "enrollment activation binding or threshold mismatch",
            ));
        }
        drop(state);
        Ok(Some(EnrollmentActivationCapability {
            tracker: self,
            ceremony_id: ceremony_id.clone(),
            custody,
        }))
    }

    /// Verified refusal settles the same original registered generation under
    /// the terminal gate; it cannot add an accepted participant or activate keys.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_rejection",
        capability_type = VerifiedEnrollmentRejectionCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn record_verified_enrollment_rejection(
        &self,
        proof: crate::handlers::invitation::enrollment_trust::VerifiedEnrollmentRejectionCapability,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let state = self.get(proof.ceremony_id()).await?;
        if state.kind != CeremonyKind::DeviceEnrollment
            || state.initiator_id != proof.subject()
            || state.enrollment_device_id != Some(proof.device_id())
            || state.new_epoch != proof.pending_epoch()
        {
            return Err(AuraError::invalid(
                "verified rejection belongs to another enrollment generation",
            ));
        }
        if let Some(outcome) = state.terminal_outcome {
            return if outcome == CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Rejected) {
                Ok(outcome)
            } else {
                Err(AuraError::invalid(
                    "contradictory verified enrollment rejection",
                ))
            };
        }
        let now = self
            .time
            .physical_time()
            .await
            .map_err(|source| AuraError::Internal {
                message: "required rejection clock failed".into(),
                source: Some(Arc::new(source)),
            })?;
        let active = state.timeout_budget.remaining_at(&now);
        self.checkpoint_enrollment_clock(proof.ceremony_id())
            .await?;
        active.map_err(AuraError::from)?;
        self.complete_under_decision_gate(
            proof.ceremony_id(),
            CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Rejected),
        )
        .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "registered_enrollment_cancellation",
        capability_type = VerifiedEnrollmentCancellationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn cancel_verified_enrollment(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<VerifiedEnrollmentCancellationCapability, AuraError> {
        let runtime_owner = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("issued enrollment cancellation requires persistent owner")
        })?;
        issued.require_runtime_owner(runtime_owner.as_ref())?;
        let manifest = issued.manifest();

        let _decision = self.shared.enrollment_decision_gate.lock().await;
        let state = self.get(&manifest.ceremony).await?;
        if state.kind != CeremonyKind::DeviceEnrollment
            || state.initiator_id != manifest.subject
            || state.enrollment_device_id != Some(manifest.invitee_device)
            || state.new_epoch != manifest.pending_epoch
        {
            return Err(AuraError::invalid(
                "issued cancellation belongs to another generation",
            ));
        }
        if let Some(outcome) = state.terminal_outcome {
            if outcome != CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled) {
                return Err(AuraError::invalid(
                    "enrollment already has a different terminal decision",
                ));
            }
        } else {
            let lease = state.timeout_budget.acquire_observation().await;
            let now = self
                .time
                .physical_time()
                .await
                .map_err(|source| AuraError::Internal {
                    message: "required enrollment cancellation clock".into(),
                    source: Some(Arc::new(source)),
                })?;
            let remaining = state.timeout_budget.remaining_at(&now);
            self.checkpoint_enrollment_clock(&manifest.ceremony).await?;
            remaining.map_err(AuraError::from)?;
            if now.ts_ms >= manifest.expires_at_ms {
                return Err(AuraError::Invalid {
                    message: "issued enrollment manifest validity ended before cancellation".into(),
                    source: Some(Arc::new(
                        aura_invitation::enrollment_manifest::EnrollmentManifestError::Expired,
                    )),
                });
            }

            drop(lease);
            self.complete_under_decision_gate(
                &manifest.ceremony,
                CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled),
            )
            .await?;
        }
        Ok(VerifiedEnrollmentCancellationCapability {
            runtime_owner: runtime_owner.clone(),
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
        })
    }

    pub async fn complete(
        &self,
        ceremony_id: &CeremonyId,
        outcome: CeremonyTerminalOutcome,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        self.complete_under_decision_gate(ceremony_id, outcome)
            .await
    }

    async fn complete_under_decision_gate(
        &self,
        ceremony_id: &CeremonyId,
        outcome: CeremonyTerminalOutcome,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        if matches!(outcome, CeremonyTerminalOutcome::Failed(_)) {
            if let Some(effects) = &self.shared.persistence {
                if effects
                    .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                        "device_enrollment_activation_v1",
                        ceremony_id.to_string(),
                    ))
                    .await?
                {
                    return Err(AuraError::invalid("prepared enrollment requires activation reconciliation before cancellation"));
                }
            }
        }
        let current = self.get(ceremony_id).await?;
        if let Some(existing) = current.terminal_outcome {
            if existing != outcome {
                return Err(AuraError::invalid(
                    "contradictory enrollment terminal outcome",
                ));
            }
        }
        if current.kind == CeremonyKind::DeviceEnrollment
            && outcome == CeremonyTerminalOutcome::Committed
        {
            self.require_verified_enrollment_response(ceremony_id)
                .await?;
            if current.threshold_k == 0
                || current.accepted_participants.len() < usize::from(current.threshold_k)
            {
                return Err(AuraError::invalid(
                    "cannot persist enrollment commit before threshold",
                ));
            }
        }
        // Required secure first-decision write precedes visible terminal publication.
        self.persist_first_enrollment_terminal_decision(&current, outcome)
            .await?;
        let result = with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                if outcome == CeremonyTerminalOutcome::Committed {
                    let current = tracker
                        .ceremonies
                        .get(ceremony_id)
                        .ok_or_else(|| AuraError::invalid("unknown enrollment ceremony"))?;
                    if current.kind == CeremonyKind::DeviceEnrollment {
                        let proof =
                            tracker
                                .enrollment_responses
                                .get(ceremony_id)
                                .ok_or_else(|| {
                                    AuraError::invalid("missing verified enrollment response")
                                })?;
                        if proof.ceremony_id() != ceremony_id
                            || proof.subject() != current.initiator_id
                            || Some(proof.device_id()) != current.enrollment_device_id
                            || proof.pending_epoch() != current.new_epoch
                        {
                            return Err(AuraError::invalid(
                                "verified enrollment response binding mismatch",
                            ));
                        }
                    }
                }
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    AuraError::not_found(format!("Ceremony {} not found", ceremony_id))
                })?;
                if let Some(existing) = state.terminal_outcome {
                    return if existing == outcome {
                        Ok(existing)
                    } else {
                        Err(AuraError::invalid(format!(
                            "Ceremony {} already completed with {:?}",
                            ceremony_id, existing
                        )))
                    };
                }
                match outcome {
                    CeremonyTerminalOutcome::Committed => {
                        if state.accepted_participants.len() < state.threshold_k as usize {
                            return Err(AuraError::invalid(format!(
                                "Ceremony {} cannot commit before threshold",
                                ceremony_id
                            )));
                        }
                        state.is_committed = true;
                        state.agreement_mode = AgreementMode::ConsensusFinalized;
                    }
                    CeremonyTerminalOutcome::Failed(reason) => {
                        state.has_failed = true;
                        state.failure_reason = Some(reason);
                    }
                }
                state.terminal_outcome = Some(outcome);
                Ok(outcome)
            },
            |tracker| tracker.validate(),
        )
        .await?;
        self.shared.terminal_changed.notify_waiters();
        if let Err(error) = self.persist_enrollment(ceremony_id).await {
            tracing::error!(ceremony_id = %ceremony_id, %error, "enrollment observation index refresh failed after durable terminal decision");
        }

        Ok(result)
    }

    /// Wait for the actual enrollment owner's terminal publication. The caller
    /// retains its effect-backed budget; pending state is not a timeout error.
    pub(crate) async fn await_enrollment_terminal_outcome(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        loop {
            let changed = self.shared.terminal_changed.notified();
            tokio::pin!(changed);
            // Register before checking state, so a completion between the read
            // and await cannot be lost. Multiple readers each own their wait.
            changed.as_mut().enable();
            let state = self.get(ceremony).await?;
            if state.kind != CeremonyKind::DeviceEnrollment {
                return Err(AuraError::invalid(
                    "enrollment terminal wait requires enrollment state",
                ));
            }
            if let Some(outcome) = state.terminal_outcome {
                return Ok(outcome);
            }
            changed.await;
        }
    }

    /// The ceremony result, if an owner has completed it.
    pub async fn terminal_outcome(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Result<Option<CeremonyTerminalOutcome>, AuraError> {
        if let Ok(state) = self.get(ceremony_id).await {
            return Ok(state.terminal_outcome);
        }
        let Some(stored) = self.stored_enrollment_outcome(ceremony_id).await? else {
            return Err(AuraError::invalid(format!(
                "Ceremony {} not found",
                ceremony_id
            )));
        };
        Ok(stored.outcome)
    }

    /// Check if ceremony is complete (committed)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// True if threshold is reached
    pub async fn is_complete(&self, ceremony_id: &CeremonyId) -> Result<bool, AuraError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.is_committed)
    }

    /// Check if ceremony has timed out
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    ///
    /// # Returns
    /// True if ceremony has exceeded its timeout
    pub async fn is_timed_out(&self, ceremony_id: &CeremonyId) -> Result<bool, AuraError> {
        let state = self.get(ceremony_id).await?;
        let now = self
            .time
            .physical_time()
            .await
            .map_err(|e| aura_core::AuraError::Internal {
                message: "Failed to get current time".into(),
                source: Some(std::sync::Arc::new(e)),
            })?;
        self.observe_ceremony_window(&state, &now).await
    }

    /// Mark ceremony as failed
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    /// * `error_message` - Optional error description
    pub async fn mark_failed(
        &self,
        ceremony_id: &CeremonyId,
        error_message: Option<String>,
    ) -> Result<(), AuraError> {
        self.fail_with_reason(
            ceremony_id,
            CeremonyFailureReason::RuntimeFailed,
            error_message,
        )
        .await
    }

    /// Fail a ceremony with a stable classification and optional diagnostic.
    pub async fn fail_with_reason(
        &self,
        ceremony_id: &CeremonyId,
        reason: CeremonyFailureReason,
        error_message: Option<String>,
    ) -> Result<(), AuraError> {
        self.complete(ceremony_id, CeremonyTerminalOutcome::Failed(reason))
            .await?;
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                let state = tracker.ceremonies.get_mut(ceremony_id).ok_or_else(|| {
                    AuraError::not_found(format!("Ceremony {} not found", ceremony_id))
                })?;
                if state.error_message.is_none() {
                    state.error_message = error_message;
                }
                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Remove ceremony from tracker (cleanup after completion/failure)
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony identifier
    pub async fn remove(&self, ceremony_id: &CeremonyId) -> Result<(), AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        if let Some(effects) = &self.shared.persistence {
            if effects
                .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                    "device_enrollment_activation_v1",
                    ceremony_id.to_string(),
                ))
                .await?
                && self.get(ceremony_id).await?.terminal_outcome.is_none()
            {
                return Err(AuraError::invalid(
                    "prepared enrollment requires reconciliation before registry invalidation",
                ));
            }
        }
        let current = self.get(ceremony_id).await?;
        if current.kind == CeremonyKind::DeviceEnrollment {
            self.persist_first_enrollment_terminal_decision(
                &current,
                current
                    .terminal_outcome
                    .unwrap_or(CeremonyTerminalOutcome::Failed(
                        CeremonyFailureReason::Cancelled,
                    )),
            )
            .await?;
        }
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                let is_enrollment = tracker
                    .ceremonies
                    .get(ceremony_id)
                    .is_some_and(|state| state.kind == CeremonyKind::DeviceEnrollment);
                if is_enrollment {
                    tracker.retired_enrollment_ids.insert(ceremony_id.clone());
                }
                tracker.ceremonies.remove(ceremony_id).ok_or_else(|| {
                    AuraError::not_found(format!("Ceremony {} not found", ceremony_id))
                })?;

                tracker.enrollment_responses.remove(ceremony_id);

                tracing::debug!(ceremony_id = %ceremony_id, "Ceremony removed from tracker");

                Ok(())
            },
            |tracker| tracker.validate(),
        )
        .await
    }

    /// Get list of all active ceremonies
    ///
    /// # Returns
    /// Vector of (ceremony_id, state) tuples
    pub async fn list_active(&self) -> Vec<(CeremonyId, TrackedCeremony)> {
        let state = self.shared.state.read().await;
        state
            .ceremonies
            .iter()
            .map(|(id, ceremony)| (id.clone(), ceremony.clone()))
            .collect()
    }

    /// Return the original terminal failure of the owned cleanup service.
    pub(crate) async fn require_cleanup_service_success(&self) -> Result<(), AuraError> {
        let group = self.shared.cleanup_tasks.read().await;
        if let Some(source) = group.as_ref().and_then(TaskGroup::terminal_failure) {
            return Err(AuraError::Internal {
                message: "required ceremony cleanup service failed".into(),
                source: Some(Arc::new(source)),
            });
        }
        Ok(())
    }

    /// Cleanup timed out ceremonies
    ///
    /// Should be called periodically to remove stale ceremonies
    ///
    /// # Returns
    /// Number of ceremonies cleaned up
    pub async fn cleanup_timed_out(&self) -> Result<usize, AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        // Get current time before entering the closure for deterministic simulation support
        let now = self
            .time
            .physical_time()
            .await
            .map_err(|source| AuraError::Internal {
                message: "read required ceremony cleanup clock".into(),
                source: Some(Arc::new(source)),
            })?;

        let candidates = self.list_active().await;
        let mut count = 0;
        for (id, state) in candidates {
            if state.terminal_outcome.is_some() || state.is_superseded {
                continue;
            }
            if !self.observe_ceremony_window(&state, &now).await? {
                continue;
            }
            if state.kind == CeremonyKind::DeviceEnrollment {
                if let Some(effects) = &self.shared.persistence {
                    let location = aura_core::effects::SecureStorageLocation::new(
                        "device_enrollment_activation_v1",
                        id.to_string(),
                    );
                    if effects.secure_exists(&location).await? {
                        continue; // Prepared irreversible work belongs to recovery.
                    }
                }
            }
            self.complete_under_decision_gate(
                &id,
                CeremonyTerminalOutcome::Failed(CeremonyFailureReason::TimedOut),
            )
            .await?;
            count += 1;
        }
        Ok(count)
    }

    // =========================================================================
    // SUPERSESSION METHODS
    // =========================================================================

    /// Supersede an existing ceremony with a new one.
    ///
    /// The old ceremony is marked as superseded and should stop processing.
    /// Supersession facts should be emitted after calling this method.
    ///
    /// # Arguments
    /// * `old_ceremony_id` - The ceremony being superseded
    /// * `new_ceremony_id` - The ceremony that supersedes it
    /// * `reason` - Why the supersession occurred
    /// * `timestamp_ms` - When the supersession was recorded
    pub async fn supersede(
        &self,
        old_ceremony_id: &CeremonyId,
        new_ceremony_id: &CeremonyId,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, AuraError> {
        let _decision = self.shared.enrollment_decision_gate.lock().await;
        self.supersede_under_decision(old_ceremony_id, new_ceremony_id, reason, timestamp_ms)
            .await
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        family = "runtime_helper"
    )]
    pub(crate) async fn supersede_owned_device_enrollment(
        &self,
        generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        old_ceremony_id: &CeremonyId,
        new_ceremony_id: &CeremonyId,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, AuraError> {
        generation.require_tracker(self)?;
        if generation.ceremony_id() != new_ceremony_id {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
            ));
        }
        self.supersede_under_decision(old_ceremony_id, new_ceremony_id, reason, timestamp_ms)
            .await
    }
    async fn supersede_under_decision(
        &self,
        old_ceremony_id: &CeremonyId,
        new_ceremony_id: &CeremonyId,
        reason: SupersessionReason,
        timestamp_ms: u64,
    ) -> Result<SupersessionRecord, AuraError> {
        if let Some(effects) = &self.shared.persistence {
            if effects
                .secure_exists(&aura_core::effects::SecureStorageLocation::new(
                    "device_enrollment_activation_v1",
                    old_ceremony_id.to_string(),
                ))
                .await?
                && self.get(old_ceremony_id).await?.terminal_outcome.is_none()
            {
                return Err(AuraError::invalid(
                    "prepared enrollment requires reconciliation before registry invalidation",
                ));
            }
        }
        let previous = self.get(old_ceremony_id).await?;
        if previous.kind == CeremonyKind::DeviceEnrollment && previous.terminal_outcome.is_none() {
            self.persist_first_enrollment_terminal_decision(
                &previous,
                CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Superseded),
            )
            .await?;
        }
        // Create record outside the lock scope
        let old_ceremony_hash = Hash32::from_bytes(old_ceremony_id.as_str().as_bytes());
        let new_ceremony_hash = Hash32::from_bytes(new_ceremony_id.as_str().as_bytes());
        let record = SupersessionRecord::new(
            old_ceremony_hash,
            new_ceremony_hash,
            reason.clone(),
            timestamp_ms,
        );

        let record = with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                // Verify old ceremony exists
                let old_state = tracker.ceremonies.get_mut(old_ceremony_id).ok_or_else(|| {
                    AuraError::invalid(format!("Ceremony {} not found", old_ceremony_id))
                })?;

                // Check if already in terminal state
                if old_state.is_committed {
                    return Err(AuraError::invalid(format!(
                        "Cannot supersede committed ceremony {}",
                        old_ceremony_id
                    )));
                }

                if old_state.is_superseded {
                    // Already superseded - idempotent
                    tracing::debug!(
                        old_ceremony = %old_ceremony_id,
                        new_ceremony = %new_ceremony_id,
                        "Ceremony already superseded (idempotent)"
                    );
                    return Ok(record.clone());
                }

                if old_state.terminal_outcome.is_some() {
                    return Err(AuraError::invalid(format!(
                        "Cannot supersede completed ceremony {}",
                        old_ceremony_id
                    )));
                }

                // Mark old ceremony as superseded
                old_state.is_superseded = true;
                old_state.superseded_by = Some(new_ceremony_id.clone());
                old_state.has_failed = true;
                old_state.failure_reason = Some(CeremonyFailureReason::Superseded);
                old_state.terminal_outcome = Some(CeremonyTerminalOutcome::Failed(
                    CeremonyFailureReason::Superseded,
                ));
                old_state.error_message = Some(format!("Superseded: {}", reason.description()));

                // Update new ceremony if it exists (may be registered separately)
                if let Some(new_state) = tracker.ceremonies.get_mut(new_ceremony_id) {
                    new_state.supersedes.push(old_ceremony_id.clone());
                }

                // Record for audit trail
                tracker.supersession_records.push(record.clone());

                tracing::info!(
                    old_ceremony = %old_ceremony_id,
                    new_ceremony = %new_ceremony_id,
                    reason = %reason.code(),
                    "Ceremony superseded"
                );

                Ok(record.clone())
            },
            |tracker| tracker.validate(),
        )
        .await?;
        self.shared.terminal_changed.notify_waiters();
        self.persist_enrollment(old_ceremony_id).await?;
        Ok(record)
    }

    /// Check for ceremonies that would be superseded by a new ceremony.
    ///
    /// Returns active ceremonies of the same kind that could be superseded
    /// based on prestate staleness or same-initiator detection.
    ///
    /// # Arguments
    /// * `kind` - The kind of ceremony being initiated
    /// * `prestate_hash` - Current prestate hash
    ///
    /// # Returns
    /// Vector of ceremony IDs that are candidates for supersession
    pub async fn check_supersession_candidates(
        &self,
        kind: CeremonyKind,
        prestate_hash: &Hash32,
    ) -> Vec<CeremonyId> {
        let state = self.shared.state.read().await;

        state
            .ceremonies
            .iter()
            .filter(|(_, ceremony)| {
                // Must be same kind
                if ceremony.kind != kind {
                    return false;
                }

                // Skip terminal ceremonies
                if ceremony.terminal_outcome.is_some()
                    || ceremony.is_committed
                    || ceremony.is_superseded
                {
                    return false;
                }

                // Check for prestate staleness against the explicit prestate hash.
                if prestate_hash != &ceremony.prestate_hash {
                    return true; // Prestate changed, candidate for supersession
                }

                // Active ceremony of same kind is always a candidate
                true
            })
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get supersession chain for audit trail.
    ///
    /// Returns all supersession records involving the given ceremony
    /// (either as superseded or superseding).
    ///
    /// # Arguments
    /// * `ceremony_id` - The ceremony to get supersession history for
    pub async fn get_supersession_chain(
        &self,
        ceremony_id: &CeremonyId,
    ) -> Vec<SupersessionRecord> {
        let ceremony_hash = Hash32::from_bytes(ceremony_id.as_str().as_bytes());
        let state = self.shared.state.read().await;

        state
            .supersession_records
            .iter()
            .filter(|record| {
                record.superseded_id == ceremony_hash || record.superseding_id == ceremony_hash
            })
            .cloned()
            .collect()
    }

    /// Check if a ceremony has been superseded.
    pub async fn is_superseded(&self, ceremony_id: &CeremonyId) -> Result<bool, AuraError> {
        let state = self.get(ceremony_id).await?;
        Ok(state.is_superseded)
    }

    /// Get all supersession records (for debugging/auditing).
    pub async fn all_supersession_records(&self) -> Vec<SupersessionRecord> {
        let state = self.shared.state.read().await;
        state.supersession_records.clone()
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for CeremonyTracker {
    fn name(&self) -> &'static str {
        "ceremony_tracker"
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        *self.shared.lifecycle.write().await = ServiceHealth::Healthy;
        let cleanup_group = context.tasks().group(self.name());
        self.spawn_timeout_cleanup_task(cleanup_group.clone(), context.time_effects());
        *self.shared.cleanup_tasks.write().await = Some(cleanup_group);
        Ok(())
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        *self.shared.lifecycle.write().await = ServiceHealth::Stopping;
        if let Some(task_group) = self.shared.cleanup_tasks.write().await.take() {
            task_group
                .shutdown_with_timeout(Duration::from_secs(2))
                .await
                .map_err(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        "failed to stop ceremony cleanup task group",
                    )
                    .with_cause(error)
                })?;
        }
        // Clean up any tracked ceremonies
        self.cleanup_timed_out().await.map_err(|source| {
            ServiceError::shutdown_failed(self.name(), "required ceremony cleanup failed")
                .with_cause(source)
        })?;
        *self.shared.lifecycle.write().await = ServiceHealth::Stopped;
        Ok(())
    }

    async fn health(&self) -> ServiceHealth {
        if let Err(source) = self.require_cleanup_service_success().await {
            return ServiceHealth::Unhealthy {
                reason: source.to_string(),
            };
        }
        self.shared.lifecycle.read().await.clone()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn enrollment_custody_waits_generation_then_decision_then_tree() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_app::runtime_bridge::RuntimeBridge;
            use futures::FutureExt;
            let (issuer, invitee, _invitation, start, _acceptance, _proof) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "custody-lock-order",
                ),
            )
            .await;
            let effects = issuer.runtime().effects();
            let tracker = issuer.runtime().ceremony_tracker();
            let generation = effects.acquire_enrollment_generation_custody().await;
            let mut activation = Box::pin(tracker.begin_enrollment_activation(&start.ceremony_id));
            assert!(
                activation.as_mut().now_or_never().is_none(),
                "activation must wait for actual generation custody"
            );
            let decision = tracker
                .shared
                .enrollment_decision_gate
                .try_lock()
                .expect("activation must not take decision before generation");
            drop(decision);
            drop(activation);
            drop(generation);
            crate::runtime_bridge::AgentRuntimeBridge::new(issuer.clone())
                .cancel_key_rotation_ceremony(&start.ceremony_id)
                .await
                .expect("actual cancellation releases original pending generation");
            let code = crate::runtime_bridge::AgentRuntimeBridge::new(invitee.clone())
                .export_device_enrollment_setup_request()
                .await
                .expect("actual physical setup export");
            let app = Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    Arc::new(crate::runtime_bridge::AgentRuntimeBridge::new(
                        issuer.clone(),
                    )),
                )
                .expect("actual app transfer owner"),
            ));
            let setup =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                    &app, code,
                )
                .await
                .expect("actual exported setup pin");
            let tree = effects.lock_tree_decision().await;
            let mut plan =
                Box::pin(effects.prepare_authenticated_enrollment_rotation(&setup, tracker));
            assert!(
                plan.as_mut().now_or_never().is_none(),
                "fresh issuer waits for actual tree owner"
            );
            assert!(
                tracker.shared.enrollment_decision_gate.try_lock().is_err(),
                "fresh issuer must own decision before waiting for tree"
            );
            let mut contender =
                Box::pin(tracker.acquire_enrollment_generation_decision(effects.as_ref()));
            assert!(
                contender.as_mut().now_or_never().is_none(),
                "second actual owner cannot pass the first physical generation owner"
            );
            drop(tree);
            let held = plan.await.expect("fresh roster retains ordered custody");
            assert!(
                contender.as_mut().now_or_never().is_none(),
                "roster retains exclusive custody through handoff"
            );
            drop(held);
            let next = contender
                .await
                .expect("second owner proceeds after complete owner release");
            next.require_effects(effects.as_ref())
                .expect("exact original effect owner");
            next.require_tracker(tracker)
                .expect("exact original tracker owner");
        });
    }

    #[test]
    fn physical_generation_cannot_enter_another_runtime_tracker() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, _invitation, _start, _acceptance, _proof) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "custody-runtime-owner",
                ),
            )
            .await;
            let issuer_effects = issuer.runtime().effects();
            let generation = issuer_effects.acquire_enrollment_generation_custody().await;
            let failure = match invitee
                .runtime()
                .ceremony_tracker()
                .retain_enrollment_generation_decision(generation)
                .await
            {
                Ok(_) => panic!("another actual physical runtime cannot retain this generation"),
                Err(error) => error,
            };
            assert!(matches!(
                std::error::Error::source(&failure).and_then(|cause| {
                    cause.downcast_ref::<crate::runtime::effects::HeldEnrollmentRegistrationError>()
                }),
                Some(crate::runtime::effects::HeldEnrollmentRegistrationError::EffectIdentity)
            ));
        });
    }
    use super::*;
    use aura_core::types::identifiers::{AuthorityId, CeremonyId};
    use aura_core::DeviceId;
    use aura_effects::time::PhysicalTimeHandler;

    async fn register_original_fixture(
        tracker: &CeremonyTracker,
        ceremony: &CeremonyId,
    ) -> Result<(), AuraError> {
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("actual retained effect owner");
        let (generation, retained) = effects
            .recover_initial_enrollment_allocation(ceremony, tracker)
            .await?;
        tracker
            .register_owned_device_enrollment(
                &generation,
                crate::runtime::services::ceremony_runner::CeremonyInitRequest {
                    ceremony_id: retained.ceremony_id,
                    kind: retained.kind,
                    initiator_id: retained.initiator_id,
                    threshold_k: retained.threshold_k,
                    total_n: retained.total_n,
                    participants: retained.participants.into_iter().collect(),
                    new_epoch: retained.new_epoch,
                    enrollment_device_id: retained.enrollment_device_id,
                    enrollment_nickname_suggestion: retained.enrollment_nickname_suggestion,
                    prestate_hash: retained.prestate_hash,
                },
            )
            .await
    }
    #[tokio::test]
    async fn interrupted_pre_live_allocation_recovers_original_window_without_renewal() {
        use aura_core::effects::{SecureStorageEffects, SecureStorageLocation};
        let (interrupted, clock, id, result) = Box::pin(allocated_clock_fixture(
            "interrupted-pre-live-original-clock",
            Some(AuraError::Storage {
                message: "fault before initial clock acknowledgment".into(),
                source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                    "initial clock write fault".into(),
                ))),
            }),
        ))
        .await;
        let effects = interrupted
            .shared
            .persistence
            .as_ref()
            .expect("actual durable owner")
            .clone();
        let failure = result.expect_err("initial write fault prevents registration");
        assert!(
            std::error::Error::source(&failure).is_some_and(|source| source
                .downcast_ref::<aura_core::effects::StorageError>()
                .is_some())
        );
        let original = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(&effects, &id).await.expect("actual immutable allocation acknowledged before injected fault");
        assert!(!effects
            .secure_exists(&SecureStorageLocation::new(
                "enrollment_clock_v1",
                id.to_string()
            ))
            .await
            .expect("initial clock absence"));
        assert!(
            interrupted.get(&id).await.is_err(),
            "unacknowledged allocation must not be visible for execution"
        );
        clock.set_time(7_000);
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock.clone()), effects);
        register_original_fixture(&restarted, &original.ceremony_id)
            .await
            .expect("recover exact immutable pre-live allocation");
        let recovered = restarted
            .get(&id)
            .await
            .expect("recovered registered owner");
        assert_eq!(recovered.timeout_budget.started_at_ms(), 5_000);
        assert_eq!(
            recovered.timeout_budget.deadline_at_ms(),
            original.timeout_budget.deadline_at_ms()
        );
        assert_eq!(
            recovered
                .timeout_budget
                .remaining_at(&PhysicalTime::exact(7_000))
                .expect("original remaining allowance")
                .as_millis(),
            u128::from(original.timeout_budget.deadline_at_ms() - 7_000)
        );
    }
    #[tokio::test]
    async fn once_live_missing_clock_is_not_reconstructed_on_registration_retry() {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let (first, clock, id) = registered_clock_fixture(
            "once_live_missing_clock_is_not_reconstructed_on_registration_retry",
        )
        .await;
        let original = first.get(&id).await.expect("original live allocation");
        let effects = first
            .shared
            .persistence
            .as_ref()
            .expect("fixture persistence")
            .clone();
        let location = SecureStorageLocation::new("enrollment_clock_v1", id.to_string());
        effects
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .expect("inject lost live checkpoint");
        clock.set_time(7_000);
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock), effects.clone());
        assert!(register_original_fixture(&restarted, &original.ceremony_id)
            .await
            .is_err());
        assert!(!effects
            .secure_exists(&location)
            .await
            .expect("required absence query"));
        assert!(
            restarted.get(&id).await.is_err(),
            "failed repair must not publish live registration"
        );
    }
    #[tokio::test]
    #[cfg(unix)]
    async fn canonical_legacy_registration_prevents_missing_marker_and_clock_repair() {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let (first, clock, id) = registered_clock_fixture(
            "canonical_legacy_registration_prevents_missing_marker_and_clock_repair",
        )
        .await;
        let original = first
            .get(&id)
            .await
            .expect("original canonical registration");
        let effects = first
            .shared
            .persistence
            .as_ref()
            .expect("fixture persistence")
            .clone();
        let location = SecureStorageLocation::new("enrollment_clock_v1", id.to_string());
        effects
            .fault_remove_secure_record_for_test(&live_enrollment_window_location(&id))
            .await
            .expect("simulate legacy missing phase marker");
        effects
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .expect("inject lost original clock");
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock), effects.clone());
        assert!(register_original_fixture(&restarted, &original.ceremony_id)
            .await
            .is_err());
        assert!(!effects
            .secure_exists(&location)
            .await
            .expect("required clock absence query"));
        assert!(!effects
            .secure_exists(&live_enrollment_window_location(&id))
            .await
            .expect("required marker absence query"));
    }

    #[tokio::test]
    async fn registered_window_rejects_replaced_allocation_before_checkpoint() {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let (_issuer, _invitee, tracker, _, id) = issued_registered_clock_fixture(
            "registered_window_rejects_replaced_allocation_before_checkpoint",
        )
        .await;
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("original runtime");
        let registered = tracker
            .get(&id)
            .await
            .expect("original registered generation");
        let generation = effects
            .resume_owned_enrollment_registration(
                &tracker,
                registered.initiator_id,
                registered.new_epoch,
                &registered.ceremony_id,
                registered.prestate_hash,
            )
            .await
            .expect("recover actual signed registered generation owner");
        let capability = tracker
            .acquire_registered_enrollment_generation_window(&generation)
            .await
            .expect("actual registered owner");
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("fixture secure storage");
        let location = SecureStorageLocation::new("enrollment_clock_v1", id.to_string());
        let before = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("original retained clock");
        {
            let mut state = tracker.shared.state.write().await;
            let registered = state
                .ceremonies
                .get_mut(&id)
                .expect("live original allocation");
            registered.timeout_budget = aura_core::TimeoutBudget::from_start_and_timeout(
                &registered.started_at,
                registered.timeout,
            )
            .expect("same bounds with independent observation owner and original lease");
        }
        assert!(
            capability.checkpoint().await.is_err(),
            "a replaced registry allocation cannot acknowledge the sealed owner"
        );
        let after = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("retained original clock after rejection");
        assert_eq!(
            after, before,
            "rejection must not overwrite original durable evidence"
        );
    }

    #[tokio::test]
    async fn observed_snapshot_cannot_replace_registered_execution_window() {
        let (_issuer, _invitee, tracker, _, id) = issued_registered_clock_fixture(
            "observed_snapshot_cannot_replace_registered_execution_window",
        )
        .await;
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("original runtime");
        let registered = tracker
            .get(&id)
            .await
            .expect("original registered generation");
        let generation = effects
            .resume_owned_enrollment_registration(
                &tracker,
                registered.initiator_id,
                registered.new_epoch,
                &registered.ceremony_id,
                registered.prestate_hash,
            )
            .await
            .expect("recover actual signed registered generation owner");
        let original = tracker
            .get(&id)
            .await
            .expect("registered original allocation");
        let mut observed = original.clone();
        observed.timeout_budget = aura_core::TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(50_000),
            Duration::from_secs(1_000),
        )
        .expect("independent observer budget");
        observed.enrollment_window_lease = Arc::new(tokio::sync::Semaphore::new(1));
        let capability = tracker
            .acquire_registered_enrollment_generation_window(&generation)
            .await
            .expect("actual registered owner");
        assert_eq!(
            capability.budget().started_at_ms(),
            original.timeout_budget.started_at_ms()
        );
        assert_eq!(
            capability.budget().deadline_at_ms(),
            original.timeout_budget.deadline_at_ms()
        );
        assert_ne!(
            capability.budget().deadline_at_ms(),
            observed.timeout_budget.deadline_at_ms()
        );
        let competing = tracker
            .acquire_registered_enrollment_generation_window(&generation)
            .await;
        let Err(error) = competing else {
            panic!("a second owner must not acquire the original allocation")
        };
        assert!(registered_enrollment_window_already_owned(&error));
        let mut source: &(dyn std::error::Error + 'static) = &error;
        while source
            .downcast_ref::<tokio::sync::TryAcquireError>()
            .is_none()
        {
            source = source
                .source()
                .expect("actual semaphore admission cause retained");
        }
    }

    #[tokio::test]
    async fn registered_window_postoperation_checkpoint_failure_prevents_success_publication() {
        let (_issuer, _invitee, tracker, clock, id) = issued_registered_clock_fixture(
            "registered_window_postoperation_checkpoint_failure_prevents_success_publication",
        )
        .await;
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("original runtime");
        let registered = tracker
            .get(&id)
            .await
            .expect("original registered generation");
        let generation = effects
            .resume_owned_enrollment_registration(
                &tracker,
                registered.initiator_id,
                registered.new_epoch,
                &registered.ceremony_id,
                registered.prestate_hash,
            )
            .await
            .expect("recover actual signed registered generation owner");
        let runner = super::super::ceremony_runner::CeremonyRunner::new(tracker.clone());
        let owner = runner
            .registered_enrollment_generation_window(&generation)
            .await
            .expect("actual registered execution owner");
        let result = owner
            .execute(&clock, || async {
                clock.set_time(6_000);
                *tracker.shared.clock_checkpoint_fault.lock().await = Some(AuraError::Storage {
                    message: "postoperation secure checkpoint fault".into(),
                    source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                        "fault before success publication".into(),
                    ))),
                });
                Ok::<_, AuraError>("operation ready")
            })
            .await;
        let error =
            result.expect_err("operation result waits for required checkpoint acknowledgment");
        assert!(
            matches!(
                &error,
                aura_core::TimeoutRunError::Timeout(
                    aura_core::TimeoutBudgetError::CheckpointFailure { .. }
                )
            ),
            "required persistence failure keeps its typed checkpoint category"
        );
        let mut source: &(dyn std::error::Error + 'static) = &error;
        loop {
            if matches!(
                source.downcast_ref::<aura_core::effects::StorageError>(),
                Some(aura_core::effects::StorageError::WriteFailed(_))
            ) {
                break;
            }
            source = source
                .source()
                .expect("actual postoperation storage cause retained");
        }
        // A retry in the same runtime retains the unacknowledged observation;
        // secure restart only trusts the last acknowledged checkpoint.
        clock.set_time(5_500);
        assert!(owner.remaining_ms(&clock).await.is_err());
    }

    async fn allocated_clock_fixture(
        test_identity: &str,
        fault: Option<AuraError>,
    ) -> (
        CeremonyTracker,
        aura_testkit::time::ManualPhysicalClock,
        CeremonyId,
        Result<(), AuraError>,
    ) {
        use crate::runtime_bridge::AgentRuntimeBridge;
        use aura_app::runtime_bridge::RuntimeBridge;
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let mut agents = Vec::new();
        for seed in [83u8, 86u8] {
            let authority = AuthorityId::new_from_entropy([seed; 32]);
            let config = crate::core::AgentConfig {
                device_id: DeviceId::new_from_entropy([seed + 1; 32]),
                storage: crate::core::config::StorageConfig {
                    base_path: tempfile::Builder::new()
                        .prefix("aura-owned-clock-")
                        .tempdir()
                        .expect("isolated real clock profile")
                        .keep(),
                    ..Default::default()
                },
                ..Default::default()
            };
            let context = aura_core::context::EffectContext::new(
                authority,
                aura_core::ContextId::new_from_entropy([seed + 2; 32]),
                aura_core::effects::ExecutionMode::Testing,
            );
            let profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
                .expect("actual selected profile lease");
            let runtime = crate::runtime::EffectSystemBuilder::testing_with_owned_profile(profile)
                .with_authority(authority)
                .with_config(config)
                .with_physical_time_provider(Arc::new(clock.clone()))
                .build(&context)
                .await
                .expect("actual clock-bound runtime");
            let agent = Arc::new(crate::AuraAgent::new(runtime, authority));
            AgentRuntimeBridge::new(agent.clone())
                .bootstrap_signing_keys()
                .await
                .expect("actual signing bootstrap");
            agents.push(agent);
        }
        let issuer = &agents[0];
        let invitee = &agents[1];
        let code = AgentRuntimeBridge::new(invitee.clone())
            .export_device_enrollment_setup_request()
            .await
            .expect("actual provisional setup");
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(issuer.clone())),
            )
            .expect("actual transfer app"),
        ));
        let pin =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &app, code,
            )
            .await
            .expect("user transferred setup pin");
        let reserved = issuer
            .invitations()
            .expect("actual invitation owner")
            .reserve_device_enrollment_invitation()
            .await
            .expect("actual invitation reservation");
        let id = test_ceremony_id(test_identity);
        let effects = issuer.runtime().effects().clone();
        let tracker = CeremonyTracker::new_with_storage(Arc::new(clock.clone()), effects.clone());
        let plan = effects
            .prepare_authenticated_enrollment_rotation(&pin, &tracker)
            .await
            .expect("actual authenticated roster/prestate owner");
        let prestate = plan.prestate();
        let (epoch, _, _, generation) = effects
            .prepare_pinned_enrollment_rotation(&pin, &reserved, &id, plan)
            .await
            .expect("actual held generation");
        let pending = issuer
            .threshold_signing()
            .capture_retained_pending_generation(&issuer.authority_id(), epoch)
            .await
            .expect("actual pending generation");
        crate::handlers::invitation::enrollment_trust::retain_pending_signing_generation(
            &effects, &id, &pending, prestate,
        )
        .await
        .expect("retain original pending evidence");
        crate::handlers::invitation::enrollment_trust::retain_user_transferred_verifier(
            &effects,
            issuer.authority_id(),
            &id,
            epoch,
            issuer.context().device_id(),
            &pin,
        )
        .await
        .expect("retain independent setup");

        *tracker.shared.clock_checkpoint_fault.lock().await = fault;
        let result = tracker
            .register_owned_device_enrollment(
                &generation,
                crate::runtime::services::ceremony_runner::CeremonyInitRequest {
                    ceremony_id: id.clone(),
                    kind: CeremonyKind::DeviceEnrollment,
                    initiator_id: issuer.authority_id(),
                    threshold_k: 1,
                    total_n: 1,
                    participants: vec![ParticipantIdentity::device(invitee.context().device_id())],
                    new_epoch: epoch,
                    enrollment_device_id: Some(invitee.context().device_id()),
                    enrollment_nickname_suggestion: None,
                    prestate_hash: prestate,
                },
            )
            .await;
        drop(generation);
        (tracker, clock, id, result)
    }
    async fn issued_registered_clock_fixture(
        label: &str,
    ) -> (
        Arc<crate::AuraAgent>,
        Arc<crate::AuraAgent>,
        CeremonyTracker,
        aura_testkit::time::ManualPhysicalClock,
        CeremonyId,
    ) {
        let clock = aura_testkit::time::ManualPhysicalClock::new(5_000);
        let (issuer, invitee, _, start, _, _) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock(
                label,
                Arc::new(clock.clone()),
            )
            .await;
        // Release the real sender's execution lease before acquiring the same
        // registered owner. Forced drain does not invent a terminal decision.
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(Duration::from_secs(2))
            .await
            .expect("actual sender owner drained");
        let tracker = issuer.ceremony_tracker().await;
        let state = tracker
            .get(&start.ceremony_id)
            .await
            .expect("actual issued registration");
        assert!(
            state.terminal_outcome.is_none(),
            "fixture remains an active registration"
        );
        (issuer, invitee, tracker, clock, start.ceremony_id)
    }

    async fn registered_clock_fixture(
        test_identity: &str,
    ) -> (
        CeremonyTracker,
        aura_testkit::time::ManualPhysicalClock,
        CeremonyId,
    ) {
        let (tracker, clock, id, result) =
            Box::pin(allocated_clock_fixture(test_identity, None)).await;
        result.expect("register actual held original allocation");
        (tracker, clock, id)
    }
    #[tokio::test]
    async fn original_completion_observation_releases_execution_lease_without_renewing_clock() {
        let (issuer, invitee, tracker, clock, id) =
            issued_registered_clock_fixture("original-completion-observation-lease").await;
        let state = tracker
            .get(&id)
            .await
            .expect("actual original registered allocation");
        let semaphore = state.enrollment_window_lease.clone();
        let deadline = state.timeout_budget.deadline_at_ms();
        let permit = semaphore
            .clone()
            .try_acquire_owned()
            .expect("actual original execution permit");
        // Test-only original capability construction uses genuine retained state
        // and the actual semaphore allocation, not a serialized replacement.
        let execution = RegisteredEnrollmentWindowCapability {
            tracker,
            state,
            lease: Arc::new(permit),
            notice_binding: std::sync::OnceLock::new(),
        };
        let observation = execution.completion_observation();
        assert!(observation
            .budget()
            .shares_observation_owner_with(execution.budget()));
        assert!(
            semaphore.clone().try_acquire_owned().is_err(),
            "observation does not release a still-live executor"
        );
        assert!(
            observation
                .require_effects(invitee.runtime().effects().as_ref())
                .is_err(),
            "another physical runtime cannot drive original completion observation"
        );
        drop(execution);
        let registered_permit = semaphore
            .clone()
            .try_acquire_owned()
            .expect("completion observer retains no execution permit during registered handoff");
        observation
            .require_effects(issuer.runtime().effects().as_ref())
            .expect("actual original effect owner");
        observation
            .checkpoint()
            .await
            .expect("genuine original protected checkpoint remains valid after lease transfer");
        assert_eq!(observation.budget().deadline_at_ms(), deadline);
        clock.set_time(deadline + 1);
        let now = issuer
            .runtime()
            .effects()
            .physical_time()
            .await
            .expect("actual original local clock");
        assert!(observation.budget().remaining_at(&now).is_err());
        observation
            .checkpoint()
            .await
            .expect("persist original expiration without renewing interval");
        assert_eq!(observation.budget().deadline_at_ms(), deadline);
        drop(registered_permit);
    }

    #[tokio::test]
    async fn held_original_recovery_rejects_expired_allocation_before_live_publication() {
        let (first, clock, id) = registered_clock_fixture("held-original-expired-allocation").await;
        let mut original = first.get(&id).await.expect("actual original allocation");
        let effects = first
            .shared
            .persistence
            .as_ref()
            .expect("actual durable profile")
            .clone();
        clock.set_time(original.timeout_budget.deadline_at_ms());
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock.clone()), effects);
        let failure = register_original_fixture(&restarted, &id)
            .await
            .expect_err("expired original cannot regain live eligibility");
        let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut typed_deadline = false;
        while let Some(source) = cursor {
            if source
                .downcast_ref::<aura_core::TimeoutBudgetError>()
                .is_some()
            {
                typed_deadline = true;
            }
            cursor = source.source();
        }
        assert!(
            typed_deadline,
            "actual deadline failure retains its native source"
        );
        assert!(
            restarted.get(&id).await.is_err(),
            "expired recovery cannot publish a live entry"
        );
        restarted
            .restore_enrollment_clock(&mut original)
            .await
            .expect("original observation checkpoint retained");
        assert_eq!(original.timeout_budget.started_at_ms(), 5000);
        assert!(original
            .timeout_budget
            .remaining_at(&PhysicalTime::exact(
                clock
                    .physical_time()
                    .await
                    .expect("actual injected clock")
                    .ts_ms
            ))
            .is_err());
    }
    #[tokio::test]
    async fn held_recovery_does_not_authorize_from_mutated_profile_invitation_or_roster() {
        use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
        let (first, clock, id) =
            registered_clock_fixture("held-original-mutable-profile-tamper").await;
        let original = first.get(&id).await.expect("real original allocation");
        let effects = first
            .shared
            .persistence
            .as_ref()
            .expect("actual profile owner")
            .clone();
        let location = crate::runtime::effects::enrollment_generation_profile_location(
            &original.initiator_id,
            original.new_epoch,
        );
        let bytes = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .expect("actual mutable profile");
        let mut profile: serde_json::Value =
            serde_json::from_slice(&bytes).expect("profile schema");
        profile["invitation"] = serde_json::to_value(aura_core::InvitationId::new(
            "contradictory-original-invitation",
        ))
        .expect("negative selector encoding");
        effects
            .secure_store(
                &location,
                &serde_json::to_vec(&profile).expect("corrupt profile encoding"),
                &[SecureStorageCapability::Write],
            )
            .await
            .expect("inject mutable profile corruption");
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock.clone()), effects.clone());
        let first_error = register_original_fixture(&restarted, &id)
            .await
            .expect_err("mutable invitation cannot replace independent original");
        // With selected custody the generation history refuses the contradictory
        // invitation first; registration binding is the next line of defense.
        let first_source = std::error::Error::source(&first_error);
        assert!(
            matches!(
                first_source
                    .and_then(|source| source
                        .downcast_ref::<crate::runtime::effects::HeldEnrollmentRegistrationError>(
                    )),
                Some(crate::runtime::effects::HeldEnrollmentRegistrationError::Binding)
            ) || matches!(
                first_source
                    .and_then(|source| source
                        .downcast_ref::<crate::runtime::effects::EnrollmentGenerationHistoryError>(
                    )),
                Some(crate::runtime::effects::EnrollmentGenerationHistoryError::OriginalBinding)
            ),
            "contradictory invitation must be refused by a binding check: {first_error:?}"
        );
        let mut profile: serde_json::Value =
            serde_json::from_slice(&bytes).expect("original profile schema");
        profile["participants"]
            .as_array_mut()
            .expect("actual ordered roster")
            .reverse();
        effects
            .secure_store(
                &location,
                &serde_json::to_vec(&profile).expect("reordered profile encoding"),
                &[SecureStorageCapability::Write],
            )
            .await
            .expect("inject ordered roster corruption");
        register_original_fixture(&restarted, &id)
            .await
            .expect_err("same set in a different signing order cannot replace original roster");
        assert!(
            restarted.get(&id).await.is_err(),
            "corrupt mutable evidence never exposes live owner"
        );
        effects
            .secure_store(&location, &bytes, &[SecureStorageCapability::Write])
            .await
            .expect("restore original mutable profile for control case");
        register_original_fixture(&restarted, &id)
            .await
            .expect("exact independent original remains recoverable");
    }
    #[tokio::test]
    async fn registered_clock_checkpoint_restores_highwater_and_latched_rollback() {
        use std::error::Error;
        let (first, clock, id) = registered_clock_fixture(
            "registered_clock_checkpoint_restores_highwater_and_latched_rollback",
        )
        .await;
        let mut original = first.get(&id).await.expect("original registered state");
        let deadline = original.timeout_budget.deadline_at_ms();
        clock.set_time(6_000);
        assert!(!first
            .is_timed_out(&id)
            .await
            .expect("progress observation checkpoint"));
        let restarted = CeremonyTracker::new_with_storage(
            Arc::new(clock.clone()),
            first
                .shared
                .persistence
                .as_ref()
                .expect("actual secure store")
                .clone(),
        );
        restarted
            .restore_enrollment_clock(&mut original)
            .await
            .expect("restore actual retained observation");
        assert_eq!(original.timeout_budget.deadline_at_ms(), deadline);
        assert!(matches!(
            original
                .timeout_budget
                .remaining_at(&PhysicalTime::exact(5_500)),
            Err(aura_core::TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms: 6_000,
                observed_at_ms: 5_500
            })
        ));
        clock.set_time(5_500);
        let error = first
            .is_timed_out(&id)
            .await
            .expect_err("rollback must not rejuvenate registered window");
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
        ));
        restarted
            .restore_enrollment_clock(&mut original)
            .await
            .expect("restore latched rollback from secure checkpoint");
        assert!(matches!(
            original
                .timeout_budget
                .remaining_at(&PhysicalTime::exact(7_000)),
            Err(aura_core::TimeoutBudgetError::ClockRollback { .. })
        ));
    }
    #[tokio::test]
    async fn registered_window_checkpoint_failure_blocks_operation_and_retains_storage_source() {
        use std::error::Error;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_issuer, _invitee, tracker, clock, id) = issued_registered_clock_fixture(
            "registered_window_checkpoint_failure_blocks_operation_and_retains_storage_source",
        )
        .await;
        let effects = tracker
            .shared
            .persistence
            .as_ref()
            .expect("original runtime");
        let registered = tracker
            .get(&id)
            .await
            .expect("original registered generation");
        let generation = effects
            .resume_owned_enrollment_registration(
                &tracker,
                registered.initiator_id,
                registered.new_epoch,
                &registered.ceremony_id,
                registered.prestate_hash,
            )
            .await
            .expect("recover actual signed registered generation owner");
        let runner = super::super::ceremony_runner::CeremonyRunner::new(tracker.clone());
        let owner = runner
            .registered_enrollment_generation_window(&generation)
            .await
            .expect("sealed actual registered owner");
        *tracker.shared.clock_checkpoint_fault.lock().await = Some(AuraError::Storage {
            message: "injected required secure checkpoint failure".into(),
            source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                "fault at secure checkpoint boundary".into(),
            ))),
        });
        let polls = AtomicUsize::new(0);
        let error = owner
            .execute(&clock, || async {
                polls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, AuraError>(())
            })
            .await
            .expect_err("checkpoint acknowledgment is required before operation polling");
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        let mut source: &(dyn Error + 'static) = &error;
        loop {
            if matches!(
                source.downcast_ref::<aura_core::effects::StorageError>(),
                Some(aura_core::effects::StorageError::WriteFailed(_))
            ) {
                break;
            }
            source = source
                .source()
                .expect("original secure storage failure stays in standard chain");
        }
    }
    #[tokio::test]
    async fn registered_clock_missing_checkpoint_fails_closed() {
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };
        let (tracker, _, id) =
            registered_clock_fixture("registered_clock_missing_checkpoint_fails_closed").await;
        let mut original = tracker.get(&id).await.expect("retained registered state");
        tracker
            .shared
            .persistence
            .as_ref()
            .expect("actual secure storage")
            .secure_delete(
                &SecureStorageLocation::new("enrollment_clock_v1", id.to_string()),
                &[SecureStorageCapability::Delete],
            )
            .await
            .expect("remove only the clock record as crash/corruption fault");
        assert!(
            tracker
                .restore_enrollment_clock(&mut original)
                .await
                .is_err(),
            "existing enrollment cannot recreate missing required clock state"
        );
    }

    fn test_ceremony_id(label: &str) -> CeremonyId {
        CeremonyId::new(label)
    }

    fn test_time() -> Arc<dyn PhysicalTimeEffects> {
        Arc::new(PhysicalTimeHandler::new())
    }

    #[tokio::test]
    async fn test_ceremony_registration() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
                Hash32([1; 32]),
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.threshold_k, 2);
        assert_eq!(state.total_n, 3);
        assert_eq!(state.participants.len(), 3);
        assert_eq!(state.accepted_participants.len(), 0);
    }

    #[tokio::test]
    async fn test_kind_specific_timeouts_are_applied_on_registration() {
        let tracker = CeremonyTracker::new(test_time());
        let authority = AuthorityId::new_from_entropy([9u8; 32]);

        tracker
            .register(
                test_ceremony_id("enrollment-timeout"),
                CeremonyKind::DeviceEnrollment,
                authority,
                1,
                1,
                vec![ParticipantIdentity::device(DeviceId::new_from_entropy(
                    [1u8; 32],
                ))],
                1,
                Some(DeviceId::new_from_entropy([1u8; 32])),
                Some("phone".to_string()),
                Hash32([9; 32]),
            )
            .await
            .unwrap();
        tracker
            .register(
                test_ceremony_id("recovery-timeout"),
                CeremonyKind::Recovery,
                authority,
                1,
                1,
                vec![ParticipantIdentity::guardian(authority)],
                1,
                None,
                None,
                Hash32([10; 32]),
            )
            .await
            .unwrap();

        let enrollment = tracker
            .get(&test_ceremony_id("enrollment-timeout"))
            .await
            .unwrap();
        let recovery = tracker
            .get(&test_ceremony_id("recovery-timeout"))
            .await
            .unwrap();

        assert_eq!(enrollment.timeout, Duration::from_secs(600));
        // Recovery waits on guardians approving on their devices (Task 61).
        assert_eq!(recovery.timeout, Duration::from_secs(600));
    }

    #[tokio::test]
    async fn test_guardian_acceptance() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
                Hash32([2; 32]),
            )
            .await
            .unwrap();

        // First acceptance
        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!threshold_reached);

        // Second acceptance - threshold reached
        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.accepted_participants.len(), 2);
        assert!(state
            .accepted_participants
            .contains(&ParticipantIdentity::guardian(a)));
        assert!(state
            .accepted_participants
            .contains(&ParticipantIdentity::guardian(b)));
    }

    #[tokio::test]
    async fn test_ceremony_completion() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
                Hash32([3; 32]),
            )
            .await
            .unwrap();

        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());

        let threshold_reached = tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();
        assert!(threshold_reached);

        // Completion is only true once the key rotation is committed.
        assert!(!tracker.is_complete(&ceremony_id).await.unwrap());
        tracker.mark_committed(&ceremony_id).await.unwrap();
        assert!(tracker.is_complete(&ceremony_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                2,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                ],
                100,
                None,
                None,
                Hash32([4; 32]),
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(b))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_idempotent_acceptance() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
                Hash32([5; 32]),
            )
            .await
            .unwrap();

        // Accept twice
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.accepted_participants.len(), 1);
    }

    #[tokio::test]
    async fn count_only_enrollment_cannot_record_or_commit() {
        let tracker = CeremonyTracker::new(test_time());
        let ceremony_id = test_ceremony_id("count-only-enrollment");
        let authority = AuthorityId::new_from_entropy([91; 32]);
        let device = DeviceId::new_from_entropy([92; 32]);
        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceEnrollment,
                authority,
                1,
                1,
                vec![ParticipantIdentity::device(device)],
                100,
                Some(device),
                None,
                Hash32([93; 32]),
            )
            .await
            .unwrap();
        assert!(tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device))
            .await
            .is_err());
        // Model a corrupt legacy count without manufacturing verified evidence.
        tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&ceremony_id)
            .unwrap()
            .accepted_participants
            .insert(ParticipantIdentity::device(device));
        assert!(tracker
            .require_verified_enrollment_response(&ceremony_id)
            .await
            .is_err());
        assert!(tracker
            .complete(&ceremony_id, CeremonyTerminalOutcome::Committed)
            .await
            .is_err());
        assert!(!tracker.get(&ceremony_id).await.unwrap().is_committed);
    }

    #[tokio::test]
    async fn test_ceremony_failure() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-1");
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        let c = AuthorityId::new_from_entropy([3u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::GuardianRotation,
                a,
                2,
                3,
                vec![
                    ParticipantIdentity::guardian(a),
                    ParticipantIdentity::guardian(b),
                    ParticipantIdentity::guardian(c),
                ],
                100,
                None,
                None,
                Hash32([6; 32]),
            )
            .await
            .unwrap();

        tracker
            .mark_failed(&ceremony_id, Some("Test failure".to_string()))
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert!(state.has_failed);
        assert_eq!(state.error_message, Some("Test failure".to_string()));
    }

    #[tokio::test]
    async fn test_mark_failed_rejects_committed_ceremony() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-committed");
        let a = AuthorityId::new_from_entropy([21u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::Invitation,
                a,
                1,
                1,
                vec![ParticipantIdentity::guardian(a)],
                0,
                None,
                None,
                Hash32([7; 32]),
            )
            .await
            .unwrap();

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker.mark_committed(&ceremony_id).await.unwrap();
        let conflict = tracker
            .mark_failed(&ceremony_id, Some("should be ignored".to_string()))
            .await
            .expect_err("a committed ceremony cannot fail later");
        assert!(matches!(conflict, AuraError::Invalid { .. }));

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(
            state.terminal_outcome,
            Some(CeremonyTerminalOutcome::Committed)
        );
        assert!(state.is_committed);
        assert!(!state.has_failed);
        assert_eq!(state.error_message, None);
    }

    #[tokio::test]
    async fn terminal_result_is_set_once_and_preserves_first_failure() {
        let tracker = CeremonyTracker::new(test_time());
        let ceremony_id = test_ceremony_id("single-terminal");
        let participant = AuthorityId::new_from_entropy([71; 32]);
        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::Invitation,
                participant,
                1,
                1,
                vec![ParticipantIdentity::guardian(participant)],
                0,
                None,
                None,
                Hash32([17; 32]),
            )
            .await
            .unwrap();

        tracker
            .fail_with_reason(
                &ceremony_id,
                CeremonyFailureReason::Rejected,
                Some("participant refused".to_string()),
            )
            .await
            .unwrap();
        tracker
            .fail_with_reason(
                &ceremony_id,
                CeremonyFailureReason::Rejected,
                Some("later diagnostic".to_string()),
            )
            .await
            .unwrap();
        assert!(tracker.mark_committed(&ceremony_id).await.is_err());
        assert!(tracker
            .fail_with_reason(&ceremony_id, CeremonyFailureReason::TimedOut, None)
            .await
            .is_err());
        assert!(tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(participant))
            .await
            .is_err());
        assert_eq!(
            tracker.terminal_outcome(&ceremony_id).await.unwrap(),
            Some(CeremonyTerminalOutcome::Failed(
                CeremonyFailureReason::Rejected
            ))
        );
        assert_eq!(
            tracker
                .get(&ceremony_id)
                .await
                .unwrap()
                .error_message
                .as_deref(),
            Some("participant refused")
        );
    }

    #[tokio::test]
    async fn test_cleanup_timed_out_skips_committed_ceremony() {
        let tracker = CeremonyTracker::new(test_time());

        let ceremony_id = test_ceremony_id("ceremony-timeout-committed");
        let a = AuthorityId::new_from_entropy([22u8; 32]);

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::Invitation,
                a,
                1,
                1,
                vec![ParticipantIdentity::guardian(a)],
                0,
                None,
                None,
                Hash32([8; 32]),
            )
            .await
            .unwrap();

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::guardian(a))
            .await
            .unwrap();
        tracker.mark_committed(&ceremony_id).await.unwrap();

        {
            let mut guard = tracker.shared.state.write().await;
            let state = guard.ceremonies.get_mut(&ceremony_id).unwrap();
            state.started_at.ts_ms = state.started_at.ts_ms.saturating_sub(60_000);
            state.timeout = Duration::from_millis(1);
            state.timeout_budget =
                aura_core::TimeoutBudget::from_start_and_timeout(&state.started_at, state.timeout)
                    .expect("explicit expired fixture window");
        }

        let cleaned = tracker
            .cleanup_timed_out()
            .await
            .expect("cleanup owner succeeds");
        assert_eq!(cleaned, 0);

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert!(state.is_committed);
        assert!(!state.has_failed);
    }

    #[tokio::test]
    async fn unanswered_ceremony_times_out_once() {
        let clock = aura_testkit::time::ControllableTimeSource::new(1_000);
        let tracker = CeremonyTracker::new(Arc::new(clock.clone()));
        let ceremony_id = test_ceremony_id("unanswered-terminal");
        let participant = AuthorityId::new_from_entropy([72; 32]);
        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::Invitation,
                participant,
                1,
                1,
                vec![ParticipantIdentity::guardian(participant)],
                0,
                None,
                None,
                Hash32([18; 32]),
            )
            .await
            .unwrap();
        clock.advance_time(600_001);
        assert_eq!(
            tracker
                .cleanup_timed_out()
                .await
                .expect("cleanup owner succeeds"),
            1
        );
        assert_eq!(
            tracker
                .cleanup_timed_out()
                .await
                .expect("cleanup owner succeeds"),
            0
        );
        assert_eq!(
            tracker.terminal_outcome(&ceremony_id).await.unwrap(),
            Some(CeremonyTerminalOutcome::Failed(
                CeremonyFailureReason::TimedOut
            ))
        );
        assert!(tracker.mark_committed(&ceremony_id).await.is_err());
    }

    #[test]
    fn enrollment_terminal_outcome_replays_after_tracker_restart() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, _invitee, invitation, start, _acceptance, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "tracker-terminal-replay",
                ),
            )
            .await;
            let effects = issuer.runtime().effects().clone();
            let time: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
            let ceremony_id = start.ceremony_id;
            issuer
                .invitations()
                .expect("issuer invitation service")
                .cancel(&invitation.invitation_id)
                .await
                .expect("actual issued cancellation owner");
            let restarted = CeremonyTracker::new_with_storage(time, effects);
            assert!(restarted
                .list_device_enrollment_ceremonies()
                .await
                .unwrap()
                .contains(&ceremony_id));
            assert_eq!(
                restarted.terminal_outcome(&ceremony_id).await.unwrap(),
                Some(CeremonyTerminalOutcome::Failed(
                    CeremonyFailureReason::Cancelled
                ))
            );
        });
    }

    #[tokio::test]
    async fn pending_enrollment_recovery_times_out_on_fake_clock() {
        let (first, clock, ceremony_id) =
            registered_clock_fixture("restarted-pending-enrollment").await;
        let effects = first
            .shared
            .persistence
            .as_ref()
            .expect("actual durable clock owner")
            .clone();
        let restarted = CeremonyTracker::new_with_storage(Arc::new(clock.clone()), effects);
        assert_eq!(
            restarted.terminal_outcome(&ceremony_id).await.unwrap(),
            None
        );
        clock.set_time(605_001);
        assert_eq!(
            first
                .cleanup_timed_out()
                .await
                .expect("registered timeout owner persists failure"),
            1
        );
        assert_eq!(
            restarted.terminal_outcome(&ceremony_id).await.unwrap(),
            Some(CeremonyTerminalOutcome::Failed(
                CeremonyFailureReason::TimedOut
            ))
        );
    }

    #[test]
    fn test_device_enrollment_ceremony_acceptance() {
        std::thread::Builder::new().stack_size(32 * 1024 * 1024).spawn(|| {
            tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
                let (issuer, _invitee, _invitation, start, _acceptance, verified) =
                    crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture("tracker-owned-acceptance").await;
                let tracker = issuer.runtime().ceremony_tracker();
                let runner = issuer.runtime().ceremony_runner();
                let state = tracker.get(&start.ceremony_id).await.unwrap();
                assert_eq!(state.kind, CeremonyKind::DeviceEnrollment);
                assert_eq!(state.threshold_k, 1);
                assert_eq!(state.total_n, 1);
                assert!(tracker.mark_accepted(&start.ceremony_id,
                    ParticipantIdentity::device(start.device_id)).await.is_err(),
                    "generic counts cannot substitute for verified remote evidence");
                assert_eq!(runner.terminal_outcome(&start.ceremony_id).await.unwrap(), None);
                assert!(runner.record_verified_enrollment_response(verified).await.unwrap());
                let service = crate::handlers::device_epoch_rotation::DeviceEpochRotationService::new(
                    issuer.authority_id(), issuer.runtime().effects(),
                    issuer.runtime().ceremony_tracker().clone(), runner.clone(),
                    issuer.runtime().threshold_signing(), issuer.runtime().reconfiguration().clone(),
                );
                tokio::time::timeout(std::time::Duration::from_secs(10),
                    service.finalize_sole_device_enrollment(&start.ceremony_id))
                    .await.expect("bounded finalizer").expect("actual finalizer commits verified acceptance");
                assert_eq!(runner.terminal_outcome(&start.ceremony_id).await.unwrap(),
                    Some(CeremonyTerminalOutcome::Committed));
                assert!(tracker.is_complete(&start.ceremony_id).await.unwrap());
            });
        }).unwrap().join().unwrap();
    }

    #[tokio::test]
    async fn test_device_rotation_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new(test_time());
        let device_a = DeviceId::new_from_entropy([10u8; 32]);
        let device_b = DeviceId::new_from_entropy([11u8; 32]);
        let initiator = AuthorityId::new_from_entropy([2u8; 32]);
        let ceremony_id = test_ceremony_id("ceremony-rotate-1");

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceRotation,
                initiator,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                77,
                None,
                None,
                Hash32([10; 32]),
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    #[tokio::test]
    async fn test_device_removal_agreement_mode_transitions() {
        let tracker = CeremonyTracker::new(test_time());
        let device_a = DeviceId::new_from_entropy([12u8; 32]);
        let device_b = DeviceId::new_from_entropy([13u8; 32]);
        let initiator = AuthorityId::new_from_entropy([3u8; 32]);
        let ceremony_id = test_ceremony_id("ceremony-remove-1");

        tracker
            .register(
                ceremony_id.clone(),
                CeremonyKind::DeviceRemoval,
                initiator,
                2,
                2,
                vec![
                    ParticipantIdentity::device(device_a),
                    ParticipantIdentity::device(device_b),
                ],
                88,
                Some(device_b),
                None,
                Hash32([11; 32]),
            )
            .await
            .unwrap();

        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::CoordinatorSoftSafe);

        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_a))
            .await
            .unwrap();
        tracker
            .mark_accepted(&ceremony_id, ParticipantIdentity::device(device_b))
            .await
            .unwrap();

        tracker.mark_committed(&ceremony_id).await.unwrap();
        let state = tracker.get(&ceremony_id).await.unwrap();
        assert_eq!(state.agreement_mode, AgreementMode::ConsensusFinalized);
    }

    // =========================================================================
    // PROPERTY TESTS
    // =========================================================================

    use proptest::prelude::*;

    /// Strategy to generate a valid TrackedCeremony
    #[allow(dead_code)] // Reserved for future proptest expansion
    fn tracked_ceremony_strategy() -> impl Strategy<Value = TrackedCeremony> {
        (
            2usize..=8, // num_participants
            1u16..=8,   // threshold (will be clamped)
        )
            .prop_flat_map(|(num_participants, threshold)| {
                let threshold = threshold.min(num_participants as u16);
                let participants: Vec<ParticipantIdentity> = (0..num_participants)
                    .map(|i| {
                        ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32]))
                    })
                    .collect();

                // Generate a subset of participants to be accepted
                let num_accepted = 0..=num_participants;

                (Just(participants), Just(threshold), num_accepted)
            })
            .prop_map(|(participants, threshold, num_accepted)| {
                let accepted: HashSet<_> =
                    participants.iter().take(num_accepted).cloned().collect();
                let participants_set: HashSet<_> = participants.into_iter().collect();

                TrackedCeremony {
                    ceremony_id: CeremonyId::new("proptest"),
                    kind: CeremonyKind::GuardianRotation,
                    initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                    threshold_k: threshold,
                    total_n: participants_set.len() as u16,
                    participants: participants_set,
                    accepted_participants: accepted,
                    new_epoch: 100,
                    enrollment_device_id: None,
                    enrollment_nickname_suggestion: None,
                    started_at: PhysicalTime {
                        ts_ms: 0,
                        uncertainty: None,
                    },
                    has_failed: false,
                    is_committed: false,
                    is_superseded: false,
                    superseded_by: None,
                    supersedes: Vec::new(),
                    agreement_mode: AgreementMode::CoordinatorSoftSafe,
                    error_message: None,
                    terminal_outcome: None,
                    failure_reason: None,
                    timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
                    timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(
                        &PhysicalTime::exact(0),
                        Duration::from_secs(30),
                    )
                    .expect("valid fixture original window"),
                    prestate_hash: Hash32([0; 32]),
                    committed_at: None,
                    committed_consensus_id: None,
                }
            })
    }

    proptest! {
        /// Property: Participants list has no duplicates.
        /// This invariant is enforced by the validate() function.
        #[test]
        fn prop_no_duplicate_participants(
            num_participants in 2usize..=8,
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 1,
                        total_n: participants_set.len() as u16,
                        participants: participants_set,
                        accepted_participants: HashSet::new(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        terminal_outcome: None,
                        failure_reason: None,
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            // Unique participants should pass validation
            prop_assert!(state.validate().is_ok());

            // Verify HashSet uniqueness invariant
            let participant_set: HashSet<_> = participants.iter().collect();
            prop_assert_eq!(participant_set.len(), participants.len());
        }

        /// Property: Accepted participants is always a subset of participants.
        #[test]
        fn prop_accepted_subset_of_participants(
            num_participants in 2usize..=8,
            num_accepted in 0usize..=8
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            // Take a valid subset of participants as accepted
            let num_accepted = num_accepted.min(num_participants);
            let accepted: HashSet<_> = participants.iter().take(num_accepted).cloned().collect();

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 1,
                        total_n: num_participants as u16,
                        participants: participants_set.clone(),
                        accepted_participants: accepted.clone(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        terminal_outcome: None,
                        failure_reason: None,
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            // Valid subset should pass validation
            prop_assert!(state.validate().is_ok());

            // Verify subset relationship
            prop_assert!(accepted.is_subset(&participants_set));
        }

        /// Property: Threshold must be <= total participants.
        #[test]
        fn prop_threshold_within_bounds(
            num_participants in 1usize..=8,
            threshold in 1u16..=8
        ) {
            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let participants_set: HashSet<_> = participants.iter().cloned().collect();

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: threshold,
                        total_n: num_participants as u16,
                        participants: participants_set,
                        accepted_participants: HashSet::new(),
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed: false,
                        is_committed: false,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::CoordinatorSoftSafe,
                        error_message: None,
                        terminal_outcome: None,
                        failure_reason: None,
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Should succeed iff threshold <= num_participants
            if threshold as usize <= num_participants {
                prop_assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
            } else {
                prop_assert!(result.is_err(), "Expected Err for threshold {} > total {}", threshold, num_participants);
            }
        }

        /// Property: Committed ceremonies must have threshold met.
        /// is_committed implies accepted_participants.len() >= threshold_k
        #[test]
        fn prop_committed_implies_threshold_met(
            num_participants in 2usize..=8,
            threshold in 1u16..=8,
            num_accepted in 0usize..=8
        ) {
            let threshold = threshold.min(num_participants as u16);
            let num_accepted = num_accepted.min(num_participants);

            let participants: Vec<ParticipantIdentity> = (0..num_participants)
                .map(|i| ParticipantIdentity::guardian(AuthorityId::new_from_entropy([i as u8; 32])))
                .collect();
            let accepted: HashSet<_> = participants.iter().take(num_accepted).cloned().collect();
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let threshold_met = num_accepted >= threshold as usize;

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: threshold,
                        total_n: num_participants as u16,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed: false,
                        is_committed: true, // Mark as committed
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: AgreementMode::ConsensusFinalized,
                        error_message: None,
                        terminal_outcome: Some(CeremonyTerminalOutcome::Committed),
                        failure_reason: None,
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Should succeed iff threshold is met
            if threshold_met {
                prop_assert!(result.is_ok(), "Expected Ok when threshold met, got: {:?}", result);
            } else {
                prop_assert!(result.is_err(), "Expected Err for committed ceremony without threshold");
            }
        }

        /// Property: Committed and failed are mutually exclusive.
        #[test]
        fn prop_committed_and_failed_mutually_exclusive(
            is_committed in any::<bool>(),
            has_failed in any::<bool>()
        ) {
            let a = AuthorityId::new_from_entropy([1u8; 32]);
            let b = AuthorityId::new_from_entropy([2u8; 32]);
            let participants = vec![
                ParticipantIdentity::guardian(a),
                ParticipantIdentity::guardian(b),
            ];
            let accepted: HashSet<_> = participants.iter().cloned().collect(); // All accepted for threshold
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 2,
                        total_n: 2,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed,
                        is_committed,
                        is_superseded: false,
                        superseded_by: None,
                        supersedes: Vec::new(),
                        agreement_mode: if is_committed {
                            AgreementMode::ConsensusFinalized
                        } else {
                            AgreementMode::CoordinatorSoftSafe
                        },
                        error_message: if has_failed { Some("test".to_string()) } else { None },
                        terminal_outcome: if is_committed {
                            Some(CeremonyTerminalOutcome::Committed)
                        } else if has_failed {
                            Some(CeremonyTerminalOutcome::Failed(CeremonyFailureReason::RuntimeFailed))
                        } else {
                            None
                        },
                        failure_reason: has_failed.then_some(CeremonyFailureReason::RuntimeFailed),
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Both true => error
            if is_committed && has_failed {
                prop_assert!(result.is_err());
            }
        }

        /// Property: Superseded and committed are mutually exclusive.
        #[test]
        fn prop_superseded_and_committed_mutually_exclusive(
            is_committed in any::<bool>(),
            is_superseded in any::<bool>()
        ) {
            let a = AuthorityId::new_from_entropy([1u8; 32]);
            let b = AuthorityId::new_from_entropy([2u8; 32]);
            let participants = vec![
                ParticipantIdentity::guardian(a),
                ParticipantIdentity::guardian(b),
            ];
            let accepted: HashSet<_> = participants.iter().cloned().collect(); // All accepted for threshold
            let participants_set: HashSet<_> = participants.into_iter().collect();

            let state = CeremonyTrackerState {
                enrollment_responses: HashMap::new(),
                retired_enrollment_ids: HashSet::new(),
                ceremonies: {
                    let mut map = HashMap::new();
                    map.insert(test_ceremony_id("test"), TrackedCeremony {
                        ceremony_id: test_ceremony_id("test"),
                        kind: CeremonyKind::GuardianRotation,
                        initiator_id: AuthorityId::new_from_entropy([0u8; 32]),
                        threshold_k: 2,
                        total_n: 2,
                        participants: participants_set,
                        accepted_participants: accepted,
                        new_epoch: 100,
                        enrollment_device_id: None,
                        enrollment_nickname_suggestion: None,
                        started_at: PhysicalTime { ts_ms: 0, uncertainty: None },
                        has_failed: is_superseded, // Superseded ceremonies are marked failed
                        is_committed,
                        is_superseded,
                        superseded_by: if is_superseded {
                            Some(test_ceremony_id("other"))
                        } else {
                            None
                        },
                        supersedes: Vec::new(),
                        agreement_mode: if is_committed {
                            AgreementMode::ConsensusFinalized
                        } else {
                            AgreementMode::CoordinatorSoftSafe
                        },
                        error_message: None,
                        terminal_outcome: if is_committed {
                            Some(CeremonyTerminalOutcome::Committed)
                        } else if is_superseded {
                            Some(CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Superseded))
                        } else {
                            None
                        },
                        failure_reason: is_superseded.then_some(CeremonyFailureReason::Superseded),
                        timeout: Duration::from_secs(30),
                    enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
            timeout_budget: aura_core::TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(0), Duration::from_secs(30)).expect("valid fixture original window"),
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    });
                    map
                },
                supersession_records: Vec::new(),
            };

            let result = state.validate();

            // Both true => error
            if is_committed && is_superseded {
                prop_assert!(result.is_err());
            }
        }
    }
    #[test]
    fn retained_notice_identity_rejects_digest_transcript_or_expiry_replacement() {
        // Pure identity comparison laws. These local values are not admitted
        // runtime owners and never enter a signature or mutation boundary.
        let original = RegisteredEnrollmentNoticeBindingCapability {
            manifest_digest: [0x41; 32],
            transcript: vec![1, 2, 3],
            expires_at_ms: 400,
        };
        let exact = RegisteredEnrollmentNoticeBindingCapability {
            manifest_digest: [0x41; 32],
            transcript: vec![1, 2, 3],
            expires_at_ms: 400,
        };
        RegisteredEnrollmentWindowCapability::require_same_notice_binding(&original, &exact)
            .expect("same canonical identity may be observed repeatedly");
        let replacements = [
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x42; 32],
                transcript: vec![1, 2, 3],
                expires_at_ms: 400,
            },
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x41; 32],
                transcript: vec![1, 2, 4],
                expires_at_ms: 400,
            },
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x41; 32],
                transcript: vec![1, 2, 3],
                expires_at_ms: 401,
            },
        ];
        for candidate in replacements {
            let error = RegisteredEnrollmentWindowCapability::require_same_notice_binding(
                &original, &candidate,
            )
            .expect_err("an original notice identity cannot be replaced");
            assert!(
                matches!(
                    std::error::Error::source(&error)
                        .and_then(|cause| cause.downcast_ref::<aura_core::TimeoutBudgetError>()),
                    Some(aura_core::TimeoutBudgetError::CheckpointDiscontinuity { .. })
                ),
                "identity conflict retains its concrete ownership cause"
            );
        }
    }
}

#[cfg(test)]
mod registered_window_admission_tests {
    use super::*;
    #[tokio::test]
    async fn actual_semaphore_busy_and_closed_remain_distinct_sources() {
        use std::error::Error;
        let lease = Arc::new(tokio::sync::Semaphore::new(1));
        let owner = lease
            .clone()
            .try_acquire_owned()
            .expect("actual first owner");
        let busy = registered_window_lease_error(
            lease
                .clone()
                .try_acquire_owned()
                .expect_err("actual owner holds permit"),
        );
        assert!(registered_enrollment_window_already_owned(&busy));
        let cause = busy
            .source()
            .expect("typed admission")
            .source()
            .expect("native semaphore source");
        assert!(matches!(
            cause.downcast_ref::<tokio::sync::TryAcquireError>(),
            Some(tokio::sync::TryAcquireError::NoPermits)
        ));
        lease.close();
        let closed = registered_window_lease_error(
            lease
                .try_acquire_owned()
                .expect_err("actual owner closed admission"),
        );
        assert!(!registered_enrollment_window_already_owned(&closed));
        assert!(matches!(
            closed
                .source()
                .expect("typed admission")
                .downcast_ref::<RegisteredEnrollmentWindowAdmissionError>(),
            Some(RegisteredEnrollmentWindowAdmissionError::Closed { .. })
        ));
        drop(owner);
    }
}

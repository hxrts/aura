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

enum OriginalRegistrationClock<'a> {
    SelectAtRegistration,
    SelectedEnrollment(&'a mut super::enrollment_window::OriginalIssuerLaunchWindow),
}

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
    version: u16,
    ceremony: CeremonyId,
    subject: AuthorityId,
    device: DeviceId,
    prestate: Hash32,
    epoch: u64,
    budget: aura_core::TimeoutBudget,
    negative_cleanup_budget: aura_core::TimeoutBudget,
    original_policy: super::enrollment_window::OriginalEnrollmentWindowPolicy,
    execution: super::enrollment_window::DurableEnrollmentExecutionState,
}

pub(super) struct OriginalIssuerNegativeCheckpointReceipt {
    original: Arc<RegisteredEnrollmentWindowCapability>,
    retained_decision: super::enrollment_window::DurableEnrollmentExecutionState,
}
impl OriginalIssuerNegativeCheckpointReceipt {
    pub(super) fn require_original(
        &self,
        original: &Arc<RegisteredEnrollmentWindowCapability>,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.original, original)
            || !matches!(
                self.retained_decision,
                super::enrollment_window::DurableEnrollmentExecutionState::Closed
                    | super::enrollment_window::DurableEnrollmentExecutionState::Refused
            )
        {
            return Err(AuraError::invalid(
                "negative checkpoint has another original decision owner",
            ));
        }
        Ok(())
    }
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
    original_policy: super::enrollment_window::OriginalEnrollmentWindowPolicy,
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
            original_policy: state.original_window_policy.clone(),
        })
    }
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
pub(crate) struct RegisteredEnrollmentWindowCapability {
    tracker: OriginalEnrollmentTrackerReference,
    state: TrackedCeremony,
    lease: Arc<OriginalRegisteredExecutionLease>,
    notice_binding: std::sync::OnceLock<Arc<RegisteredEnrollmentNoticeBindingCapability>>,
    negative_decision: std::sync::OnceLock<OriginalEnrollmentNegativeDecisionReceipt>,
    verified_rejection: std::sync::OnceLock<
        crate::handlers::invitation::enrollment_trust::VerifiedEnrollmentRejectionCapability,
    >,
    positive_decision: std::sync::OnceLock<OriginalEnrollmentPositiveDecisionReceipt>,
    interrupted_resources: std::sync::OnceLock<()>,
    #[cfg(test)]
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    committed_confirmation_gate: parking_lot::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
    _original_allocation: crate::runtime::effects::OriginalEnrollmentAllocationAcknowledgment,
}

/// Provenance of the actual protected first negative decision, retained in its
/// original registered owner. Neither a status snapshot nor stored bytes can
/// construct this receipt. Weak custody avoids a cycle through the owner slot.
pub(crate) struct OriginalEnrollmentNegativeDecisionReceipt {
    original: std::sync::Weak<RegisteredEnrollmentWindowCapability>,
    kind: aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind,
    decision_digest: [u8; 32],
}
impl OriginalEnrollmentNegativeDecisionReceipt {
    pub(crate) fn kind(
        &self,
    ) -> aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind {
        self.kind
    }
    pub(crate) fn decision_digest(&self) -> [u8; 32] {
        self.decision_digest
    }
    pub(crate) fn original_owner_for_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<std::sync::Weak<RegisteredEnrollmentWindowCapability>, AuraError> {
        self.require_original_control(issued)?;
        Ok(self.original.clone())
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalEnrollmentNegativeDecisionReceipt",
        receiver_type = OriginalEnrollmentNegativeDecisionReceipt,
        family = "runtime_helper"
    )]
    pub(crate) fn require_original_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        let original = self.original.upgrade().ok_or_else(|| {
            AuraError::permission_denied("original negative decision owner was lost")
        })?;
        original.require_issued_notice_control(issued)?;
        let retained = original.negative_decision.get().ok_or_else(|| {
            AuraError::permission_denied("original negative decision is not retained")
        })?;
        if !std::ptr::eq(retained, self) {
            return Err(AuraError::permission_denied(
                "foreign original negative decision receipt",
            ));
        }
        Ok(())
    }
}

/// Actual original held activation and protected Committed decision. This token
/// cannot be reconstructed from a terminal snapshot, wire frame or stored bytes.
pub(crate) struct OriginalEnrollmentPositiveDecisionReceipt {
    original: std::sync::Weak<RegisteredEnrollmentWindowCapability>,
    decision_digest: [u8; 32],
    committed_ops: Vec<aura_core::AttestedOp>,
}
impl OriginalEnrollmentPositiveDecisionReceipt {
    pub(crate) fn committed_ops(&self) -> &[aura_core::AttestedOp] {
        &self.committed_ops
    }
    pub(crate) fn decision_digest(&self) -> [u8; 32] {
        self.decision_digest
    }
    pub(crate) fn original_owner_for_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<std::sync::Weak<RegisteredEnrollmentWindowCapability>, AuraError> {
        self.require_original_control(issued)?;
        Ok(self.original.clone())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalEnrollmentPositiveDecisionReceipt", receiver_type = OriginalEnrollmentPositiveDecisionReceipt, family = "runtime_helper")]
    pub(crate) fn require_original_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        let original = self.original.upgrade().ok_or_else(|| {
            AuraError::permission_denied("original positive decision owner was lost")
        })?;
        original.require_issued_notice_control(issued)?;
        let actual = original.positive_decision.get().ok_or_else(|| {
            AuraError::permission_denied("original positive decision is not retained")
        })?;
        if !std::ptr::eq(actual, self) {
            return Err(AuraError::permission_denied(
                "foreign original positive decision receipt",
            ));
        }
        Ok(())
    }
}

/// Actual protected retirement writes sealed through the original execution owner.
/// Neither protected bytes nor supervision completion can construct this receipt.
pub(crate) struct RejectedOriginalEnrollmentRetirement {
    pub(crate) source: AuraError,
    // Retains actual rejected write custody until its enclosing owner settles.
    pub(crate) _writes: Box<crate::runtime::effects::AcknowledgedEnrollmentRetirement>,
}
pub(crate) struct OriginalEnrollmentRetirementAcknowledgment {
    original: Arc<RegisteredEnrollmentWindowCapability>,
    writes: crate::runtime::effects::AcknowledgedEnrollmentRetirement,
}
impl OriginalEnrollmentRetirementAcknowledgment {
    pub(crate) fn require_original(
        &self,
        original: &Arc<RegisteredEnrollmentWindowCapability>,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.original, original) {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
            ));
        }
        original.require_issued_notice_control(issued)?;
        self.writes
            .require_original(original.tracker.effects.as_ref(), issued)
    }
}

/// Actual fresh Pending publication acknowledgment, never decoded or cloned.
/// Only the original generation owner can produce this at its durable birth.
pub(super) struct AcknowledgedIssuerEnrollmentBirth {
    original: RegisteredEnrollmentWindowCapability,
}

impl AcknowledgedIssuerEnrollmentBirth {
    pub(super) fn into_original_window(self) -> RegisteredEnrollmentWindowCapability {
        self.original
    }
}

/// Completion observation retains original clock identity without an execution
/// permit. It cannot admit a session, sign, bind notices, or manufacture children.
pub(super) struct HeldIssuerClockObservationCapability {
    tracker: OriginalEnrollmentTrackerReference,
    state: TrackedCeremony,
}

/// Exact original actor identity without a cycle through its retained root.
/// The provider stays retained; losing the actor never reconstructs its owner.
#[derive(Clone)]
struct OriginalEnrollmentTrackerReference {
    shared: std::sync::Weak<CeremonyTrackerShared>,
    time: Arc<dyn PhysicalTimeEffects>,
    effects: Arc<AuraEffectSystem>,
}

#[derive(Debug, thiserror::Error)]
enum OriginalEnrollmentTrackerError {
    #[error("the original enrollment tracker no longer owns this execution")]
    OwnerLost,
    #[error("the original enrollment tracker has no selected persistent provider")]
    MissingProvider,
    #[error("enrollment execution selected a different physical provider")]
    ForeignProvider,
    #[error("the original enrollment Pending birth was already published")]
    AlreadyBorn,
    #[error("the original enrollment Pending publication readback changed")]
    BirthReadbackChanged,
    #[error("original issuer task disposal did not complete successfully")]
    DeliveryInterrupted,
    #[error("original signed issuer acknowledgment is negative")]
    NegativeDeliveryAcknowledgment,
}

fn original_tracker_refusal(source: OriginalEnrollmentTrackerError) -> AuraError {
    AuraError::PermissionDenied {
        message: "original enrollment tracker custody refused".into(),
        source: Some(Arc::new(source)),
    }
}

impl OriginalEnrollmentTrackerReference {
    fn from_original(tracker: &CeremonyTracker) -> Result<Self, AuraError> {
        let effects = tracker.shared.persistence.clone().ok_or_else(|| {
            original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
        })?;
        Ok(Self {
            shared: Arc::downgrade(&tracker.shared),
            time: tracker.time.clone(),
            effects,
        })
    }

    fn upgrade(&self) -> Result<CeremonyTracker, AuraError> {
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        Ok(CeremonyTracker {
            shared,
            time: self.time.clone(),
        })
    }

    fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.upgrade()?;
        if !std::ptr::eq(self.effects.as_ref(), effects) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::ForeignProvider,
            ));
        }
        Ok(())
    }
}
impl HeldIssuerClockObservationCapability {
    #[cfg(test)]
    pub(super) fn ceremony_id(&self) -> &CeremonyId {
        &self.state.ceremony_id
    }
    pub(super) async fn require_pending(&self) -> Result<(), AuraError> {
        self.tracker.require_pending(&self.state).await
    }
    pub(super) fn budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.timeout_budget
    }
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.tracker.require_effects(effects)
    }
}
/// Exact issuer identity retained by the original registered window. This
/// runtime-local binding is never decoded from storage or peer wire data.
pub(crate) struct RegisteredEnrollmentNoticeBindingCapability {
    manifest_digest: [u8; 32],
    transcript: Vec<u8>,
    expires_at_ms: u64,
    canonical_invitation: Vec<u8>,
}
/// Read-only observation rooted in the original registered entry. The unique
/// task control and terminal acknowledgment remain in that entry while waiting.
pub(crate) struct OriginalIssuerDeliveryObservation {
    original: Arc<RegisteredEnrollmentWindowCapability>,
    disposal: crate::task_registry::OriginalRequiredTaskDisposalObserver,
}
impl OriginalIssuerDeliveryObservation {
    #[cfg(test)]
    pub(crate) async fn await_disposal_or_cleanup_failure_for_test(
        &self,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObservation, AuraError> {
        self.disposal
            .await_disposal_or_cleanup_failure_for_test()
            .await
    }

    /// Caller bounds this wait with the once-issued shortcut operation window.
    /// Cancellation drops only this observer, never the retained task control.
    pub(crate) async fn await_disposal(
        &self,
    ) -> crate::task_registry::OriginalRequiredTaskDisposalObservation {
        self.disposal.await_disposal().await
    }
    #[cfg(any(test, feature = "simulation"))]
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerDeliveryObservation", receiver_type = OriginalIssuerDeliveryObservation, family = "runtime_helper")]
    pub(crate) fn require_failed_disposal(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<AuraError, AuraError> {
        let tracker = self.original.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "validate original failed issuer disposal".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.original.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !entry
            .running
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(&actual.original, &self.original))
        {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        let task = entry
            .issuer_task
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        task.require_disposal(disposal)?;
        match disposal.outcome() {
            aura_core::effects::task::RequiredTaskExit::Failed(source) => Ok(source),
            _ => Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::DeliveryInterrupted,
            )),
        }
    }
    #[cfg(any(test, feature = "simulation"))]
    pub(crate) fn original_simulated_subtree_disposal(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>, AuraError> {
        self.require_failed_disposal(disposal)?;
        let tracker = self.original.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original disposed subtree".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.original.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, &self.original) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .simulated_original_subtree_disposal()
            .ok_or_else(|| AuraError::invalid("original sealed subtree disposal is unresolved"))
    }
    /// Validate actual original resource and domain acknowledgment without
    /// moving either. Used as the original shortcut window's real checkpoint.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerDeliveryObservation", receiver_type = OriginalIssuerDeliveryObservation, family = "runtime_helper")]
    pub(crate) fn require_positive(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let tracker = self.original.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "checkpoint original issuer delivery acknowledgment".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.original.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        self.require_positive_entry(entry, disposal)
    }
    fn require_positive_entry(
        &self,
        entry: &TrackedCeremonyEntry,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let running = entry
            .running
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, &self.original) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        let task = entry
            .issuer_task
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        task.require_disposal(disposal)?;
        match disposal.outcome() {
            aura_core::effects::task::RequiredTaskExit::Completed => {}
            aura_core::effects::task::RequiredTaskExit::Failed(source) => return Err(source),
            _ => {
                return Err(original_tracker_refusal(
                    OriginalEnrollmentTrackerError::DeliveryInterrupted,
                ))
            }
        }
        let acknowledged = entry
            .acknowledged_issuer
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        acknowledged.require_original(&self.original)?;
        if !acknowledged.positive() {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::NegativeDeliveryAcknowledgment,
            ));
        }
        Ok(())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerDeliveryObservation", receiver_type = OriginalIssuerDeliveryObservation, family = "runtime_helper")]
    pub(crate) fn consume_positive(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
        purpose: super::enrollment_window::EnrolledDeviceShortcutPurpose,
    ) -> Result<super::enrollment_window::EnrolledDeviceShortcutAuthority, AuraError> {
        let tracker = self.original.tracker.upgrade()?;
        let mut state = tracker
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "consume original issuer delivery acknowledgment".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get_mut(&self.original.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        self.require_positive_entry(entry, disposal)?;
        // All fallible checks precede the one-use removal. The unique task
        // control remains rooted in the same original entry.
        Ok(entry
            .acknowledged_issuer
            .take()
            .expect("checked original acknowledgment")
            .into_shortcut_authority(purpose))
    }
}

// Lifecycle status is a mutable observation, not part of original issuance
// identity. Preserve every other field through the existing signed wire shape.
fn original_invitation_binding(
    invitation: &aura_core::invitation::Invitation,
) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(&(
        aura_signature::invitation::ShareableInvitation::from(invitation),
        invitation.receiver_id,
        invitation.created_at,
        &invitation.receiver_nickname,
    ))
    .map_err(AuraError::from)
}

impl RegisteredEnrollmentWindowCapability {
    /// Fence the disposed original producer under the same first-decision gate.
    /// This proves resource interruption only; it creates no domain decision,
    /// terminal signature or generation-retirement authority.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RegisteredEnrollmentWindowCapability", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(super) async fn fence_original_interrupted_resources(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        destination: &super::issuer_terminal_destination::OriginalIssuerTerminalDestination,
        disposal: &crate::task_registry::TaskGroupDisposalAcknowledgment,
        children: &crate::task_registry::TaskGroup,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, AuraError> {
        self.require_issued_notice_control(issued)?;
        disposal.require_original_group(children)?;
        destination.require_unsigned_vm_disposed(issued)?;
        let tracker = self.tracker.upgrade()?;
        let guard = Arc::clone(&tracker.shared.enrollment_decision_gate)
            .lock_owned()
            .await;
        let current = tracker.get(&self.state.ceremony_id).await?;
        if current.terminal_outcome.is_some()
            || self.positive_decision.get().is_some()
            || self.negative_decision.get().is_some()
            || tracker
                .retained_terminal_decision(&self.state.ceremony_id)
                .await?
                .is_some()
        {
            return Err(AuraError::permission_denied(
                "interrupted issuer resources cannot replace an original terminal decision",
            ));
        }
        use aura_core::effects::{SecureStorageEffects, SecureStorageLocation};
        if self
            .tracker
            .effects
            .secure_exists(&SecureStorageLocation::new(
                "device_enrollment_activation_v1",
                self.state.ceremony_id.to_string(),
            ))
            .await?
        {
            return Err(AuraError::permission_denied(
                "interrupted issuer resources require original activation reconciliation",
            ));
        }
        self.interrupted_resources.get_or_init(|| ());
        Ok(guard)
    }

    pub(super) fn require_original_interrupted_resources(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        self.require_issued_notice_control(issued)?;
        if self.interrupted_resources.get().is_none()
            || self.positive_decision.get().is_some()
            || self.negative_decision.get().is_some()
        {
            return Err(AuraError::permission_denied(
                "original interruption fence missing or decided",
            ));
        }
        Ok(())
    }
    /// Original selected endpoint retained for the separate shortcut operation.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RegisteredEnrollmentWindowCapability", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(crate) fn post_enrollment_budget(&self) -> Option<aura_core::TimeoutBudget> {
        self.state.shortcut_budget.clone()
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn install_progress_error_gate_for_test(
        self: &Arc<Self>,
    ) -> Result<
        crate::runtime::services::issuer_terminal_destination::OriginalProgressErrorObservationGate,
        aura_core::AuraError,
    > {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original progress error gate owner".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .install_progress_error_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_cancelled_notice_gate_for_test(
        self: &Arc<Self>,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original notice gate owner".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .install_cancelled_notice_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_negative_cleanup_first_observation_gate_for_test(
        self: &Arc<Self>,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original notice gate owner".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .install_negative_cleanup_first_observation_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn observe_negative_cleanup_clock_for_test(
        &self,
        now: &aura_core::time::PhysicalTime,
    ) -> aura_core::TimeoutBudgetResult<std::time::Duration> {
        self.negative_cleanup_budget().remaining_at(now)
    }
    #[cfg(test)]
    pub(crate) fn install_negative_cleanup_after_observation_gate_for_test(
        self: &Arc<Self>,
    ) -> Result<
        crate::handlers::invitation_service::OriginalNegativeCleanupObservationGate,
        AuraError,
    > {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original notice gate owner".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .install_negative_cleanup_after_observation_gate_for_test()
    }
    #[cfg(test)]
    pub(crate) fn install_committed_confirmation_gate_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let mut actual = self.committed_confirmation_gate.lock();
        if actual.is_some() || self.positive_decision.get().is_some() {
            return Err(AuraError::invalid("original committed gate already used"));
        }
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *actual = Some((entered, released));
        Ok((observed, release))
    }
    #[cfg(test)]
    pub(crate) fn has_original_positive_receipt_for_test(self: &Arc<Self>) -> bool {
        self.positive_decision.get().is_some_and(|actual| {
            actual
                .original
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, self))
        })
    }

    pub(super) fn original_resource_decision(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<super::enrollment_window::OriginalIssuerResourceDecision, AuraError> {
        self.require_issued_notice_control(issued)?;
        match (self.positive_decision.get(), self.negative_decision.get()) {
            (Some(receipt), None) => {
                receipt.require_original_control(issued)?;
                Ok(super::enrollment_window::OriginalIssuerResourceDecision::Committed)
            }
            (None, Some(_)) => {
                self.require_original_verified_rejection(issued)?;
                Ok(super::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected)
            }
            (None, None) if self.interrupted_resources.get().is_some() => {
                self.require_original_interrupted_resources(issued)?;
                Ok(super::enrollment_window::OriginalIssuerResourceDecision::Interrupted)
            }
            _ => Err(AuraError::permission_denied(
                "original resource cleanup requires one exclusive protected decision",
            )),
        }
    }

    pub(crate) fn original_positive_decision(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<&OriginalEnrollmentPositiveDecisionReceipt, AuraError> {
        self.require_issued_notice_control(issued)?;
        let actual = self.positive_decision.get().ok_or_else(|| {
            AuraError::permission_denied("original positive decision has not been published")
        })?;
        actual.require_original_control(issued)?;
        Ok(actual)
    }
    pub(crate) fn original_negative_decision(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<&OriginalEnrollmentNegativeDecisionReceipt, AuraError> {
        self.require_issued_notice_control(issued)?;
        let receipt = self.negative_decision.get().ok_or_else(|| {
            AuraError::permission_denied("original negative decision has not been published")
        })?;
        receipt.require_original_control(issued)?;
        Ok(receipt)
    }
    pub(in crate::runtime) fn require_original_held_reservation(
        &self,
        reservation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
    ) -> Result<(), AuraError> {
        let tracker = self.tracker.upgrade()?;
        reservation.require_tracker(&tracker)?;
        reservation.validate_allocated_registration(self.tracker.effects.as_ref(), &self.state)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalIssuerNegativeCheckpointReceipt",
        family = "runtime_helper"
    )]
    pub(super) fn release_held_negative_execution_lease(
        self: &Arc<Self>,
        reservation: &crate::runtime::effects::EnrollmentGenerationReservation<'static>,
        retirement: &crate::runtime::effects::AcknowledgedEnrollmentRetirement,
        checkpoint: &OriginalIssuerNegativeCheckpointReceipt,
        disposal: &crate::task_registry::TaskGroupDisposalAcknowledgment,
        children: &crate::task_registry::TaskGroup,
    ) -> Result<(), AuraError> {
        self.require_original_held_reservation(reservation)?;
        retirement.require_original_held(reservation, self.tracker.effects.as_ref())?;
        checkpoint.require_original(self)?;
        disposal.require_original_group(children)?;
        self.lease.release_after_acknowledgment()
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_verified_issuer_cancellation",
        receiver_type = RegisteredEnrollmentWindowCapability,
        family = "runtime_helper"
    )]
    pub(crate) fn require_original_cancellation(
        &self,
        terminal: &crate::handlers::invitation::OriginalIssuerCancellationTerminal,
    ) -> Result<(), AuraError> {
        terminal
            .require_original_effects(&self.tracker.effects)
            .map_err(|source| match source {
                crate::core::AgentError::Aura(source) => source,
                source => AuraError::Internal {
                    message: "original cancellation validation".into(),
                    source: Some(Arc::new(source)),
                },
            })?;
        self.require_issued_notice_control(terminal.issued())
    }
    #[cfg(feature = "simulation")]
    pub(crate) fn original_simulation_ceremony(&self) -> &CeremonyId {
        &self.state.ceremony_id
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(crate) fn original_committed_decision(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<bool, AuraError> {
        self.require_effects(effects)?;
        self.require_issued_notice_control(issued)?;
        match (self.positive_decision.get(), self.negative_decision.get()) {
            (Some(actual), None) => {
                actual.require_original_control(issued)?;
                Ok(true)
            }
            (None, Some(actual)) => {
                actual.require_original_control(issued)?;
                Ok(false)
            }
            (None, None) => Ok(false),
            _ => Err(AuraError::permission_denied(
                "contradictory original enrollment decisions",
            )),
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RegisteredEnrollmentWindowCapability", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(crate) async fn await_original_resource_disposal(
        self: &Arc<Self>,
    ) -> Result<
        (
            OriginalIssuerDeliveryObservation,
            crate::task_registry::OriginalRequiredTaskDisposalObservation,
        ),
        AuraError,
    > {
        let observation = self.observe_original_delivery()?;
        let disposal = aura_core::execute_with_timeout_budget(
            self.tracker.effects.as_ref(),
            &self.state.negative_cleanup_budget,
            || async { Ok::<_, AuraError>(observation.await_disposal().await) },
        )
        .await
        .map_err(|source| match source {
            aura_core::TimeoutRunError::Operation(source) => source,
            aura_core::TimeoutRunError::Timeout(source) => AuraError::from(source),
        })?;
        Ok((observation, disposal))
    }
    pub(super) fn negative_cleanup_budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.negative_cleanup_budget
    }
    pub(super) async fn acknowledge_negative_checkpoint(
        self: &Arc<Self>,
    ) -> Result<OriginalIssuerNegativeCheckpointReceipt, AuraError> {
        let tracker = self.tracker.upgrade()?;
        self.require_effects(self.tracker.effects.as_ref())?;
        let retained_decision = tracker
            .persist_original_negative_clock(
                &self.state,
                self.tracker.effects.as_ref(),
                OriginalNegativeCheckpointOwner::Issuer,
            )
            .await?;
        Ok(OriginalIssuerNegativeCheckpointReceipt {
            original: Arc::clone(self),
            retained_decision,
        })
    }
    /// Observe only the actual original retirement receipt, independently of
    /// its retained domain failure.
    #[cfg(test)]
    pub(crate) fn observes_retained_retirement_acknowledgment(
        self: &Arc<Self>,
    ) -> Result<bool, AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original retirement custody".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        Ok(running
            .retirement_destination
            .retains_original_retirement_acknowledgment())
    }
    #[cfg(test)]
    pub(crate) fn require_original_issuer_subtree_disposal_for_test(
        self: &Arc<Self>,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        self.observe_original_delivery()?
            .original_simulated_subtree_disposal(disposal)
            .map(|_| ())
    }
    #[cfg(test)]
    pub(crate) async fn original_cancellation_disposal_for_test(
        self: &Arc<Self>,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        let original = self.original_cancellation_registration_for_test()?;
        original.disposal_observer().await
    }
    #[cfg(test)]
    pub(crate) fn original_cancellation_registration_for_test(
        self: &Arc<Self>,
    ) -> Result<Arc<OriginalCancellationTaskRegistration>, AuraError> {
        let tracker = self.tracker.upgrade()?;
        let original = {
            let state = tracker
                .shared
                .state
                .try_read()
                .map_err(|source| AuraError::Internal {
                    message: "observe original cancellation disposal entry".into(),
                    source: Some(Arc::new(source)),
                })?;
            let entry = state
                .ceremonies
                .get(&self.state.ceremony_id)
                .ok_or_else(|| {
                    original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
                })?;
            let running = entry.running.as_ref().ok_or_else(|| {
                original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
            })?;
            if !Arc::ptr_eq(&running.original, self) {
                return Err(original_tracker_refusal(
                    OriginalEnrollmentTrackerError::OwnerLost,
                ));
            }
            Arc::clone(
                &entry
                    .cancellation_task
                    .as_ref()
                    .ok_or_else(|| {
                        AuraError::invalid("original cancellation control not published")
                    })?
                    .original,
            )
        };
        Ok(original)
    }
    #[cfg(test)]
    pub(crate) fn install_original_receipt_test_hook(
        self: &Arc<Self>,
        hook: crate::handlers::invitation_service::OriginalTerminalReceiptTestHook,
    ) -> Result<(), AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "inspect original receipt destination".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        running
            .retirement_destination
            .install_receipt_test_hook(hook)
    }
    #[cfg(test)]
    pub(crate) fn original_finalizer_failure_for_test(
        self: &Arc<Self>,
    ) -> Result<Option<AuraError>, AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original retirement custody".into(),
                source: Some(Arc::new(source)),
            })?;
        let running = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .and_then(|entry| entry.running.as_ref())
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        Ok(running
            .retirement_destination
            .original_finalizer_failure_for_test())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(super) async fn await_original_terminal(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<CeremonyTerminalOutcome, AuraError> {
        self.require_issued_notice_control(issued)?;
        let tracker = self.tracker.upgrade()?;
        let destination = {
            let state = tracker.shared.state.read().await;
            let running = state
                .ceremonies
                .get(&self.state.ceremony_id)
                .and_then(|entry| entry.running.as_ref())
                .ok_or_else(|| {
                    original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
                })?;
            if !std::ptr::eq(running.original.as_ref(), self) {
                return Err(original_tracker_refusal(
                    OriginalEnrollmentTrackerError::OwnerLost,
                ));
            }
            Arc::clone(&running.retirement_destination)
        };
        if let Some(source) = destination.retained_finalizer_failure() {
            return Err(source);
        }
        tokio::select! {
            biased;
            source = destination.await_finalizer_failure() => Err(source),
            outcome = tracker.await_enrollment_terminal_outcome(&self.state.ceremony_id) => {
                if let Some(source) = destination.retained_finalizer_failure() {
                    Err(source)
                } else {
                    outcome
                }
            }
        }
    }
    pub(super) fn acknowledge_original_retirement(
        self: &Arc<Self>,
        writes: crate::runtime::effects::AcknowledgedEnrollmentRetirement,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<OriginalEnrollmentRetirementAcknowledgment, RejectedOriginalEnrollmentRetirement>
    {
        let checked = self.require_issued_notice_control(issued).and_then(|()| {
            writes.require_original(self.tracker.effects.as_ref(), issued)?;
            if writes.prestate() != self.state.prestate_hash {
                return Err(crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                ));
            }
            Ok(())
        });
        match checked {
            Ok(()) => Ok(OriginalEnrollmentRetirementAcknowledgment {
                original: Arc::clone(self),
                writes,
            }),
            Err(source) => Err(RejectedOriginalEnrollmentRetirement {
                source,
                _writes: Box::new(writes),
            }),
        }
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) async fn observe_original_prepared_interruption(
        &self,
        receiver: tokio::sync::oneshot::Receiver<
            crate::handlers::device_epoch_rotation::OriginalPreparedActivationInterruption,
        >,
    ) -> crate::core::AgentResult<
        crate::handlers::device_epoch_rotation::OriginalPreparedActivationInterruption,
    > {
        let effects = self.tracker.effects.as_ref();
        aura_core::execute_with_timeout_budget(effects, &self.state.timeout_budget, || async {
            let receipt = receiver.await.map_err(|source| AuraError::Internal {
                message: "original prepared interruption observation lost".into(),
                source: Some(Arc::new(source)),
            })?;
            receipt.require_original(effects, &self.state.ceremony_id)?;
            Ok::<_, AuraError>(receipt)
        })
        .await
        .map_err(|source| {
            crate::core::AgentError::from(AuraError::Internal {
                message: "observe original prepared interruption".into(),
                source: Some(Arc::new(source)),
            })
        })
    }
    #[cfg(test)]
    pub(crate) async fn observe_original_finalizer(
        self: &Arc<Self>,
    ) -> crate::core::AgentResult<()> {
        let tracker = self.tracker.upgrade()?;
        let running = {
            let state = tracker
                .shared
                .state
                .try_read()
                .map_err(|source| AuraError::Internal {
                    message: "observe retained original issuer entry".into(),
                    source: Some(Arc::new(source)),
                })?;
            let running = state
                .ceremonies
                .get(&self.state.ceremony_id)
                .and_then(|entry| entry.running.as_ref())
                .ok_or_else(|| {
                    original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
                })?;
            if !Arc::ptr_eq(&running.original, self) {
                return Err(
                    original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost).into(),
                );
            }
            Arc::clone(running)
        };
        running
            .observer
            .observe_original_terminal(
                self.tracker.effects.as_ref(),
                &super::ceremony_runner::CeremonyRunner::new(tracker),
            )
            .await
    }
    pub(crate) fn observe_original_delivery(
        self: &Arc<Self>,
    ) -> Result<OriginalIssuerDeliveryObservation, AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original issuer disposal".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let running = entry
            .running
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        let task = entry
            .issuer_task
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        Ok(OriginalIssuerDeliveryObservation {
            original: Arc::clone(self),
            disposal: task.disposal_observer()?,
        })
    }
    #[cfg(test)]
    pub(crate) fn request_original_issuer_cancel_for_test(
        self: &Arc<Self>,
    ) -> Result<(), AuraError> {
        let tracker = self.tracker.upgrade()?;
        let mut state = tracker
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "borrow same original issuer cancellation".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get_mut(&self.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !entry
            .running
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(&running.original, self))
        {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        entry
            .issuer_task
            .as_mut()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?
            .request_cancel()?;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn require_original_issuer_disposal_for_test(
        self: &Arc<Self>,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "borrow same original issuer disposal".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !entry
            .running
            .as_ref()
            .is_some_and(|running| Arc::ptr_eq(&running.original, self))
        {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        entry
            .issuer_task
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?
            .require_disposal(disposal)
    }
    pub(super) fn prepare_running_observation(
        self: &Arc<Self>,
        observer: super::enrollment_window::HeldIssuerCompletionObserver,
        original_group: crate::task_registry::TaskGroup,
        generation: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
        retirement_destination: Arc<
            crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
        >,
    ) -> Result<PreparedRunningIssuerObservation, AuraError> {
        let tracker = self.tracker.upgrade()?;
        generation.require_tracker(&tracker)?;
        generation.require_effects(self.tracker.effects.as_ref())?;
        self.require_effects(self.tracker.effects.as_ref())?;
        self._original_allocation
            .require_registered_generation(self.tracker.effects.as_ref(), generation)?;
        Ok(PreparedRunningIssuerObservation {
            original: Arc::clone(self),
            tracker: OriginalEnrollmentTrackerReference::from_original(&tracker)?,
            ceremony: self.state.ceremony_id.clone(),
            observer: Some(observer),
            original_group,
            retirement_destination,
        })
    }

    /// Move the same acknowledged root into its original entry only after all
    /// checks succeed. A failed lock or binding check leaves caller custody intact.
    pub(super) fn retain_issuer_acknowledgment(
        self: &Arc<Self>,
        pending: &mut Option<super::enrollment_window::AcknowledgedIssuerEnrollmentWindow>,
    ) -> Result<(), AuraError> {
        let acknowledged = pending
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        acknowledged.require_original(self)?;
        let tracker = self.tracker.upgrade()?;
        let mut state = tracker
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "retain original acknowledged issuer root".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get_mut(&self.state.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let running = entry
            .running
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&running.original, self) || entry.acknowledged_issuer.is_some() {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::AlreadyBorn,
            ));
        }
        // No fallible work or await follows removal from the retained custodian.
        entry.acknowledged_issuer = pending.take();
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn require_original_rejected_resources_for_test(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        self.original_resource_decision(issued)
            .and_then(|decision| match decision {
                super::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected => {
                    Ok(())
                }
                super::enrollment_window::OriginalIssuerResourceDecision::Committed => {
                    Err(AuraError::permission_denied(
                        "positive decision cannot admit rejected resource cleanup",
                    ))
                }
                super::enrollment_window::OriginalIssuerResourceDecision::Interrupted => Err(
                    AuraError::permission_denied("interruption is not verified rejection"),
                ),
            })
    }

    pub(super) fn require_original_verified_rejection(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        self.require_issued_notice_control(issued)?;
        let proof = self.verified_rejection.get().ok_or_else(|| {
            AuraError::permission_denied("original verified rejection proof missing")
        })?;
        let manifest = issued.manifest();
        if proof.ceremony_id() != &manifest.ceremony
            || proof.subject() != manifest.subject
            || proof.device_id() != manifest.invitee_device
            || proof.pending_epoch() != manifest.pending_epoch
        {
            return Err(AuraError::permission_denied(
                "original verified rejection scope differs",
            ));
        }
        let decision = self.original_negative_decision(issued)?;
        if decision.kind()
            != aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Rejected
        {
            return Err(AuraError::permission_denied(
                "original protected decision is not verified rejection",
            ));
        }
        decision.require_original_control(issued)
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "verified_rejected_issuer_generation", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(super) async fn retire_original_rejected_generation(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<crate::runtime::effects::AcknowledgedEnrollmentRetirement, AuraError> {
        self.require_original_verified_rejection(issued)?;
        let tracker = self.tracker.upgrade()?;
        tracker
            .retire_failed_enrollment_generation(&self.state.ceremony_id)
            .await
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_issuer_resource_closure", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(super) async fn close_original_issuer_resources(
        self: &Arc<Self>,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        retired: &super::enrollment_window::OriginalIssuerSigningResourceSettlement,
        decision: super::enrollment_window::OriginalIssuerResourceDecision,
        generation: Option<&OriginalEnrollmentRetirementAcknowledgment>,
        disposal: &crate::task_registry::TaskGroupDisposalAcknowledgment,
        children: &crate::task_registry::TaskGroup,
    ) -> Result<OriginalIssuerResourceClosure, AuraError> {
        decision.require_original(issued, child)?;
        match decision {
            super::enrollment_window::OriginalIssuerResourceDecision::Committed => {
                if generation.is_some() {
                    return Err(AuraError::permission_denied(
                        "committed resources cannot retire pending generation",
                    ));
                }
            }
            super::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected => {
                self.require_original_verified_rejection(issued)?;
                generation
                    .ok_or_else(|| {
                        AuraError::permission_denied(
                            "verified rejection generation retirement missing",
                        )
                    })?
                    .require_original(self, issued)?;
            }
            super::enrollment_window::OriginalIssuerResourceDecision::Interrupted => {
                self.require_original_interrupted_resources(issued)?;
                if generation.is_some() {
                    return Err(AuraError::permission_denied(
                        "unsigned interruption cannot retire the original generation",
                    ));
                }
            }
        }
        retired.require_original(issued, child)?;
        disposal.require_original_group(children)?;
        // The permit check prevents closing while any original execution is borrowed.
        self.lease.close_resource_admission()?;
        self.write_original_closed_checkpoint(true).await?;
        Ok(OriginalIssuerResourceClosure {
            original: Arc::downgrade(self),
        })
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerResourceClosure", receiver_type = RegisteredEnrollmentWindowCapability, family = "runtime_helper")]
    pub(super) fn release_issuer_resource_execution(
        self: &Arc<Self>,
        receipt: &OriginalIssuerResourceClosure,
    ) -> Result<(), AuraError> {
        if !receipt
            .original
            .upgrade()
            .is_some_and(|original| Arc::ptr_eq(&original, self))
        {
            return Err(AuraError::permission_denied(
                "committed resource closure belongs to another original",
            ));
        }
        self.lease.release_after_acknowledgment()
    }
    pub(super) async fn acknowledge_terminal_checkpoint(&self) -> Result<(), AuraError> {
        self.write_original_closed_checkpoint(false).await
    }
    async fn write_original_closed_checkpoint(
        &self,
        resource_reentry: bool,
    ) -> Result<(), AuraError> {
        use aura_core::effects::{SecureStorageCapability, SecureStorageLocation};
        let tracker = self.tracker.upgrade()?;
        self.require_effects(self.tracker.effects.as_ref())?;
        let _write = tracker.shared.persistence_guard.lock().await;
        if !resource_reentry {
            self.require_pending().await?;
        }
        let mut record = tracker.read_bound_enrollment_clock(&self.state).await?;
        if resource_reentry
            && !matches!(
                record.execution,
                super::enrollment_window::DurableEnrollmentExecutionState::Pending
                    | super::enrollment_window::DurableEnrollmentExecutionState::Closed
            )
        {
            return Err(AuraError::permission_denied(
                "committed resource clock has incompatible execution state",
            ));
        }
        record.budget = self.state.timeout_budget.clone();
        record.execution = super::enrollment_window::DurableEnrollmentExecutionState::Closed;
        let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Internal {
            message: "encode original issuer terminal checkpoint".into(),
            source: Some(Arc::new(source)),
        })?;
        let location =
            SecureStorageLocation::new("enrollment_clock_v1", self.state.ceremony_id.to_string());
        self.tracker
            .effects
            .secure_store(&location, &bytes, &[SecureStorageCapability::Write])
            .await?;
        let retained = self
            .tracker
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if retained != bytes {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::BirthReadbackChanged,
            ));
        }
        Ok(())
    }

    pub(super) async fn require_pending(&self) -> Result<(), AuraError> {
        self.tracker.require_pending(&self.state).await
    }
    pub(super) fn completion_observation(&self) -> HeldIssuerClockObservationCapability {
        HeldIssuerClockObservationCapability {
            tracker: self.tracker.clone(),
            state: self.state.clone(),
        }
    }
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.tracker.require_effects(effects)
    }
}

impl OriginalEnrollmentTrackerReference {
    async fn require_pending(&self, original: &TrackedCeremony) -> Result<(), AuraError> {
        let tracker = self.upgrade()?;
        self.require_effects(self.effects.as_ref())?;
        let registry = tracker.shared.state.read().await;
        let retained = registry
            .ceremonies
            .get(&original.ceremony_id)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !retained
            .timeout_budget
            .shares_observation_owner_with(&original.timeout_budget)
            || !Arc::ptr_eq(
                &retained.enrollment_window_lease,
                &original.enrollment_window_lease,
            )
        {
            return Err(AuraError::from(
                aura_core::TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "original issuer allocation owner changed".into(),
                },
            ));
        }
        let record = tracker.read_bound_enrollment_clock(original).await?;
        if record.execution != super::enrollment_window::DurableEnrollmentExecutionState::Pending {
            return Err(AuraError::permission_denied(
                "original issuer execution is closed",
            ));
        }
        original
            .timeout_budget
            .validate_checkpoint_continuation_from(&record.budget)
            .map_err(AuraError::from)
    }
}

impl RegisteredEnrollmentWindowCapability {
    // This producer stays in the tracker module; callers cannot construct raw
    // state/lease authority or bypass the original allocation verification.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentWindowCapability",
        receiver_type = RegisteredEnrollmentWindowCapability,
        family = "runtime_helper"
    )]
    pub(super) fn require_approved_manifest(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        use aura_signature::SecurityTranscript;
        self.require_effects(approval.effects().as_ref())?;
        let manifest = approval.manifest();
        self._original_allocation
            .require_manifest(approval.effects().as_ref(), manifest)?;
        if self.state.kind != CeremonyKind::DeviceEnrollment
            || self.state.ceremony_id != manifest.ceremony
            || self.state.initiator_id != manifest.subject
            || self.state.enrollment_device_id != Some(manifest.invitee_device)
            || self.state.new_epoch != manifest.pending_epoch
        {
            return Err(AuraError::permission_denied(
                "approved manifest differs from original registered allocation",
            ));
        }
        approval
            .negative_terminal_policy()
            .require_manifest(manifest)?;
        if let Some(bound) = self.notice_binding.get() {
            let transcript = manifest.required_transcript_bytes().map_err(|source| {
                AuraError::crypto_with_source(
                    "encode original approved allocation manifest",
                    Arc::new(source),
                )
            })?;
            if bound.transcript != transcript
                || bound.manifest_digest != aura_core::hash::hash(&transcript)
                || bound.expires_at_ms != manifest.expires_at_ms
            {
                return Err(AuraError::permission_denied(
                    "approved manifest differs from original issued notice binding",
                ));
            }
        }
        Ok(())
    }

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
        self.tracker.upgrade()?;
        let effects = &self.tracker.effects;
        issued.require_runtime_owner(effects.as_ref())?;
        self._original_allocation
            .require_issued_control(effects, issued)?;
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
            canonical_invitation: original_invitation_binding(issued.canonical_invitation())?,
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
            || original.canonical_invitation != candidate.canonical_invitation
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
    pub(super) fn require_activation_generation(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        activation: &EnrollmentActivationCapability<'_>,
    ) -> Result<aura_core::Hash32, AuraError> {
        self.require_effects(effects)?;
        self.require_issued_notice_control(issued)?;
        activation.require_effects(effects)?;
        activation.require_tracker(&self.tracker.upgrade()?)?;
        let manifest = issued.manifest();
        if activation.ceremony_id() != &self.state.ceremony_id
            || manifest.ceremony != self.state.ceremony_id
            || manifest.subject != self.state.initiator_id
            || manifest.pending_epoch != self.state.new_epoch
        {
            return Err(AuraError::permission_denied(
                "activation differs from original registered generation",
            ));
        }
        Ok(self.state.prestate_hash)
    }

    pub(super) fn budget(&self) -> &aura_core::TimeoutBudget {
        &self.state.timeout_budget
    }
    pub(super) fn lease(&self) -> Arc<OriginalRegisteredExecutionLease> {
        self.lease.clone()
    }
}

impl CeremonyTracker {
    /// Wait only for the original issuer's verified acceptance or terminal
    /// decision. The caller's sealed child bounds this observation.
    pub(crate) async fn await_original_enrollment_acceptance(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        original: &super::enrollment_window::EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
        })?;
        issued.require_runtime_owner(effects.as_ref())?;
        original.require_issued_notice_owner(issued)?;
        loop {
            let changed = self.shared.terminal_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let state = self.get(&issued.manifest().ceremony).await?;
            if state.kind != CeremonyKind::DeviceEnrollment
                || state.initiator_id != issued.manifest().subject
                || state.new_epoch != issued.manifest().pending_epoch
                || state.enrollment_device_id != Some(issued.manifest().invitee_device)
            {
                return Err(AuraError::permission_denied(
                    "original acceptance binding differs",
                ));
            }
            if state.has_failed {
                let reason = state.failure_reason.ok_or_else(|| {
                    AuraError::invalid("negative original enrollment lacks its terminal cause")
                })?;
                return Err(AuraError::PermissionDenied {
                    message: "original enrollment terminal decision is negative".into(),
                    source: Some(Arc::new(
                        crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalFailed(
                            reason,
                        ),
                    )),
                });
            }
            if state.threshold_k > 0
                && state.accepted_participants.len() >= usize::from(state.threshold_k)
            {
                self.require_verified_enrollment_response(&issued.manifest().ceremony)
                    .await?;
                let tracker = self.shared.state.read().await;
                let proof = tracker
                    .enrollment_responses
                    .get(&issued.manifest().ceremony)
                    .ok_or_else(|| AuraError::invalid("original acceptance proof disappeared"))?;
                if proof.invitation_id() != issued.manifest().invitation
                    || proof.acceptance().manifest_digest != issued.digest()
                {
                    return Err(AuraError::permission_denied(
                        "original acceptance transcript differs",
                    ));
                }
                return Ok(());
            }
            changed.await;
        }
    }
    pub(crate) fn original_issuer_retirement_observation(
        &self,
        retained: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<Option<Arc<RunningIssuerObservation>>, AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
        })?;
        retained.require_runtime_owner(effects.as_ref())?;
        let state = self
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original issuer retirement entry".into(),
                source: Some(Arc::new(source)),
            })?;
        let Some(running) = state
            .ceremonies
            .get(&retained.manifest().ceremony)
            .and_then(|entry| entry.running.as_ref())
        else {
            return Ok(None);
        };
        running.original.require_issued_notice_control(retained)?;
        Ok(Some(Arc::clone(running)))
    }
    /// This is the private durable-publication closure. Its caller must retain
    /// the actual result through the shared original-deadline birth boundary.
    /// Merely observing these bytes later cannot call this constructor again.
    #[cfg_attr(
        test,
        allow(
            clippy::disallowed_types,
            reason = "Constructs the synchronous test observation gate; its guard never crosses an await"
        )
    )]
    async fn publish_pending_issuer_birth(
        &self,
        generation: &mut crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        state: &TrackedCeremony,
    ) -> Result<AcknowledgedIssuerEnrollmentBirth, AuraError> {
        use aura_core::effects::secure::ImmutableSecureStoreOutcome;
        use aura_core::effects::{
            SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
        };

        generation.require_tracker(self)?;
        generation.require_original_birth()?;
        let tracker = OriginalEnrollmentTrackerReference::from_original(self)?;
        generation.validate_allocated_registration(tracker.effects.as_ref(), state)?;
        let record = StoredEnrollmentClock {
            version: 4,
            ceremony: state.ceremony_id.clone(),
            subject: state.initiator_id,
            device: state.enrollment_device_id.ok_or_else(|| {
                crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::Binding,
                )
            })?,
            prestate: state.prestate_hash,
            epoch: state.new_epoch,
            budget: state.timeout_budget.clone(),
            negative_cleanup_budget: state.negative_cleanup_budget.clone(),
            original_policy: state.original_window_policy.clone(),
            execution: super::enrollment_window::DurableEnrollmentExecutionState::Pending,
        };
        let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Serialization {
            message: "encode original issuer Pending birth".into(),
            source: Some(Arc::new(source)),
        })?;
        let location =
            SecureStorageLocation::new("enrollment_clock_v1", state.ceremony_id.to_string());
        #[cfg(test)]
        if let Some(source) = self.shared.clock_checkpoint_fault.lock().await.take() {
            return Err(source);
        }
        if tracker
            .effects
            .secure_create_mutable(
                &location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?
            != ImmutableSecureStoreOutcome::Created
        {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::AlreadyBorn,
            ));
        }
        let readback = tracker
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if readback != bytes {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::BirthReadbackChanged,
            ));
        }
        let lease = state
            .enrollment_window_lease
            .clone()
            .try_acquire_owned()
            .map_err(registered_window_lease_error)?;
        let original_allocation = generation.take_original_birth()?;
        Ok(AcknowledgedIssuerEnrollmentBirth {
            original: RegisteredEnrollmentWindowCapability {
                tracker,
                state: state.clone(),
                lease: Arc::new(OriginalRegisteredExecutionLease::new(lease)),
                notice_binding: std::sync::OnceLock::new(),
                negative_decision: std::sync::OnceLock::new(),
                verified_rejection: std::sync::OnceLock::new(),
                positive_decision: std::sync::OnceLock::new(),
                interrupted_resources: std::sync::OnceLock::new(),
                #[cfg(test)]
                committed_confirmation_gate: parking_lot::Mutex::default(),
                _original_allocation: original_allocation,
            },
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        family = "runtime_helper"
    )]
    pub(crate) async fn held_issuer_window(
        &self,
        effects: &Arc<crate::runtime::AuraEffectSystem>,
        reservation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
    ) -> Result<
        super::enrollment_window::EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        AuraError,
    > {
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
        let mut entries = self
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "original issuer execution entry is contended".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = entries
            .ceremonies
            .get_mut(reservation.ceremony_id())
            .ok_or_else(|| AuraError::invalid("original issuer execution entry is missing"))?;
        let state = &entry.observed;
        reservation.validate_allocated_registration(effects.as_ref(), state)?;
        if state.terminal_outcome.is_some()
            || state.has_failed
            || state.is_committed
            || state.is_superseded
        {
            return Err(AuraError::permission_denied(
                "held issuer allocation is terminal",
            ));
        }
        let original = entry
            .execution
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::AlreadyBorn))?;
        original.origin().require_effects(effects.as_ref())?;
        reservation.validate_allocated_registration(effects.as_ref(), &original.origin().state)?;
        entry
            .execution
            .take()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::AlreadyBorn))
    }
}

/// Original allocation observation custody for cancellation preparation only.
/// This does not acquire an execution permit or authorize VM/session admission.
pub(super) struct CancellationClockObservationCapability {
    negative: Option<Arc<VerifiedEnrollmentCancellationCapability>>,
    tracker: OriginalEnrollmentTrackerReference,
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
    observation: Arc<CancellationClockObservationCapability>,
    lease: Arc<tokio::sync::OwnedSemaphorePermit>,
    cancelled: Arc<VerifiedEnrollmentCancellationCapability>,
    manifest_digest: [u8; 32],
}
impl RegisteredCancelledNoticeCapability {
    pub(super) fn original_parts(
        &self,
    ) -> (
        Arc<CancellationClockObservationCapability>,
        Arc<tokio::sync::OwnedSemaphorePermit>,
        Arc<VerifiedEnrollmentCancellationCapability>,
        [u8; 32],
    ) {
        (
            Arc::clone(&self.observation),
            Arc::clone(&self.lease),
            Arc::clone(&self.cancelled),
            self.manifest_digest,
        )
    }
}
pub(crate) struct OriginalCancellationNegativeCheckpointReceipt {
    original: Arc<RegisteredCancelledNoticeCapability>,
    retained_decision: super::enrollment_window::DurableEnrollmentExecutionState,
}
impl OriginalCancellationNegativeCheckpointReceipt {
    pub(crate) fn require_original(
        &self,
        original: &Arc<RegisteredCancelledNoticeCapability>,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.original, original)
            || !matches!(
                self.retained_decision,
                super::enrollment_window::DurableEnrollmentExecutionState::Closed
                    | super::enrollment_window::DurableEnrollmentExecutionState::Refused
            )
        {
            return Err(AuraError::permission_denied(
                "another cancellation negative checkpoint",
            ));
        }
        Ok(())
    }
}
impl RegisteredCancelledNoticeCapability {
    pub(crate) fn negative_cleanup_budget(&self) -> &aura_core::TimeoutBudget {
        &self.observation.state.negative_cleanup_budget
    }
    pub(crate) fn cancelled(&self) -> &Arc<VerifiedEnrollmentCancellationCapability> {
        &self.cancelled
    }
    pub(crate) async fn acknowledge_negative_checkpoint(
        self: &Arc<Self>,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<OriginalCancellationNegativeCheckpointReceipt, AuraError> {
        self.observation.require_effects(effects)?;
        let retained_decision = self
            .observation
            .tracker
            .upgrade()?
            .persist_original_negative_clock(
                &self.observation.state,
                effects,
                OriginalNegativeCheckpointOwner::Cancellation,
            )
            .await?;
        Ok(OriginalCancellationNegativeCheckpointReceipt {
            original: Arc::clone(self),
            retained_decision,
        })
    }
}
/// Actual original cancellation retirement; fields stay private to the producer.
pub(crate) struct OriginalCancellationRetirementReceipt {
    original: Arc<RegisteredCancelledNoticeCapability>,
    writes: crate::runtime::effects::AcknowledgedEnrollmentRetirement,
}
impl OriginalCancellationRetirementReceipt {
    pub(crate) fn require_original(
        &self,
        original: &Arc<RegisteredCancelledNoticeCapability>,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.original, original) {
            return Err(AuraError::permission_denied(
                "another original cancellation retirement",
            ));
        }
        original.require_original_retirement(&self.writes, issued, effects)
    }
}
impl RegisteredCancelledNoticeCapability {
    fn require_original_retirement(
        &self,
        writes: &crate::runtime::effects::AcknowledgedEnrollmentRetirement,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.observation.require_effects(effects)?;
        writes.require_original(effects, issued)?;
        let manifest = issued.manifest();
        let state = &self.observation.state;
        if self.manifest_digest != issued.digest()
            || self.cancelled.invitation() != &manifest.invitation
            || self.cancelled.ceremony() != &manifest.ceremony
            || state.kind != CeremonyKind::DeviceEnrollment
            || state.ceremony_id != manifest.ceremony
            || state.initiator_id != manifest.subject
            || state.enrollment_device_id != Some(manifest.invitee_device)
            || state.new_epoch != manifest.pending_epoch
            || writes.prestate() != state.prestate_hash
            || self.observation.signed_expiry_ms != manifest.expires_at_ms
        {
            return Err(AuraError::permission_denied(
                "original cancellation retirement binding differs",
            ));
        }
        Ok(())
    }
    pub(crate) fn seal_original_retirement(
        self: &Arc<Self>,
        writes: crate::runtime::effects::AcknowledgedEnrollmentRetirement,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<
        OriginalCancellationRetirementReceipt,
        Box<(
            crate::runtime::effects::AcknowledgedEnrollmentRetirement,
            AuraError,
        )>,
    > {
        if let Err(source) = self.require_original_retirement(&writes, issued, effects) {
            return Err(Box::new((writes, source)));
        }
        Ok(OriginalCancellationRetirementReceipt {
            original: Arc::clone(self),
            writes,
        })
    }
}
enum RegisteredClockCheckpointAuthority<'a> {
    Cancellation(&'a CancellationClockObservationCapability),
}
impl CancellationClockObservationCapability {
    pub(super) fn require_effects(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.tracker.upgrade()?;
        let owner = &self.tracker.effects;
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
            .upgrade()?
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum OriginalNegativeCheckpointOwner {
    Issuer,
    Cancellation,
}

#[cfg(test)]
type CancellationPostcheckpointGate = (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
    Option<Box<dyn FnOnce() + Send>>,
);

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
    #[cfg(test)]
    cancellation_postcheckpoint_gate: Mutex<Option<CancellationPostcheckpointGate>>,
    #[cfg(test)]
    negative_checkpoint_fault: Mutex<Option<(OriginalNegativeCheckpointOwner, AuraError)>>,
    #[cfg(test)]
    negative_checkpoint_fault_observer: Mutex<Option<tokio::sync::oneshot::Sender<AuraError>>>,
    #[cfg(test)]
    cancellation_cleanup_observer:
        Mutex<Option<tokio::sync::oneshot::Sender<Result<(), AuraError>>>>,
    enrollment_decision_gate: Arc<Mutex<()>>,
    terminal_changed: tokio::sync::Notify,
    /// Each cancelled-notice owner publishes whether it signed and released
    /// the cancelled pending generation. Retained, so cancel retries observe it.
    cancelled_generation_settlements: Mutex<HashMap<CeremonyId, Result<(), AuraError>>>,
    cancelled_generation_settled: tokio::sync::Notify,
}

#[derive(Debug, Default)]
struct CeremonyTrackerState {
    ceremonies: HashMap<CeremonyId, TrackedCeremonyEntry>,
    // Opaque capability is never restored by deserializing persisted records.
    enrollment_responses:
        HashMap<CeremonyId, crate::handlers::invitation::VerifiedEnrollmentResponse>,
    retired_enrollment_ids: HashSet<CeremonyId>,
    /// Supersession records for audit trail
    supersession_records: Vec<SupersessionRecord>,
}

/// The canonical actor entry owns execution separately from cloneable views.
/// Restoring a snapshot produces only an observed entry, never a new root.
struct TrackedCeremonyEntry {
    original_entry: Arc<()>,
    cancellation_task: Option<Arc<RetainedOriginalCancellationTask>>,
    cancelled_notice_task: Option<Arc<RetainedOriginalCancellationTask>>,
    issuer_task: Option<Arc<RetainedOriginalIssuerTask>>,
    acknowledged_issuer: Option<super::enrollment_window::AcknowledgedIssuerEnrollmentWindow>,
    observed: TrackedCeremony,
    execution: Option<
        super::enrollment_window::EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
    >,
    running: Option<Arc<RunningIssuerObservation>>,
}

/// The actual original window and accepted registry spawn, retained by the
/// existing canonical entry. It cannot mint execution or terminal authority.
/// The canonical entry and its observations borrow this same retained control;
/// cancellation of reconciliation leaves the handle in its original slot.
struct RetainedOriginalIssuerTask {
    control: tokio::sync::Mutex<crate::task_registry::OriginalRequiredTaskHandle>,
}
impl RetainedOriginalIssuerTask {
    fn require_disposal(
        &self,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let control = self
            .control
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original issuer control is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        disposal.require_original(&control)
    }
    fn disposal_observer(
        &self,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        let control = self
            .control
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original issuer control is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        Ok(control.disposal_observer())
    }
    #[cfg(test)]
    fn request_cancel(&self) -> Result<(), AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        let mut control = self
            .control
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original issuer control is borrowed".into(),
                source: Some(Arc::new(source)),
            })?;
        control.request_cancel();
        Ok(())
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedOriginalIssuerTask", receiver_type = RetainedOriginalIssuerTask, family = "runtime_helper")]
    async fn reconcile_original_disposal(
        &self,
    ) -> Result<OriginalIssuerCancellationSettlement, AuraError> {
        use aura_core::effects::task::RequiredTaskControl;
        use futures::FutureExt;
        let mut control = self.control.lock().await;
        control.await_original_attempt_disposition().await?;
        let observer = control.disposal_observer();
        let mut reconciliation_failure = None;
        if observer.await_disposal().now_or_never().is_none() {
            // This physical retry is a new explicit request, so it may advance
            // a previously completed failed target abandoned by an observer.
            control.request_resident_cleanup_retry()?;
            #[cfg(not(target_arch = "wasm32"))]
            let actual = control.reconcile().await;
            #[cfg(target_arch = "wasm32")]
            let actual = control.reconcile_local().await;
            reconciliation_failure = actual.err();
        }
        let Some(disposal) = observer.await_disposal().now_or_never() else {
            return Err(reconciliation_failure.unwrap_or_else(|| {
                AuraError::invalid("original issuer cleanup returned without actual disposal")
            }));
        };
        disposal.require_original(&control)?;
        Ok(OriginalIssuerCancellationSettlement {
            disposal,
            independent: reconciliation_failure.into_iter().collect(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("original issuer cleanup: {primary}; independent observation: {secondary}")]
struct OriginalIssuerCleanupObservationFailure {
    #[source]
    primary: AuraError,
    secondary: AuraError,
}
fn original_issuer_cleanup_observation_failure(
    primary: AuraError,
    secondary: AuraError,
) -> AuraError {
    AuraError::Internal {
        message: "original issuer cleanup and observation failures retained".into(),
        source: Some(Arc::new(OriginalIssuerCleanupObservationFailure {
            primary,
            secondary,
        })),
    }
}

/// Actual same-control disposal, separate from the original failed domain result.
/// No receipt is synthesized from a successful reconciliation Result.
pub(crate) struct OriginalIssuerCancellationSettlement {
    disposal: crate::task_registry::OriginalRequiredTaskDisposalObservation,
    independent: Vec<AuraError>,
}
#[derive(thiserror::Error)]
#[error("original issuer disposed but retirement observation refused: {source}")]
struct RejectedOriginalIssuerCancellationSettlement {
    #[source]
    source: AuraError,
    _retirement_refusal: AuraError,
    _retained: OriginalIssuerCancellationSettlement,
}
impl std::fmt::Debug for RejectedOriginalIssuerCancellationSettlement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RejectedOriginalIssuerCancellationSettlement")
    }
}
impl OriginalIssuerCancellationSettlement {
    pub(crate) fn cleanup_failure(&self) -> Option<AuraError> {
        use aura_core::effects::task::RequiredTaskExit;
        match self.disposal.outcome() {
            RequiredTaskExit::Failed(source) => {
                use std::error::Error as _;
                // The issuer's ordinary signed Cancelled terminal is a failed
                // issuer domain, while the requested cancellation may succeed.
                // Exclude ONLY this exact completed-execution terminal shape;
                // native execution, retirement, VM and independent causes keep
                // their concrete diagnostic and are not classified by text.
                let expected_cancel = source.source()
                    .and_then(|actual| actual.downcast_ref::<aura_core::effects::task::RequiredTaskTerminalFailure>())
                    .is_some_and(|actual| {
                        matches!(actual.execution(), RequiredTaskExit::Completed)
                            && matches!(actual.terminal().source()
                                .and_then(|actual| actual.downcast_ref::<crate::handlers::invitation::EnrollmentVmAdmissionError>()),
                                Some(crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalFailed(
                                    aura_app::runtime_bridge::CeremonyFailureReason::Cancelled
                                )))
                    });
                (!expected_cancel).then_some(source)
            }
            RequiredTaskExit::Completed | RequiredTaskExit::Cancelled => None,
            RequiredTaskExit::Panicked => {
                Some(AuraError::invalid("original issuer disposed after panic"))
            }
        }
    }
    pub(crate) fn independent_failures(&self) -> &[AuraError] {
        &self.independent
    }
}

pub(crate) struct RunningIssuerObservation {
    retirement_destination:
        Arc<crate::handlers::invitation_service::OriginalIssuerTerminalDestination>,
    original: Arc<RegisteredEnrollmentWindowCapability>,
    control: Arc<RetainedOriginalIssuerTask>,
    observer: super::enrollment_window::HeldIssuerCompletionObserver,
    // Retains the actual admitted task's registration evidence for this owner.
    _registration: crate::task_registry::RegisteredRuntimeTaskObservation,
}

pub(crate) struct PreparedRunningIssuerObservation {
    retirement_destination:
        Arc<crate::handlers::invitation_service::OriginalIssuerTerminalDestination>,
    original: Arc<RegisteredEnrollmentWindowCapability>,
    tracker: OriginalEnrollmentTrackerReference,
    ceremony: CeremonyId,
    observer: Option<super::enrollment_window::HeldIssuerCompletionObserver>,
    original_group: crate::task_registry::TaskGroup,
}

impl RunningIssuerObservation {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = RunningIssuerObservation, family = "runtime_helper")]
    pub(crate) async fn observe_original_cancellation_settlement(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<OriginalIssuerCancellationSettlement, AuraError> {
        self.original.require_effects(effects)?;
        self.original.require_issued_notice_control(issued)?;
        if self.original.original_committed_decision(effects, issued)? {
            let (observation, disposal) = self.original.await_original_resource_disposal().await?;
            observation.require_positive(&disposal)?;
            return Ok(OriginalIssuerCancellationSettlement {
                disposal,
                independent: Vec::new(),
            });
        }
        // Reconcile the actual original executor before observing its failed
        // domain result. The resident supervisor owns any pending cleanup future.
        let mut original_cleanup = Box::pin(self.control.reconcile_original_disposal());
        let borrowed_cleanup = original_cleanup.as_mut();
        let actual = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects,
            self.original.negative_cleanup_budget(),
            issued,
            || borrowed_cleanup,
        )
        .await;
        let validation = actual.validation.err().map(AuraError::from);
        let mut settlement = match actual.operation {
            Some(Ok(actual)) => actual,
            Some(Err(source)) => {
                return Err(match validation {
                    Some(later) => original_issuer_cleanup_observation_failure(source, later),
                    None => source,
                })
            }
            None => {
                // Deadline failure is an operation outcome, never permission to
                // discard the actual original cleanup future. The same original
                // cancellation supervisor retains this mandatory continuation.
                match original_cleanup.await {
                    Ok(actual) => actual,
                    Err(source) => {
                        return Err(match validation {
                            Some(earlier) => {
                                original_issuer_cleanup_observation_failure(earlier, source)
                            }
                            None => source,
                        })
                    }
                }
            }
        };
        // The storage proof must be the original producer-issued receipt even
        // when its earlier domain failure is intentionally sticky.
        if let Some(source) = validation {
            settlement.independent.push(source);
        }
        if let Err(retirement_refusal) = self
            .retirement_destination
            .require_original_retirement_cleanup_acknowledgment(&self.original, issued)
        {
            let source = settlement
                .cleanup_failure()
                .or_else(|| settlement.independent.first().cloned())
                .unwrap_or_else(|| retirement_refusal.clone());
            return Err(AuraError::Internal {
                message: "actual original issuer disposition and retirement refusal retained"
                    .into(),
                source: Some(Arc::new(RejectedOriginalIssuerCancellationSettlement {
                    source,
                    _retirement_refusal: retirement_refusal,
                    _retained: settlement,
                })),
            });
        }
        Ok(settlement)
    }
    pub(crate) async fn observe_original_retirement(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> crate::core::AgentResult<()> {
        self.original.require_effects(effects)?;
        self.original.require_issued_notice_control(issued)?;
        self.observer
            .observe_original_retirement(
                effects,
                &self.original,
                issued,
                &self.retirement_destination,
            )
            .await
    }
}

impl PreparedRunningIssuerObservation {
    pub(crate) fn publish_required(
        &mut self,
        pending: &mut Option<crate::task_registry::OriginalRequiredTaskHandle>,
    ) -> Result<(), AuraError> {
        let actual = pending
            .as_ref()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let registration = actual.original_registration_observation()?;
        registration.require_original_tree(&self.original_group)?;
        registration.require_running()?;
        let tracker = self.tracker.upgrade()?;
        let mut state = tracker
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "publish original retained issuer task".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get_mut(&self.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if entry.execution.is_some() || entry.running.is_some() || entry.issuer_task.is_some() {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::AlreadyBorn,
            ));
        }
        let observer = self
            .observer
            .take()
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let control = Arc::new(RetainedOriginalIssuerTask {
            control: tokio::sync::Mutex::new(pending.take().ok_or_else(|| {
                original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
            })?),
        });
        entry.running = Some(Arc::new(RunningIssuerObservation {
            retirement_destination: Arc::clone(&self.retirement_destination),
            original: Arc::clone(&self.original),
            observer,
            control: Arc::clone(&control),
            _registration: registration,
        }));
        entry.issuer_task = Some(control);
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn original_owner_observation_for_test(
        &self,
    ) -> std::sync::Weak<RegisteredEnrollmentWindowCapability> {
        Arc::downgrade(&self.original)
    }
}

impl std::fmt::Debug for TrackedCeremonyEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackedCeremonyEntry")
            .field("observed", &self.observed)
            .field("has_original_execution", &self.execution.is_some())
            .finish()
    }
}

impl From<TrackedCeremony> for TrackedCeremonyEntry {
    fn from(observed: TrackedCeremony) -> Self {
        Self {
            observed,
            execution: None,
            running: None,
            acknowledged_issuer: None,
            original_entry: Arc::new(()),
            cancellation_task: None,
            cancelled_notice_task: None,
            issuer_task: None,
        }
    }
}

impl std::ops::Deref for TrackedCeremonyEntry {
    type Target = TrackedCeremony;

    fn deref(&self) -> &Self::Target {
        &self.observed
    }
}

impl std::ops::DerefMut for TrackedCeremonyEntry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.observed
    }
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
    pub(crate) shortcut_budget: Option<aura_core::TimeoutBudget>,
    pub(crate) negative_cleanup_budget: aura_core::TimeoutBudget,
    pub(crate) original_window_policy: super::enrollment_window::OriginalEnrollmentWindowPolicy,
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
enum EnrollmentDecisionTrackerOrigin<'a> {
    Borrowed(&'a CeremonyTracker),
    Owned(CeremonyTracker),
}
impl std::ops::Deref for EnrollmentDecisionTrackerOrigin<'_> {
    type Target = CeremonyTracker;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(tracker) => tracker,
            Self::Owned(tracker) => tracker,
        }
    }
}
enum EnrollmentDecisionGuard<'a> {
    Borrowed {
        _original: tokio::sync::MutexGuard<'a, ()>,
    },
    Owned {
        _original: tokio::sync::OwnedMutexGuard<()>,
    },
}
pub(crate) struct EnrollmentGenerationDecisionCapability<'a> {
    tracker: EnrollmentDecisionTrackerOrigin<'a>,
    _decision: EnrollmentDecisionGuard<'a>,
    generation: crate::runtime::effects::EnrollmentGenerationCustodyCapability<'a>,
}
impl EnrollmentGenerationDecisionCapability<'_> {
    pub(crate) fn tracker(&self) -> CeremonyTracker {
        match &self.tracker {
            EnrollmentDecisionTrackerOrigin::Borrowed(tracker) => (*tracker).clone(),
            EnrollmentDecisionTrackerOrigin::Owned(tracker) => tracker.clone(),
        }
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
    pub(crate) fn decision(&self) -> &EnrollmentGenerationDecisionCapability<'_> {
        &self.custody
    }

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

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "EnrollmentActivationCapability", receiver_type = EnrollmentActivationCapability, family = "runtime_helper")]
    pub(crate) async fn commit_original(
        self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &super::enrollment_window::EnrollmentExecutionChild,
        prepared: &crate::handlers::device_epoch_rotation::PreparedEnrollmentActivationCapability<
            '_,
        >,
    ) -> Result<(), AuraError> {
        let effects = self.tracker.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("original activation requires persistent runtime custody")
        })?;
        self.require_effects(effects.as_ref())?;
        child.require_issued_notice_owner(issued)?;
        let manifest = issued.manifest();
        if self.ceremony_id != manifest.ceremony {
            return Err(AuraError::permission_denied(
                "original activation ceremony differs",
            ));
        }
        child
            .execute(effects.as_ref(), || async {
                #[cfg(test)]
                eprintln!("original commit: executor entered; validating generation");
                self.require_generation(manifest.subject, manifest.pending_epoch)
                    .await?;
                #[cfg(test)]
                eprintln!("original commit: generation validated; reading held activation");
                let committed_ops = prepared
                    .original_committed_ops(effects.as_ref(), issued)
                    .await
                    .map_err(|source| {
                        AuraError::crypto_with_source(
                            "read original committed activation",
                            Arc::new(source),
                        )
                    })?;
                #[cfg(test)]
                eprintln!("original commit: held activation read; persisting first decision");
                let current = self.tracker.get(&self.ceremony_id).await?;
                // Publish the protected first decision and receipt before visible terminal
                // notification. The held activation gate excludes competing decisions.
                self.tracker
                    .persist_first_enrollment_terminal_decision(
                        &current,
                        CeremonyTerminalOutcome::Committed,
                    )
                    .await?;
                #[cfg(test)]
                eprintln!(
                    "original commit: first decision persisted; reading original receipt binding"
                );
                let (original, decision_digest) = self
                    .tracker
                    .original_protected_terminal_decision(
                        &current,
                        CeremonyTerminalOutcome::Committed,
                    )
                    .await?
                    .ok_or_else(|| {
                        AuraError::permission_denied("original committed execution owner absent")
                    })?;
                original.require_issued_notice_control(issued)?;
                #[cfg(test)]
                eprintln!("original commit: receipt binding verified; retaining receipt");
                let receipt = OriginalEnrollmentPositiveDecisionReceipt {
                    original: Arc::downgrade(&original),
                    decision_digest,
                    committed_ops,
                };
                if let Err(proposed) = original.positive_decision.set(receipt) {
                    let actual = original.positive_decision.get().ok_or_else(|| {
                        AuraError::invalid("original positive decision receipt disappeared")
                    })?;
                    if actual.decision_digest != proposed.decision_digest
                        || aura_core::util::serialization::to_vec(&actual.committed_ops)?
                            != aura_core::util::serialization::to_vec(&proposed.committed_ops)?
                    {
                        return Err(AuraError::permission_denied(
                            "contradictory original positive decision receipt",
                        ));
                    }
                }
                #[cfg(test)]
                eprintln!("original commit: receipt retained; publishing terminal notification");

                #[cfg(test)]
                {
                    let gate = original.committed_confirmation_gate.lock().take();
                    if let Some((entered, release)) = gate {
                        entered.send(()).map_err(|_| {
                            AuraError::invalid("actual committed test observer dropped")
                        })?;
                        release.await.map_err(|source| {
                            AuraError::crypto_with_source(
                                "actual committed confirmation gate dropped",
                                Arc::new(source),
                            )
                        })?;
                    }
                }
                self.tracker
                    .complete_under_decision_gate(
                        &self.ceremony_id,
                        CeremonyTerminalOutcome::Committed,
                    )
                    .await?;
                Ok(())
            })
            .await
            .map_err(|source| match source {
                aura_core::TimeoutRunError::Operation(source) => source,
                aura_core::TimeoutRunError::Timeout(source) => AuraError::from(source),
            })
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
    _custody: EnrollmentRetirementCustody<'a>,
    ceremony: CeremonyId,
    prestate: Hash32,
    authority: AuthorityId,
    epoch: u64,
    package_digest: [u8; 32],
    config_digest: [u8; 32],
}
enum EnrollmentRetirementCustody<'a> {
    Acquired {
        _generation: tokio::sync::MutexGuard<'a, ()>,
        _decision: tokio::sync::MutexGuard<'a, ()>,
    },
    OriginalHeld {
        _original: &'a EnrollmentGenerationDecisionCapability<'static>,
    },
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
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) async fn original_enrollment_decision_guard_for_test(
        &self,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.shared
            .enrollment_decision_gate
            .clone()
            .lock_owned()
            .await
    }

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
            tracker: EnrollmentDecisionTrackerOrigin::Borrowed(self),
            _decision: EnrollmentDecisionGuard::Borrowed {
                _original: decision,
            },
            generation,
        })
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationCustodyCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn retain_owned_enrollment_generation_decision(
        &self,
        generation: crate::runtime::effects::EnrollmentGenerationCustodyCapability<'static>,
    ) -> Result<EnrollmentGenerationDecisionCapability<'static>, AuraError> {
        let actual = self.shared.persistence.as_ref().ok_or_else(|| {
            crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            )
        })?;
        generation.require_effects(actual.as_ref())?;
        let decision = Arc::clone(&self.shared.enrollment_decision_gate)
            .lock_owned()
            .await;
        Ok(EnrollmentGenerationDecisionCapability {
            tracker: EnrollmentDecisionTrackerOrigin::Owned(self.clone()),
            _decision: EnrollmentDecisionGuard::Owned {
                _original: decision,
            },
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
                tracker
                    .ceremonies
                    .insert(ceremony.clone(), registration.into());
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

    pub(crate) async fn retire_failed_enrollment_generation(
        &self,
        ceremony: &CeremonyId,
    ) -> Result<crate::runtime::effects::AcknowledgedEnrollmentRetirement, AuraError> {
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
            _custody: EnrollmentRetirementCustody::Acquired {
                _generation: generation_guard,
                _decision: decision,
            },
            ceremony: ceremony.clone(),
            prestate: current.prestate_hash,
            authority: current.initiator_id,
            epoch: current.new_epoch,
            package_digest,
            config_digest,
        };
        effects.retire_pinned_enrollment_generation(&lease).await
    }

    /// Retire the pre-registration allocation using its already-held physical
    /// and decision leases. The caller must bound this work with the root's
    /// original negative endpoint and retain the returned write receipt before
    /// returning Ready from that callback.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        capability_type = crate::runtime::effects::EnrollmentGenerationReservation,
        family = "runtime_helper"
    )]
    pub(crate) async fn retire_original_held_enrollment_generation(
        &self,
        reservation: &crate::runtime::effects::EnrollmentGenerationReservation<'static>,
    ) -> Result<crate::runtime::effects::AcknowledgedEnrollmentRetirement, AuraError> {
        reservation.require_tracker(self)?;
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("held retirement requires original persistent storage")
        })?;
        reservation.require_effects(effects.as_ref())?;
        let ceremony = reservation.ceremony_id();
        let current = self.get(ceremony).await?;
        if current.terminal_outcome.is_some() {
            reservation.validate_held_negative_registration(effects.as_ref(), &current)?;
        } else {
            reservation.validate_allocated_registration(effects.as_ref(), &current)?;
        }
        let negative = CeremonyTerminalOutcome::Failed(CeremonyFailureReason::Cancelled);
        self.complete_under_decision_gate(ceremony, negative)
            .await?;
        let current = self.get(ceremony).await?;
        reservation.validate_held_negative_registration(effects.as_ref(), &current)?;
        let durable = self
            .retained_terminal_decision(ceremony)
            .await?
            .ok_or_else(|| {
                AuraError::invalid("held retirement requires the durable negative decision")
            })?;
        if durable.prestate != current.prestate_hash || durable.record.outcome != Some(negative) {
            return Err(AuraError::invalid(
                "held retirement original decision changed",
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
            .map_err(|source| AuraError::Internal {
                message: "load original held retirement binding".into(),
                source: Some(Arc::new(source)),
            })?;
        let lease = EnrollmentRetirementCapability {
            effects,
            _custody: EnrollmentRetirementCustody::OriginalHeld {
                _original: reservation.decision(),
            },
            ceremony: ceremony.clone(),
            prestate: current.prestate_hash,
            authority: current.initiator_id,
            epoch: current.new_epoch,
            package_digest,
            config_digest,
        };
        effects.retire_pinned_enrollment_generation(&lease).await
    }

    pub(super) fn timeout_for_kind(kind: CeremonyKind) -> Duration {
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
    ///
    /// The same effect-system owner supplies storage and physical time. Persistent
    /// callers cannot inject an independent clock; simulations install their clock
    /// in this effect system. Standalone trackers use `Self::new` instead.
    pub fn new_with_storage(effects: Arc<AuraEffectSystem>) -> Self {
        Self::new_with_optional_storage(effects.clone(), Some(effects))
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
                #[cfg(test)]
                cancellation_postcheckpoint_gate: Mutex::new(None),
                #[cfg(test)]
                negative_checkpoint_fault: Mutex::new(None),
                #[cfg(test)]
                negative_checkpoint_fault_observer: Mutex::new(None),
                #[cfg(test)]
                cancellation_cleanup_observer: Mutex::new(None),
                enrollment_decision_gate: Arc::new(Mutex::new(())),
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
        {
            let state = self.shared.state.read().await;
            if state
                .ceremonies
                .get(&current.ceremony_id)
                .and_then(|entry| entry.running.as_ref())
                .is_some_and(|running| running.original.interrupted_resources.get().is_some())
            {
                return Err(AuraError::permission_denied(
                    "disposed original issuer cannot publish a later terminal decision",
                ));
            }
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

    /// Seal the live boundary only from actual fresh Pending publication.
    async fn retain_live_enrollment_window(
        &self,
        birth: &AcknowledgedIssuerEnrollmentBirth,
    ) -> Result<(), AuraError> {
        use aura_core::effects::secure::ImmutableSecureStoreOutcome;
        use aura_core::effects::{SecureStorageCapability, SecureStorageEffects};
        let state = &birth.original.state;
        let effects = self
            .shared
            .persistence
            .as_ref()
            .ok_or_else(|| AuraError::invalid("live window requires durable storage"))?;
        birth.original.require_effects(effects.as_ref())?;
        let original = birth.original.tracker.upgrade()?;
        if !Arc::ptr_eq(&self.shared, &original.shared) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::ForeignProvider,
            ));
        }
        let marker = StoredEnrollmentLiveWindow::from_state(state)?;
        let bytes = serde_json::to_vec(&marker).map_err(|source| AuraError::Internal {
            message: "encode live enrollment allocation binding".into(),
            source: Some(Arc::new(source)),
        })?;
        let created = effects
            .secure_store_immutable(
                &live_enrollment_window_location(&state.ceremony_id),
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        if created != ImmutableSecureStoreOutcome::Created {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::AlreadyBorn,
            ));
        }
        self.require_live_enrollment_window(state).await
    }

    #[cfg(test)]
    pub(crate) async fn fail_next_cancellation_checkpoint_for_test(&self, source: AuraError) {
        *self.shared.clock_checkpoint_fault.lock().await = Some(source);
    }

    #[cfg(test)]
    pub(crate) async fn observe_cancellation_postcheckpoint_for_test(
        &self,
        after_release: Option<Box<dyn FnOnce() + Send>>,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        *self.shared.cancellation_postcheckpoint_gate.lock().await =
            Some((entered, released, after_release));
        (observed, release)
    }
    #[cfg(test)]
    pub(crate) async fn fail_issuer_negative_checkpoint_for_test(
        &self,
        source: AuraError,
    ) -> tokio::sync::oneshot::Receiver<AuraError> {
        let (observed, actual) = tokio::sync::oneshot::channel();
        *self.shared.negative_checkpoint_fault_observer.lock().await = Some(observed);
        *self.shared.negative_checkpoint_fault.lock().await =
            Some((OriginalNegativeCheckpointOwner::Issuer, source));
        actual
    }

    #[cfg(test)]
    pub(crate) async fn observe_cancellation_cleanup_for_test(
        &self,
    ) -> tokio::sync::oneshot::Receiver<Result<(), AuraError>> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        *self.shared.cancellation_cleanup_observer.lock().await = Some(sender);
        receiver
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
        let durable = self.read_bound_enrollment_clock(state).await?;
        #[cfg(test)]
        let observed_cancellation = matches!(
            &expected,
            Some(RegisteredClockCheckpointAuthority::Cancellation(_))
        );
        let negative = match &expected {
            Some(RegisteredClockCheckpointAuthority::Cancellation(capability)) => {
                capability.negative.as_ref()
            }
            None => None,
        };
        if let Some(cancelled) = negative {
            if !Arc::ptr_eq(&cancelled.runtime_owner, effects)
                || cancelled.ceremony != state.ceremony_id
                || state.terminal_outcome
                    != Some(CeremonyTerminalOutcome::Failed(
                        CeremonyFailureReason::Cancelled,
                    ))
                || state.is_superseded
            {
                return Err(AuraError::permission_denied(
                    "negative checkpoint original decision changed",
                ));
            }
        } else if durable.execution
            != super::enrollment_window::DurableEnrollmentExecutionState::Pending
        {
            return Err(AuraError::permission_denied(
                "closed enrollment cannot publish a Pending checkpoint",
            ));
        }
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&durable.budget)
            .map_err(AuraError::from)?;
        if let Some(expected) = expected {
            let original = match expected {
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
            version: 4,
            ceremony: ceremony.clone(),
            subject: state.initiator_id,
            device: state
                .enrollment_device_id
                .ok_or_else(|| AuraError::invalid("enrollment clock has no device binding"))?,
            prestate: state.prestate_hash,
            epoch: state.new_epoch,
            budget: state.timeout_budget.clone(),
            negative_cleanup_budget: state.negative_cleanup_budget.clone(),
            original_policy: state.original_window_policy.clone(),
            execution: durable.execution,
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
            .await?;
        #[cfg(test)]
        if observed_cancellation
            && state.terminal_outcome
                == Some(CeremonyTerminalOutcome::Failed(
                    CeremonyFailureReason::Cancelled,
                ))
        {
            if let Some((entered, release, after_release)) = self
                .shared
                .cancellation_postcheckpoint_gate
                .lock()
                .await
                .take()
            {
                entered
                    .send(())
                    .map_err(|_| AuraError::invalid("cancellation checkpoint observer lost"))?;
                release.await.map_err(|source| AuraError::Internal {
                    message: "original cancellation checkpoint gate release lost".into(),
                    source: Some(Arc::new(source)),
                })?;
                if let Some(after_release) = after_release {
                    after_release();
                }
            }
        }
        Ok(())
    }

    async fn restore_enrollment_clock(
        &self,
        registration: &mut TrackedCeremony,
    ) -> Result<(), AuraError> {
        // Restore observation only. The tracker execution slot remains empty
        // unless it still holds the actual original in-process birth root.
        let record = self.read_bound_enrollment_clock(registration).await?;
        registration.timeout_budget = record.budget;
        registration.negative_cleanup_budget = record.negative_cleanup_budget;
        registration.original_window_policy = record.original_policy;
        Ok(())
    }

    async fn read_bound_enrollment_clock(
        &self,
        registration: &TrackedCeremony,
    ) -> Result<StoredEnrollmentClock, AuraError> {
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
        if record.version != 4
            || record.ceremony != registration.ceremony_id
            || record.subject != registration.initiator_id
            || Some(record.device) != registration.enrollment_device_id
            || record.prestate != registration.prestate_hash
            || record.epoch != registration.new_epoch
            || record.budget.started_at_ms() != registration.timeout_budget.started_at_ms()
            || record.budget.deadline_at_ms() != registration.timeout_budget.deadline_at_ms()
            || record.original_policy != registration.original_window_policy
            || record.negative_cleanup_budget.started_at_ms()
                != registration.negative_cleanup_budget.started_at_ms()
            || record.negative_cleanup_budget.deadline_at_ms()
                != registration.negative_cleanup_budget.deadline_at_ms()
        {
            return Err(AuraError::invalid(
                "retained enrollment clock generation mismatch",
            ));
        }
        if !matches!(
            record.execution,
            super::enrollment_window::DurableEnrollmentExecutionState::Pending
                | super::enrollment_window::DurableEnrollmentExecutionState::Closed
                | super::enrollment_window::DurableEnrollmentExecutionState::Refused
        ) {
            return Err(AuraError::invalid(
                "retained enrollment clock lacks original Pending birth",
            ));
        }
        record
            .original_policy
            .require_cleanup(&record.budget, &record.negative_cleanup_budget)
            .map_err(AuraError::from)?;
        Ok(record)
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
            None,
            OriginalRegistrationClock::SelectAtRegistration,
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
        generation: &mut crate::runtime::effects::EnrollmentGenerationReservation<'_>,
        launch: &mut super::enrollment_window::OriginalIssuerLaunchWindow,
        request: super::ceremony_runner::CeremonyInitRequest,
        original_tasks: &TaskGroup,
    ) -> Result<(), AuraError> {
        if request.kind != CeremonyKind::DeviceEnrollment || self.shared.persistence.is_none() {
            return Err(crate::runtime::effects::held_registration_error(
                crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        }
        self.register_with_generation(
            Some(generation),
            Some(original_tasks),
            OriginalRegistrationClock::SelectedEnrollment(launch),
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
        generation: Option<&mut crate::runtime::effects::EnrollmentGenerationReservation<'_>>,
        original_tasks: Option<&TaskGroup>,
        original_clock: OriginalRegistrationClock<'_>,
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
        let _decision = match generation.as_deref() {
            Some(owned) => {
                owned.require_tracker(self)?;
                owned.require_original_birth()?;
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

        // Enrollment forwards the original launch selection. Only unrelated
        // ceremony admission selects an interval here; durable enrollment may
        // neither reconstruct nor renew the launch's endpoint or provider.
        let (now, original_birth) = match original_clock {
            OriginalRegistrationClock::SelectedEnrollment(launch) => {
                if kind != CeremonyKind::DeviceEnrollment {
                    return Err(AuraError::invalid(
                        "original enrollment clock cannot start another ceremony kind",
                    ));
                }
                let effects = self.shared.persistence.as_ref().ok_or_else(|| {
                    original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
                })?;
                let selected = launch.take_original_birth(effects)?;
                (selected.started_at, selected.birth)
            }
            OriginalRegistrationClock::SelectAtRegistration => {
                if generation.is_some() {
                    return Err(AuraError::invalid(
                        "owned enrollment requires its original launch selection",
                    ));
                }
                let now =
                    self.time
                        .physical_time()
                        .await
                        .map_err(|source| AuraError::Internal {
                            message: "select original ceremony registration clock".into(),
                            source: Some(Arc::new(source)),
                        })?;
                let execution_policy = Self::timeout_for_kind(kind);
                let shortcut_policy =
                    super::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(
                        execution_policy,
                        execution_policy,
                    )
                    .map_err(AuraError::from)?;
                let birth = super::enrollment_window::OriginalEnrollmentWindowBirth::allocate(
                    &now,
                    execution_policy,
                    shortcut_policy,
                )
                .map_err(AuraError::from)?;
                (now, birth)
            }
        };
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
            timeout_budget: original_birth.execution,
            shortcut_budget: Some(original_birth.shortcut),
            negative_cleanup_budget: original_birth.negative_cleanup,
            original_window_policy: original_birth.policy,
            prestate_hash,
            committed_at: None,
            committed_consensus_id: None,
        };

        let execution = if state.kind == CeremonyKind::DeviceEnrollment
            && self.shared.persistence.is_some()
        {
            let effects = self.shared.persistence.as_ref().ok_or_else(|| {
                original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
            })?;
            let generation = generation.ok_or_else(|| {
                crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
                )
            })?;
            let original_tasks = original_tasks.ok_or_else(|| {
                crate::runtime::effects::held_registration_error(
                    crate::runtime::effects::HeldEnrollmentRegistrationError::RequiredOwner,
                )
            })?;
            state = crate::handlers::invitation::enrollment_trust::persist_allocated_enrollment_registration(effects, generation, &state)
                .await.map_err(|source| AuraError::Internal { message: "retain original allocated enrollment registration".into(), source: Some(Arc::new(source)) })?;
            let birth =
                aura_core::time::timeout::acknowledge_initial_publication_with_timeout_budget(
                    effects.as_ref(),
                    &state.timeout_budget,
                    || async {
                        let birth = self
                            .publish_pending_issuer_birth(generation, &state)
                            .await?;
                        self.persist_enrollment_snapshot(&state).await?;
                        self.retain_live_enrollment_window(&birth).await?;
                        Ok::<_, AuraError>(birth)
                    },
                )
                .await
                .map_err(|source| match source {
                    aura_core::TimeoutRunError::Operation(source) => source,
                    aura_core::TimeoutRunError::Timeout(source) => AuraError::from(source),
                })?;
            Some(
                super::enrollment_window::EnrollmentExecutionRoot::from_issuer_birth(
                    birth,
                    original_tasks,
                ),
            )
        } else {
            self.persist_enrollment_snapshot(&state).await?;
            None
        };
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
                tracker.ceremonies.insert(
                    ceremony_id.clone(),
                    TrackedCeremonyEntry {
                        observed: state,
                        execution,
                        running: None,
                        acknowledged_issuer: None,
                        original_entry: Arc::new(()),
                        cancellation_task: None,
                        cancelled_notice_task: None,
                        issuer_task: None,
                    },
                );
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
        let mut durable = state.observed.clone();
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
                negative: None,
                tracker: OriginalEnrollmentTrackerReference::from_original(self)?,
                state: state.observed.clone(),
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
        let mut retained = state.observed.clone();
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
        let cancelled = Arc::new(VerifiedEnrollmentCancellationCapability {
            runtime_owner: effects.clone(),
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
        });
        Ok(RegisteredCancelledNoticeCapability {
            observation: Arc::new(CancellationClockObservationCapability {
                negative: Some(Arc::clone(&cancelled)),
                tracker: OriginalEnrollmentTrackerReference::from_original(self)?,
                state: state.observed.clone(),
                signed_expiry_ms: manifest.expires_at_ms,
            }),
            lease: Arc::new(lease),
            cancelled,
            manifest_digest: issued.digest(),
        })
    }

    pub async fn get(&self, ceremony_id: &CeremonyId) -> Result<TrackedCeremony, AuraError> {
        let state = self.shared.state.read().await;

        state
            .ceremonies
            .get(ceremony_id)
            .map(|entry| entry.observed.clone())
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
        #[cfg(test)]
        eprintln!("enrollment acceptance stage: verified response received ceremony={ceremony_id}");
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
        // The same owned readiness domain covers acceptance and terminal
        // progress. Waiters register before examining the canonical state.
        self.shared.terminal_changed.notify_waiters();
        #[cfg(test)]
        eprintln!("enrollment acceptance stage: validated response committed ceremony={ceremony_id} ready={result}");
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
        let recovered = crate::handlers::invitation::enrollment_trust::recover_enrollment_registration_for_admission(effects, ceremony_id)
            .await.map_err(|error| AuraError::Internal {
                message: "recover enrollment evidence".into(),
                source: Some(Arc::new(error)),
            })?;
        let mut registration = recovered.registration().clone();
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
        self.require_live_enrollment_window(&registration).await?;
        if registration.terminal_outcome.is_none() {
            crate::handlers::invitation::enrollment_trust::restore_recovered_pending_signing_generation(
                effects,
                signing,
                &recovered,
            )
            .await
            .map_err(|error| AuraError::Crypto {
                message: "restore exact pending signing generation before registration".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        }
        let proof = recovered.into_response();
        with_state_mut_validated(
            &self.shared.state,
            |tracker| {
                tracker
                    .ceremonies
                    .insert(ceremony_id.clone(), registration.into());
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
        expired.map_err(|source| AuraError::Internal {
            message: "enrollment activation deadline elapsed".into(),
            source: Some(Arc::new(source)),
        })?;
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

    pub(crate) fn retain_prepared_enrollment_activation<'a>(
        &'a self,
        recovery: crate::handlers::device_epoch_rotation::AuthenticatedPreparedEnrollmentRecovery<
            'a,
        >,
    ) -> Result<
        (
            EnrollmentActivationCapability<'a>,
            crate::handlers::device_epoch_rotation::AuthenticatedPreparedEnrollmentActivation,
        ),
        AuraError,
    > {
        let (authenticated, custody) = recovery.into_original_decision();
        custody.require_tracker(self)?;
        authenticated.require_original(&custody)?;
        let ceremony_id = authenticated.ceremony_id().clone();
        Ok((
            EnrollmentActivationCapability {
                tracker: self,
                ceremony_id,
                custody,
            },
            authenticated,
        ))
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
        let original = {
            let state = self.shared.state.read().await;
            state
                .ceremonies
                .get(proof.ceremony_id())
                .and_then(|entry| entry.running.as_ref())
                .map(|running| Arc::clone(&running.original))
                .ok_or_else(|| {
                    original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost)
                })?
        };
        let ceremony = proof.ceremony_id().clone();
        if let Err(proposed) = original.verified_rejection.set(proof) {
            let actual = original
                .verified_rejection
                .get()
                .ok_or_else(|| AuraError::invalid("original verified rejection disappeared"))?;
            if actual.ceremony_id() != proposed.ceremony_id()
                || actual.subject() != proposed.subject()
                || actual.device_id() != proposed.device_id()
                || actual.pending_epoch() != proposed.pending_epoch()
            {
                return Err(AuraError::permission_denied(
                    "contradictory original verified rejection",
                ));
            }
        }
        self.complete_under_decision_gate(
            &ceremony,
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

    async fn original_protected_terminal_decision(
        &self,
        current: &TrackedCeremony,
        outcome: CeremonyTerminalOutcome,
    ) -> Result<Option<(Arc<RegisteredEnrollmentWindowCapability>, [u8; 32])>, AuraError> {
        // Before registered publication, there is no terminal-signing owner.
        // Such failures still settle through their original prebirth cleanup.
        let original = {
            let registry = self.shared.state.read().await;
            registry
                .ceremonies
                .get(&current.ceremony_id)
                .and_then(|entry| entry.running.as_ref())
                .map(|running| Arc::clone(&running.original))
        };
        let Some(original) = original else {
            return Ok(None);
        };
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            AuraError::invalid("original terminal decision requires persistent runtime custody")
        })?;
        original.tracker.require_effects(effects.as_ref())?;
        if original.state.prestate_hash != current.prestate_hash
            || original.state.new_epoch != current.new_epoch
            || !Arc::ptr_eq(
                &original.state.enrollment_window_lease,
                &current.enrollment_window_lease,
            )
            || !original
                .state
                .timeout_budget
                .shares_observation_owner_with(&current.timeout_budget)
        {
            return Err(AuraError::permission_denied(
                "terminal decision belongs to another original registered owner",
            ));
        }
        let retained = self
            .retained_terminal_decision(&current.ceremony_id)
            .await?
            .ok_or_else(|| AuraError::invalid("original terminal decision is missing"))?;
        if retained.record.outcome != Some(outcome) || retained.prestate != current.prestate_hash {
            return Err(AuraError::permission_denied(
                "original terminal decision differs from protected first decision",
            ));
        }
        current
            .timeout_budget
            .validate_checkpoint_continuation_from(&retained.record.budget)
            .map_err(AuraError::from)?;
        let bytes = effects
            .secure_retrieve(
                &terminal_decision_location(&current.ceremony_id),
                &[aura_core::effects::SecureStorageCapability::Read],
            )
            .await?;
        let expected = serde_json::to_vec(&retained).map_err(|source| AuraError::Internal {
            message: "retain exact original terminal decision bytes".into(),
            source: Some(Arc::new(source)),
        })?;
        if bytes != expected {
            return Err(AuraError::permission_denied(
                "protected original terminal decision changed during receipt publication",
            ));
        }
        Ok(Some((original, aura_core::hash::hash(&bytes))))
    }

    async fn retain_original_negative_decision(
        &self,
        current: &TrackedCeremony,
        reason: CeremonyFailureReason,
    ) -> Result<(), AuraError> {
        let Some((original, decision_digest)) = self
            .original_protected_terminal_decision(current, CeremonyTerminalOutcome::Failed(reason))
            .await?
        else {
            return Ok(());
        };
        use aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind as Kind;
        let kind = match reason {
            CeremonyFailureReason::Rejected => Kind::Rejected,
            CeremonyFailureReason::Cancelled => Kind::Cancelled,
            CeremonyFailureReason::TimedOut => Kind::TimedOut,
            CeremonyFailureReason::ChoreographyFailed => Kind::ChoreographyFailed,
            CeremonyFailureReason::RuntimeFailed => Kind::RuntimeFailed,
            CeremonyFailureReason::Superseded => Kind::Superseded,
        };
        let receipt = OriginalEnrollmentNegativeDecisionReceipt {
            original: Arc::downgrade(&original),
            kind,
            decision_digest,
        };
        if let Err(proposed) = original.negative_decision.set(receipt) {
            let actual = original.negative_decision.get().ok_or_else(|| {
                AuraError::invalid("original negative decision receipt disappeared")
            })?;
            if actual.kind != proposed.kind || actual.decision_digest != proposed.decision_digest {
                return Err(AuraError::permission_denied(
                    "contradictory original negative decision receipt",
                ));
            }
        }
        Ok(())
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
        if current.kind == CeremonyKind::DeviceEnrollment {
            if let CeremonyTerminalOutcome::Failed(reason) = outcome {
                self.retain_original_negative_decision(&current, reason)
                    .await?;
            }
        }
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
            .map(|(id, ceremony)| (id.clone(), ceremony.observed.clone()))
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
        let (mut generation, retained) = effects
            .recover_initial_enrollment_allocation(ceremony, tracker)
            .await?;
        tracker
            .register_owned_device_enrollment(
                &mut generation,
                &mut super::super::enrollment_window::OriginalIssuerLaunchWindow::select(
                    effects.clone(),
                )
                .await?
                .0,
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
                &crate::task_registry::TaskSupervisor::new().group("recovered-allocation-refusal"),
            )
            .await
    }
    #[tokio::test]
    async fn interrupted_pre_live_allocation_cannot_recreate_original_birth() {
        use aura_core::effects::{SecureStorageEffects, SecureStorageLocation};
        let (_agents, interrupted, clock, id, result) = Box::pin(allocated_clock_fixture(
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
        clock
            .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                reason: "original fresh acknowledgment was lost".into(),
            })
            .await;
        let restarted = CeremonyTracker::new_with_storage(effects);
        let failure = register_original_fixture(&restarted, &original.ceremony_id)
            .await
            .expect_err("stored allocation cannot replace the lost original birth acknowledgment");
        assert!(matches!(
            std::error::Error::source(&failure)
                .and_then(|source| source
                    .downcast_ref::<crate::runtime::effects::HeldEnrollmentRegistrationError>()),
            Some(crate::runtime::effects::HeldEnrollmentRegistrationError::MissingOriginalBirth)
        ));
        assert!(restarted.get(&id).await.is_err());
        assert!(
            aura_core::effects::PhysicalTimeEffects::physical_time(&clock)
                .await
                .is_err(),
            "recovery refusal must leave the original provider fault unconsumed"
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
        let restarted = CeremonyTracker::new_with_storage(effects.clone());
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
        let (first, _clock, id) = registered_clock_fixture(
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
        let restarted = CeremonyTracker::new_with_storage(effects.clone());
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
        let (_agents, tracker, _clock, id, result) = allocated_clock_fixture(
            "registered_window_rejects_replaced_allocation_before_checkpoint",
            None,
        )
        .await;
        result.expect("actual fresh Pending birth");
        let capability = tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&id)
            .unwrap()
            .execution
            .take()
            .expect("original fresh root");
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
            capability.origin().require_pending().await.is_err(),
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
        let (_agents, tracker, clock, id, registered) = allocated_clock_fixture(
            "observed_snapshot_cannot_replace_registered_execution_window",
            None,
        )
        .await;
        registered.expect("actual fresh Pending birth");
        let _effects = tracker.shared.persistence.as_ref().unwrap().clone();
        let root = tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&id)
            .unwrap()
            .execution
            .take()
            .expect("original fresh execution root");
        let original = tracker.get(&id).await.unwrap();
        let mut observed = original.clone();
        observed.timeout_budget = aura_core::TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(50_000),
            Duration::from_secs(1_000),
        )
        .unwrap();
        observed.enrollment_window_lease = Arc::new(tokio::sync::Semaphore::new(1));
        assert_eq!(
            root.origin().budget().started_at_ms(),
            original.timeout_budget.started_at_ms()
        );
        assert_eq!(
            root.origin().budget().deadline_at_ms(),
            original.timeout_budget.deadline_at_ms()
        );
        assert_ne!(
            root.origin().budget().deadline_at_ms(),
            observed.timeout_budget.deadline_at_ms()
        );
        // The canonical allocation already yielded its sole move-owned root.
        // A mutated observed snapshot cannot return or replace that execution owner.
        root.origin()
            .require_pending()
            .await
            .expect("same original registered Pending owner remains valid");
        assert!(
            tracker
                .shared
                .state
                .read()
                .await
                .ceremonies
                .get(&id)
                .expect("original allocation remains registered")
                .execution
                .is_none(),
            "the retained root is not duplicated by snapshot observation"
        );
        assert!(root
            .origin()
            .budget()
            .shares_observation_owner_with(&original.timeout_budget));
        assert!(!root
            .origin()
            .budget()
            .shares_observation_owner_with(&observed.timeout_budget));
        let _ = clock;
    }

    #[tokio::test]
    async fn registered_window_operation_result_does_not_replace_original_terminal_acknowledgment()
    {
        let (_agents, tracker, clock, id, registered) = allocated_clock_fixture(
            "registered_window_postoperation_checkpoint_failure_prevents_success_publication",
            None,
        )
        .await;
        registered.expect("actual fresh Pending birth");
        let effects = tracker.shared.persistence.as_ref().unwrap().clone();
        let root = tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&id)
            .unwrap()
            .execution
            .take()
            .expect("original fresh execution root");
        let result = root
            .child()
            .execute(effects.as_ref(), || async {
                clock.set_time(6_000);
                Ok::<_, AuraError>("operation result")
            })
            .await
            .expect("actual bounded original operation");
        assert_eq!(result, "operation result");
        let retained = tracker.get(&id).await.unwrap();
        let record = tracker
            .read_bound_enrollment_clock(&retained)
            .await
            .unwrap();
        assert_eq!(
            record.execution,
            super::super::enrollment_window::DurableEnrollmentExecutionState::Pending
        );
        drop(root);
        let restarted = CeremonyTracker::new_with_storage(effects);
        assert!(
            register_original_fixture(&restarted, &id).await.is_err(),
            "operation result and retained Pending cannot recreate terminal root authority"
        );
    }

    async fn allocated_clock_fixture(
        test_identity: &str,
        fault: Option<AuraError>,
    ) -> (
        Vec<Arc<crate::AuraAgent>>,
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
                .bootstrap_signing_keys(
                    crate::runtime::services::RuntimeServiceContext::test_original(
                        agent.runtime().tasks(),
                        agent.runtime().effects(),
                    )
                    .await
                    .startup_window(),
                )
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
        let tracker = CeremonyTracker::new_with_storage(effects.clone());
        let plan = effects
            .prepare_authenticated_enrollment_rotation(&pin, &tracker)
            .await
            .expect("actual authenticated roster/prestate owner");
        let prestate = plan.prestate();
        let (epoch, _, _, mut generation) = effects
            .prepare_pinned_enrollment_rotation(
                &pin,
                &reserved,
                &id,
                &mut crate::runtime::effects::OriginalEnrollmentAllocationProducer::from_original(
                    plan,
                ),
            )
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
                &mut generation,
                &mut super::super::enrollment_window::OriginalIssuerLaunchWindow::select(
                    effects.clone(),
                )
                .await
                .expect("original fixture launch selection")
                .0,
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
                &issuer.runtime().tasks().group("original-clock-fixture"),
            )
            .await;
        drop(generation);
        (agents, tracker, clock, id, result)
    }
    async fn registered_clock_fixture(
        test_identity: &str,
    ) -> (
        CeremonyTracker,
        aura_testkit::time::ManualPhysicalClock,
        CeremonyId,
    ) {
        let (_agents, tracker, clock, id, result) =
            Box::pin(allocated_clock_fixture(test_identity, None)).await;
        result.expect("register actual held original allocation");
        (tracker, clock, id)
    }
    #[tokio::test]
    async fn original_tracker_loss_cannot_reconstruct_execution_from_retained_provider() {
        use aura_core::effects::StorageCoreEffects;
        let (tracker, _clock, id) =
            registered_clock_fixture("original-tracker-loss-refuses-execution").await;
        let original = OriginalEnrollmentTrackerReference::from_original(&tracker).unwrap();
        let effects = original.effects.clone();
        original.require_effects(effects.as_ref()).unwrap();
        let snapshot = tracker.get(&id).await.unwrap();
        let keys = effects.list_keys(None).await.unwrap();
        let mut stored = Vec::new();
        for key in &keys {
            stored.push((key.clone(), effects.retrieve(key).await.unwrap()));
        }
        drop(tracker);
        let error = original
            .require_effects(effects.as_ref())
            .expect_err("retained provider and serialized state cannot replace the original actor");
        assert!(matches!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<OriginalEnrollmentTrackerError>(),
            Some(OriginalEnrollmentTrackerError::OwnerLost),
        ));
        assert!(original.upgrade().is_err());
        assert_eq!(snapshot.ceremony_id, id);
        assert_eq!(effects.list_keys(None).await.unwrap(), keys);
        for (key, bytes) in stored {
            assert_eq!(effects.retrieve(&key).await.unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn original_completion_observation_releases_execution_lease_without_renewing_clock() {
        let (tracker, clock, id) =
            registered_clock_fixture("original-completion-observation-lease").await;
        let (foreign, _, _) =
            registered_clock_fixture("foreign-completion-observation-lease").await;
        let effects = tracker.shared.persistence.as_ref().unwrap().clone();
        let state = tracker
            .get(&id)
            .await
            .expect("actual original registered allocation");
        let semaphore = state.enrollment_window_lease.clone();
        let deadline = state.timeout_budget.deadline_at_ms();
        let execution = tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&id)
            .unwrap()
            .execution
            .take()
            .expect("actual fresh acknowledged execution root");
        let observation = execution.origin().completion_observation();
        assert!(observation
            .budget()
            .shares_observation_owner_with(execution.origin().budget()));
        assert!(
            semaphore.clone().try_acquire_owned().is_err(),
            "observation does not release a still-live executor"
        );
        assert!(
            observation
                .require_effects(foreign.shared.persistence.as_ref().unwrap().as_ref())
                .is_err(),
            "another physical runtime cannot drive original completion observation"
        );
        drop(execution);
        let registered_permit = semaphore
            .clone()
            .try_acquire_owned()
            .expect("completion observer retains no execution permit during registered handoff");
        observation
            .require_effects(effects.as_ref())
            .expect("actual original effect owner");
        observation
            .require_pending()
            .await
            .expect("genuine original protected checkpoint remains valid after lease transfer");
        assert_eq!(observation.budget().deadline_at_ms(), deadline);
        clock.set_time(deadline + 1);
        let now = effects
            .physical_time()
            .await
            .expect("actual original local clock");
        assert!(observation.budget().remaining_at(&now).is_err());
        observation
            .require_pending()
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
        let restarted = CeremonyTracker::new_with_storage(effects);
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
        let (first, _clock, id) =
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
        let restarted = CeremonyTracker::new_with_storage(effects.clone());
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
        register_original_fixture(&restarted, &id).await.expect_err(
            "even an exact stored allocation cannot recreate its consumed birth acknowledgment",
        );
        assert!(restarted.get(&id).await.is_err());
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
    async fn registered_window_pending_read_failure_blocks_operation_and_retains_storage_source() {
        use aura_core::effects::{SecureStorageEffects, SecureStorageLocation};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (_agents, tracker, _clock, id, registered) = allocated_clock_fixture(
            "registered_window_checkpoint_failure_blocks_operation_and_retains_storage_source",
            None,
        )
        .await;
        registered.expect("actual fresh Pending birth");
        let effects = tracker.shared.persistence.as_ref().unwrap().clone();
        let root = tracker
            .shared
            .state
            .write()
            .await
            .ceremonies
            .get_mut(&id)
            .unwrap()
            .execution
            .take()
            .expect("original fresh execution root");
        let location = SecureStorageLocation::new("enrollment_clock_v1", id.to_string());
        effects
            .secure_delete(
                &location,
                &[aura_core::effects::SecureStorageCapability::Delete],
            )
            .await
            .unwrap();
        let polls = AtomicUsize::new(0);
        let error = root
            .child()
            .execute(effects.as_ref(), || async {
                polls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, AuraError>(())
            })
            .await
            .expect_err("required original Pending read precedes operation polling");
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        let mut native = false;
        while let Some(cause) = source {
            if let Some(missing) =
                cause.downcast_ref::<aura_core::effects::secure::SecureStorageRecordMissing>()
            {
                assert_eq!(missing.location(), &location);
                native = true;
            }
            source = cause.source();
        }
        assert!(
            native,
            "actual selected storage failure remains in source chain"
        );
        assert!(!effects.secure_exists(&location).await.unwrap());
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
            let ceremony_id = start.ceremony_id;
            issuer
                .invitations()
                .expect("issuer invitation service")
                .cancel(&invitation.invitation_id)
                .await
                .expect("actual issued cancellation owner");
            let restarted = CeremonyTracker::new_with_storage(effects);
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
        let restarted = CeremonyTracker::new_with_storage(effects);
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
                let ((issuer, _invitee, _invitation, start, _acceptance, verified), original) =
                    crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation("tracker-owned-acceptance", None, None).await;
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
                original.observe_original_finalizer().await.expect("actual original finalizer commits verified acceptance");
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

                {
                    let original_fixture_birth =
                        crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(
                            &PhysicalTime::exact(0),
                            Duration::from_secs(30),
                            crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(
                                Duration::from_secs(30), Duration::from_secs(30),
                            ).expect("valid shortcut fixture policy"),
                        )
                        .expect("valid original fixture birth");
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
                        timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
                        negative_cleanup_budget: original_fixture_birth.negative_cleanup,
                        original_window_policy: original_fixture_birth.policy,
                        prestate_hash: Hash32([0; 32]),
                        committed_at: None,
                        committed_consensus_id: None,
                    }
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
                        map.insert(test_ceremony_id("test"), { let original_fixture_birth = crate::runtime::services::enrollment_window::OriginalEnrollmentWindowBirth::allocate(&PhysicalTime::exact(0), Duration::from_secs(30), crate::runtime::services::enrollment_window::OriginalShortcutWindowPolicy::bounded_extension(Duration::from_secs(30), Duration::from_secs(30)).expect("valid shortcut fixture policy")).expect("valid original fixture birth"); TrackedCeremony {
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
                timeout_budget: original_fixture_birth.execution,
            shortcut_budget: Some(original_fixture_birth.shortcut),
    negative_cleanup_budget: original_fixture_birth.negative_cleanup,
    original_window_policy: original_fixture_birth.policy,
                            prestate_hash: Hash32([0; 32]),
                            committed_at: None,
                            committed_consensus_id: None,
                        } }.into());
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
    fn original_invitation_binding_excludes_only_mutable_status() {
        use aura_core::invitation::{Invitation, InvitationStatus, InvitationType};
        let original = Invitation {
            invitation_id: aura_core::InvitationId::new("immutable-issuance"),
            context_id: aura_core::ContextId::new_from_entropy([211; 32]),
            sender_id: AuthorityId::new_from_entropy([212; 32]),
            receiver_id: AuthorityId::new_from_entropy([213; 32]),
            invitation_type: InvitationType::Contact { nickname: None },
            status: InvitationStatus::Pending,
            created_at: 5_000,
            expires_at: Some(6_000),
            message: Some("original".into()),
            receiver_nickname: Some("receiver".into()),
        };
        let identity = original_invitation_binding(&original).unwrap();
        for status in [
            InvitationStatus::Accepted,
            InvitationStatus::Declined,
            InvitationStatus::Cancelled,
            InvitationStatus::Expired,
        ] {
            let mut observed = original.clone();
            observed.status = status;
            assert_eq!(original_invitation_binding(&observed).unwrap(), identity);
        }
        let changes: [fn(&mut Invitation); 9] = [
            |v| v.invitation_id = aura_core::InvitationId::new("another-issuance"),
            |v| v.context_id = aura_core::ContextId::new_from_entropy([214; 32]),
            |v| v.sender_id = AuthorityId::new_from_entropy([215; 32]),
            |v| v.receiver_id = AuthorityId::new_from_entropy([216; 32]),
            |v| {
                v.invitation_type = InvitationType::Contact {
                    nickname: Some("changed".into()),
                }
            },
            |v| v.created_at += 1,
            |v| v.expires_at = Some(6_001),
            |v| v.message = None,
            |v| v.receiver_nickname = None,
        ];
        for change in changes {
            let mut replaced = original.clone();
            change(&mut replaced);
            assert_ne!(
                original_invitation_binding(&replaced).unwrap(),
                identity,
                "every immutable invitation field remains bound"
            );
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
            canonical_invitation: vec![9],
        };
        let exact = RegisteredEnrollmentNoticeBindingCapability {
            manifest_digest: [0x41; 32],
            transcript: vec![1, 2, 3],
            expires_at_ms: 400,
            canonical_invitation: vec![9],
        };
        RegisteredEnrollmentWindowCapability::require_same_notice_binding(&original, &exact)
            .expect("same canonical identity may be observed repeatedly");
        let replacements = [
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x42; 32],
                transcript: vec![1, 2, 3],
                expires_at_ms: 400,
                canonical_invitation: vec![9],
            },
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x41; 32],
                transcript: vec![1, 2, 4],
                expires_at_ms: 400,
                canonical_invitation: vec![9],
            },
            RegisteredEnrollmentNoticeBindingCapability {
                manifest_digest: [0x41; 32],
                transcript: vec![1, 2, 3],
                expires_at_ms: 401,
                canonical_invitation: vec![9],
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

impl CeremonyTracker {
    async fn persist_original_negative_clock(
        &self,
        state: &TrackedCeremony,
        effects: &crate::runtime::AuraEffectSystem,
        _owner: OriginalNegativeCheckpointOwner,
    ) -> Result<super::enrollment_window::DurableEnrollmentExecutionState, AuraError> {
        use aura_core::effects::{SecureStorageCapability, SecureStorageLocation};
        let _write = self.shared.persistence_guard.lock().await;
        let mut record = self.read_bound_enrollment_clock(state).await?;
        state
            .timeout_budget
            .validate_checkpoint_continuation_from(&record.budget)
            .map_err(AuraError::from)?;
        // Required negative acknowledgment retains the latest original history,
        // including a rollback discovered after the positive checkpoint write.
        record.budget = state.timeout_budget.clone();
        if record.execution == super::enrollment_window::DurableEnrollmentExecutionState::Pending {
            record.execution = super::enrollment_window::DurableEnrollmentExecutionState::Refused;
        }
        record.negative_cleanup_budget = state.negative_cleanup_budget.clone();
        let retained_decision = record.execution;
        let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Serialization {
            message: "encode original issuer negative checkpoint".into(),
            source: Some(Arc::new(source)),
        })?;
        let location =
            SecureStorageLocation::new("enrollment_clock_v1", state.ceremony_id.to_string());
        #[cfg(test)]
        if let Some(source) = {
            let injected = self.shared.negative_checkpoint_fault.lock().await;
            if injected.as_ref().is_some_and(|(owner, _)| *owner == _owner) {
                injected.as_ref().map(|(_, source)| source.clone())
            } else {
                None
            }
        } {
            if let Some(observed) = self
                .shared
                .negative_checkpoint_fault_observer
                .lock()
                .await
                .take()
            {
                let _ = observed.send(source.clone());
            }
            return Err(source);
        }
        effects
            .secure_store(&location, &bytes, &[SecureStorageCapability::Write])
            .await?;
        let retained = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if retained != bytes {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::BirthReadbackChanged,
            ));
        }
        Ok(retained_decision)
    }
}

/// Actual failed cancellation task disposal; domain refusal is separate from
/// acknowledged resource settlement and remains bound to its original task.
#[cfg(feature = "simulation")]
pub(crate) struct OriginalSimulationCancellationFailure {
    _registration: Arc<OriginalCancellationTaskRegistration>,
    disposal: crate::task_registry::OriginalRequiredTaskDisposalObservation,
    source: AuraError,
}
#[cfg(feature = "simulation")]
impl OriginalSimulationCancellationFailure {
    pub(crate) fn source(&self) -> &AuraError {
        &self.source
    }
    pub(crate) fn matches_shutdown(
        &self,
        failure: &crate::task_registry::TaskSupervisionError,
    ) -> bool {
        self.disposal.matches_supervision_failure(failure)
    }
}
/// Original entry reference issued before required cancellation task admission.
/// Neither identifiers nor later snapshots can construct this reference.
pub(crate) struct OriginalCancellationTaskRegistration {
    completion:
        std::sync::OnceLock<Result<crate::handlers::invitation::InvitationResult, AuraError>>,
    completion_changed: tokio::sync::Notify,
    cleanup_observation: std::sync::OnceLock<Arc<OriginalCancellationRetryFailure>>,
    tracker: OriginalEnrollmentTrackerReference,
    original_entry: Arc<()>,
    ceremony: CeremonyId,
    manifest_digest: [u8; 32],

    original_budget: aura_core::TimeoutBudget,
    role: OriginalCancellationTaskRole,
    #[cfg(test)]
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    delivery_gate: parking_lot::Mutex<Option<OriginalCancelledNoticeDeliveryGate>>,
}
#[derive(Clone, Copy)]
pub(crate) enum OriginalCancellationTaskRole {
    Publication,
    Notice,
}
struct RetainedOriginalCancellationTask {
    original: Arc<OriginalCancellationTaskRegistration>,
    _control: tokio::sync::Mutex<crate::task_registry::OriginalRequiredTaskHandle>,
}
impl CeremonyTracker {
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    pub(crate) fn prepare_original_cancellation_task(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        role: OriginalCancellationTaskRole,
    ) -> Result<Arc<OriginalCancellationTaskRegistration>, AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
        })?;
        issued.require_runtime_owner(effects.as_ref())?;
        let state = self
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "select original cancellation task entry".into(),
                source: Some(Arc::new(source)),
            })?;
        let manifest = issued.manifest();
        let entry = state
            .ceremonies
            .get(&manifest.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if entry.kind != CeremonyKind::DeviceEnrollment
            || entry.initiator_id != manifest.subject
            || entry.enrollment_device_id != Some(manifest.invitee_device)
            || entry.new_epoch != manifest.pending_epoch
        {
            return Err(AuraError::permission_denied(
                "another original cancellation task entry",
            ));
        }
        if let Some(running) = &entry.running {
            running.original.require_issued_notice_control(issued)?;
        }
        let occupied = match role {
            OriginalCancellationTaskRole::Publication => entry.cancellation_task.is_some(),
            OriginalCancellationTaskRole::Notice => entry.cancelled_notice_task.is_some(),
        };
        if occupied {
            return Err(AuraError::permission_denied(
                "original cancellation task already admitted; reconcile its retained control",
            ));
        }
        Ok(Arc::new(OriginalCancellationTaskRegistration {
            completion: std::sync::OnceLock::new(),
            completion_changed: tokio::sync::Notify::new(),
            cleanup_observation: std::sync::OnceLock::new(),
            tracker: OriginalEnrollmentTrackerReference::from_original(self)?,
            original_entry: Arc::clone(&entry.original_entry),
            ceremony: manifest.ceremony.clone(),
            manifest_digest: issued.digest(),
            original_budget: entry.timeout_budget.clone(),
            role,
            #[cfg(test)]
            delivery_gate: parking_lot::Mutex::new(None),
        }))
    }
}
impl CeremonyTracker {
    pub(crate) fn existing_original_cancellation_task(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        role: OriginalCancellationTaskRole,
    ) -> Result<Option<Arc<OriginalCancellationTaskRegistration>>, AuraError> {
        let effects = self.shared.persistence.as_ref().ok_or_else(|| {
            original_tracker_refusal(OriginalEnrollmentTrackerError::MissingProvider)
        })?;
        issued.require_runtime_owner(effects.as_ref())?;
        let state = self
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original cancellation task".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&issued.manifest().ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let task = match role {
            OriginalCancellationTaskRole::Publication => &entry.cancellation_task,
            OriginalCancellationTaskRole::Notice => &entry.cancelled_notice_task,
        }
        .as_ref();
        let Some(task) = task else {
            return Ok(None);
        };
        if let Some(running) = &entry.running {
            running.original.require_issued_notice_control(issued)?;
        }
        if !Arc::ptr_eq(&entry.original_entry, &task.original.original_entry)
            || task.original.manifest_digest != issued.digest()
        {
            return Err(AuraError::permission_denied(
                "another original cancellation task",
            ));
        }
        Ok(Some(Arc::clone(&task.original)))
    }
    #[cfg(test)]
    pub(crate) fn original_cancellation_task_for_test(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        role: OriginalCancellationTaskRole,
    ) -> Result<Arc<OriginalCancellationTaskRegistration>, AuraError> {
        self.existing_original_cancellation_task(issued, role)?
            .ok_or_else(|| AuraError::invalid("original cancellation task not admitted"))
    }
    #[cfg(test)]
    pub(crate) fn original_actor_alive_for_test(&self) -> impl Fn() -> bool {
        let original = Arc::downgrade(&self.shared);
        move || original.upgrade().is_some()
    }
}
#[derive(thiserror::Error)]
#[error("original cancellation failed: {source}; actual cleanup observations remain retained")]
struct OriginalCancellationRetryFailure {
    #[source]
    source: AuraError,
    observations: super::issuer_error_custody::SynchronousCustodyCell<(
        Vec<AuraError>,
        Option<crate::task_registry::OriginalRequiredTaskDisposalObservation>,
    )>,
}
impl std::fmt::Debug for OriginalCancellationRetryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (independent, disposed) = self
            .observations
            .with(|retained| (retained.0.clone(), retained.1.is_some()));
        f.debug_struct("OriginalCancellationRetryFailure")
            .field("source", &self.source)
            .field("independent", &independent)
            .field("original_disposal_retained", &disposed)
            .finish()
    }
}
impl OriginalCancellationTaskRegistration {
    #[cfg(test)]
    pub(crate) async fn publish_cleanup_observation_for_test(&self, actual: Result<(), AuraError>) {
        if let Ok(tracker) = self.tracker.upgrade() {
            if let Some(sender) = tracker
                .shared
                .cancellation_cleanup_observer
                .lock()
                .await
                .take()
            {
                let _ = sender.send(actual);
            }
        }
    }

    pub(crate) fn retain_completion(
        &self,
        result: Result<crate::handlers::invitation::InvitationResult, AuraError>,
    ) {
        let _ = self.completion.set(result);
        self.completion_changed.notify_waiters();
    }
    #[cfg(feature = "simulation")]
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalCancellationTaskRegistration", receiver_type = OriginalCancellationTaskRegistration, family = "runtime_helper")]
    pub(crate) async fn observe_simulated_failure_disposal(
        self: &Arc<Self>,
        effects: &AuraEffectSystem,
    ) -> Result<OriginalSimulationCancellationFailure, AuraError> {
        if !std::ptr::eq(self.tracker.effects.as_ref(), effects) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::MissingProvider,
            ));
        }
        let actual = self.retained()?;
        let observer = {
            let control = actual._control.lock().await;
            control.disposal_observer()
        };
        let disposal =
            aura_core::execute_with_timeout_budget(effects, &self.original_budget, || async {
                Ok::<_, AuraError>(observer.await_disposal().await)
            })
            .await
            .map_err(|source| match source {
                aura_core::TimeoutRunError::Operation(source) => source,
                aura_core::TimeoutRunError::Timeout(source) => source.into(),
            })?;
        {
            let control = actual._control.lock().await;
            disposal.require_original(&control)?;
        }
        let source = match (self.completion.get(), disposal.outcome()) {
            (Some(Err(source)), aura_core::effects::task::RequiredTaskExit::Failed(_)) => {
                source.clone()
            }
            _ => {
                return Err(AuraError::permission_denied(
                    "cancellation failure requires actual failed resource disposal",
                ))
            }
        };
        Ok(OriginalSimulationCancellationFailure {
            _registration: Arc::clone(self),
            disposal,
            source,
        })
    }
    pub(crate) async fn observe_completion(
        self: &Arc<Self>,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<crate::handlers::invitation::InvitationResult, AuraError> {
        let original_control = self.retained()?;
        if !std::ptr::eq(self.tracker.effects.as_ref(), effects) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::MissingProvider,
            ));
        }
        let mut selected_completion = None;
        let observed = match self.completion.get() {
            Some(Err(source)) => Err(source.clone()),
            _ => aura_core::execute_with_timeout_budget(effects, &self.original_budget, || async {
                loop {
                    let changed = self.completion_changed.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    if let Some(actual) = self.completion.get() {
                        let actual = actual.clone();
                        selected_completion = Some(actual.clone());
                        return actual;
                    }
                    changed.await;
                }
            })
            .await
            .map_err(|source| match source {
                aura_core::TimeoutRunError::Operation(source) => source,
                aura_core::TimeoutRunError::Timeout(source) => {
                    let later = AuraError::from(source);
                    match selected_completion.as_ref() {
                        Some(Err(earlier)) => {
                            original_issuer_cleanup_observation_failure(earlier.clone(), later)
                        }
                        _ => later,
                    }
                }
            }),
        };
        let Err(original_failure) = observed else {
            return observed;
        };
        if let Some(actual) = self
            .cleanup_observation
            .get()
            .filter(|actual| actual.observations.with(|retained| retained.1.is_some()))
        {
            return Err(AuraError::Internal {
                message: "original cancellation domain failure and actual cleanup observation"
                    .into(),
                source: Some(actual.clone()),
            });
        }
        // Issue the actual original cleanup request before observing its old
        // success window. Even an expired/failed selected read cannot suppress
        // mandatory work on the retained original executor.
        let requested = original_control
            ._control
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original cancellation control is already borrowed".into(),
                source: Some(Arc::new(source)),
            })
            .and_then(|mut control| {
                use futures::FutureExt;
                control.request_resident_cleanup_retry()?;
                let actual = control.disposal_observer().await_disposal().now_or_never();
                if let Some(actual) = &actual {
                    actual.require_original(&control)?;
                }
                Ok(actual)
            });
        if let Err(source) = requested {
            // A refused observer borrow/request is not a new host-cleanup
            // producer occurrence. Return it with the original source without
            // appending it to the canonical cleanup occurrence inventory.
            return Err(original_issuer_cleanup_observation_failure(
                original_failure,
                source,
            ));
        }
        if let Ok(Some(disposal)) = requested {
            let independent = match disposal.outcome() {
                aura_core::effects::task::RequiredTaskExit::Failed(source) => vec![source],
                _ => Vec::new(),
            };
            let actual = self.cleanup_observation.get_or_init(|| {
                Arc::new(OriginalCancellationRetryFailure {
                    source: original_failure,
                    observations: super::issuer_error_custody::SynchronousCustodyCell::new((
                        Vec::new(),
                        None,
                    )),
                })
            });
            actual.observations.with(|retained| {
                if retained.1.is_none() {
                    retained.0.extend(independent);
                    retained.1 = Some(disposal);
                }
            });
            return Err(AuraError::Internal {
                message: "original cancellation domain failure and actual cleanup observation"
                    .into(),
                source: Some(actual.clone()),
            });
        }
        // A cached domain error does not settle the original physical owner.
        // Only its resident original supervisor polls the actual cleanup future;
        // cancellation of this bounded observer cannot cancel that host work.
        let mut completed = None;
        let validation =
            aura_core::execute_with_timeout_budget(effects, &self.original_budget, || async {
                use aura_core::effects::task::RequiredTaskControl;
                use futures::FutureExt;
                let mut control = original_control._control.lock().await;
                let observer = control.disposal_observer();
                let (disposal, retry_failure) =
                    if let Some(disposal) = observer.await_disposal().now_or_never() {
                        disposal.require_original(&control)?;
                        (Some(disposal), None)
                    } else {
                        #[cfg(not(target_arch = "wasm32"))]
                        let retry = control.reconcile().await;
                        #[cfg(target_arch = "wasm32")]
                        let retry = control.reconcile_local().await;
                        let disposal = observer.await_disposal().now_or_never();
                        if let Some(actual) = &disposal {
                            actual.require_original(&control)?;
                        }
                        let failure = retry.err().or_else(|| {
                            disposal.is_none().then(|| {
                                AuraError::invalid(
                                    "original cancellation cleanup has no actual disposal",
                                )
                            })
                        });
                        (disposal, failure)
                    };
                completed = Some((disposal, retry_failure));
                Ok::<_, AuraError>(())
            })
            .await;
        let mut independent = Vec::new();
        let mut disposal = None;
        if let Some((actual, failure)) = completed {
            if let Some(source) = failure {
                independent.push(source);
            }
            if let Some(actual) = actual {
                if let aura_core::effects::task::RequiredTaskExit::Failed(source) = actual.outcome()
                {
                    independent.push(source);
                }
                disposal = Some(actual);
            }
        }
        if let Err(source) = validation {
            independent.push(match source {
                aura_core::TimeoutRunError::Operation(source) => source,
                aura_core::TimeoutRunError::Timeout(source) => source.into(),
            });
        }
        let actual = self.cleanup_observation.get_or_init(|| {
            Arc::new(OriginalCancellationRetryFailure {
                source: original_failure,
                observations: super::issuer_error_custody::SynchronousCustodyCell::new((
                    Vec::new(),
                    None,
                )),
            })
        });
        actual.observations.with(|retained| {
            // One actual disposal receipt is immutable evidence; reobserving it
            // does not create another failed-domain occurrence.
            if retained.1.is_none() {
                retained.0.extend(independent);
                retained.1 = disposal;
            }
        });
        Err(AuraError::Internal {
            message: "original cancellation domain failure and actual cleanup observation".into(),
            source: Some(actual.clone()),
        })
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalCancellationTaskRegistration",
        receiver_type = OriginalCancellationTaskRegistration,
        family = "runtime_helper"
    )]
    pub(crate) fn require_notice_input(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &crate::runtime::AuraEffectSystem,
    ) -> Result<(), AuraError> {
        issued.require_runtime_owner(effects)?;
        let tracker = self.tracker.upgrade()?;
        if !std::ptr::eq(self.tracker.effects.as_ref(), effects)
            || self.manifest_digest != issued.digest()
            || self.ceremony != issued.manifest().ceremony
            || !matches!(self.role, OriginalCancellationTaskRole::Notice)
        {
            return Err(AuraError::permission_denied(
                "another original notice admission",
            ));
        }
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "validate original notice admission".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&entry.original_entry, &self.original_entry)
            || entry.cancelled_notice_task.is_some()
        {
            return Err(AuraError::permission_denied(
                "original notice admission replaced or occupied",
            ));
        }
        Ok(())
    }
    pub(crate) fn publish(
        self: &Arc<Self>,
        pending: &mut Option<crate::task_registry::OriginalRequiredTaskHandle>,
    ) -> Result<(), AuraError> {
        let tracker = self.tracker.upgrade()?;
        let mut state = tracker
            .shared
            .state
            .try_write()
            .map_err(|source| AuraError::Internal {
                message: "retain original cancellation task control".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get_mut(&self.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        if !Arc::ptr_eq(&entry.original_entry, &self.original_entry) {
            return Err(AuraError::permission_denied(
                "original cancellation entry was replaced",
            ));
        }
        let slot = match self.role {
            OriginalCancellationTaskRole::Publication => &mut entry.cancellation_task,
            OriginalCancellationTaskRole::Notice => &mut entry.cancelled_notice_task,
        };
        if slot.is_some() || pending.is_none() {
            return Err(AuraError::permission_denied(
                "original cancellation control already retained",
            ));
        }
        // Every fallible check precedes synchronous transfer of the issued handle.
        let actual = pending
            .take()
            .ok_or_else(|| AuraError::invalid("original cancellation control missing"))?;
        *slot = Some(Arc::new(RetainedOriginalCancellationTask {
            original: Arc::clone(self),
            _control: tokio::sync::Mutex::new(actual),
        }));
        Ok(())
    }
    pub(crate) async fn observe_publication(
        &self,
        effects: &crate::runtime::AuraEffectSystem,
        admission: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), AuraError> {
        self.tracker.upgrade()?;
        let actual = &self.tracker.effects;
        if !std::ptr::eq(actual.as_ref(), effects) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::MissingProvider,
            ));
        }
        aura_core::execute_with_timeout_budget(effects, &self.original_budget, || async {
            admission.await.map_err(|source| AuraError::Internal {
                message: "original cancellation registration publication lost".into(),
                source: Some(Arc::new(source)),
            })
        })
        .await
        .map_err(|source| match source {
            aura_core::TimeoutRunError::Operation(source) => source,
            aura_core::TimeoutRunError::Timeout(source) => source.into(),
        })
    }
    fn retained(self: &Arc<Self>) -> Result<Arc<RetainedOriginalCancellationTask>, AuraError> {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "observe original cancellation control".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let slot = match self.role {
            OriginalCancellationTaskRole::Publication => &entry.cancellation_task,
            OriginalCancellationTaskRole::Notice => &entry.cancelled_notice_task,
        };
        let actual = slot
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original cancellation control not published"))?;
        if !Arc::ptr_eq(&entry.original_entry, &self.original_entry)
            || !Arc::ptr_eq(&actual.original, self)
        {
            return Err(AuraError::permission_denied(
                "another original cancellation control",
            ));
        }
        Ok(Arc::clone(actual))
    }
    #[cfg(test)]
    pub(crate) async fn require_original_disposal_for_test(
        self: &Arc<Self>,
        disposal: &crate::task_registry::OriginalRequiredTaskDisposalObservation,
    ) -> Result<(), AuraError> {
        let actual = self.retained()?;
        let control = actual._control.lock().await;
        disposal.require_original(&control)
    }
    #[cfg(test)]
    pub(crate) async fn disposal_observer(
        self: &Arc<Self>,
    ) -> Result<crate::task_registry::OriginalRequiredTaskDisposalObserver, AuraError> {
        let actual = self.retained()?;
        let control = actual._control.lock().await;
        Ok(control.disposal_observer())
    }
}

/// One original execution permit shared by all original issuer descendants.
/// Releasing exclusivity never restores execution eligibility for this cell.
pub(super) struct OriginalRegisteredExecutionLease {
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    state: parking_lot::Mutex<OriginalRegisteredExecutionLeaseState>,
}
struct OriginalRegisteredExecutionLeaseState {
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    active_work: usize,
    resource_admission_closed: bool,
}
/// Actual work admission, not a count of retained child handles. Drop releases
/// only this borrow; the original permit stays held until genuine terminal ACK.
pub(super) struct OriginalRegisteredExecutionBorrow {
    original: Arc<OriginalRegisteredExecutionLease>,
}
impl Drop for OriginalRegisteredExecutionBorrow {
    fn drop(&mut self) {
        self.original.state.lock().active_work -= 1;
    }
}
#[derive(Debug, thiserror::Error)]
#[error("the original enrollment execution lease was terminally released")]
struct OriginalExecutionLeaseReleased;
#[derive(Debug, thiserror::Error)]
#[error("the original enrollment execution still has {0} active work borrows")]
struct OriginalExecutionStillBorrowed(usize);
#[derive(Debug, thiserror::Error)]
#[error("the original enrollment execution work-borrow limit was reached")]
struct OriginalExecutionBorrowLimit;
impl OriginalRegisteredExecutionLease {
    #[allow(
        clippy::disallowed_types,
        reason = "Synchronous runtime custody deposit; the lock guard never crosses an await"
    )]
    fn new(permit: tokio::sync::OwnedSemaphorePermit) -> Self {
        Self {
            state: parking_lot::Mutex::new(OriginalRegisteredExecutionLeaseState {
                permit: Some(permit),
                active_work: 0,
                resource_admission_closed: false,
            }),
        }
    }
    pub(super) fn require_active(&self) -> Result<(), AuraError> {
        if self.state.lock().permit.is_some() {
            return Ok(());
        }
        Err(AuraError::PermissionDenied {
            message: "original enrollment execution is permanently closed".into(),
            source: Some(Arc::new(OriginalExecutionLeaseReleased)),
        })
    }
    pub(super) fn borrow_execution(
        self: &Arc<Self>,
    ) -> Result<OriginalRegisteredExecutionBorrow, AuraError> {
        let mut state = self.state.lock();
        if state.permit.is_none() || state.resource_admission_closed {
            return Err(AuraError::PermissionDenied {
                message: "original enrollment execution is permanently closed".into(),
                source: Some(Arc::new(OriginalExecutionLeaseReleased)),
            });
        }
        state.active_work =
            state
                .active_work
                .checked_add(1)
                .ok_or_else(|| AuraError::Internal {
                    message: "original execution work admission refused".into(),
                    source: Some(Arc::new(OriginalExecutionBorrowLimit)),
                })?;
        Ok(OriginalRegisteredExecutionBorrow {
            original: Arc::clone(self),
        })
    }
    fn close_resource_admission(&self) -> Result<(), AuraError> {
        let mut state = self.state.lock();
        if state.active_work != 0 {
            return Err(AuraError::PermissionDenied {
                message: "original resource closure has active execution work".into(),
                source: Some(Arc::new(OriginalExecutionStillBorrowed(state.active_work))),
            });
        }
        state.resource_admission_closed = true;
        Ok(())
    }
    fn release_after_acknowledgment(&self) -> Result<(), AuraError> {
        let mut state = self.state.lock();
        if state.active_work != 0 {
            return Err(AuraError::PermissionDenied {
                message: "original execution work remains unresolved".into(),
                source: Some(Arc::new(OriginalExecutionStillBorrowed(state.active_work))),
            });
        }
        let actual = state.permit.take();
        drop(state);
        // Wake canonical semaphore admission only after releasing the cell lock.
        drop(actual);
        Ok(())
    }
}
impl RegisteredEnrollmentWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalIssuerNegativeCheckpointReceipt",
        family = "runtime_helper"
    )]
    pub(super) fn release_negative_execution_lease(
        self: &Arc<Self>,
        terminal: &crate::handlers::invitation::OriginalIssuerCancellationTerminal,
        checkpoint: &OriginalIssuerNegativeCheckpointReceipt,
        disposal: &crate::task_registry::TaskGroupDisposalAcknowledgment,
        original_children: &crate::task_registry::TaskGroup,
    ) -> Result<(), AuraError> {
        self.require_issued_notice_control(terminal.issued())?;
        checkpoint.require_original(self)?;
        disposal.require_original_group(original_children)?;
        self.lease.release_after_acknowledgment()
    }
}

#[cfg(test)]
mod original_execution_lease_tests {
    use super::*;
    use std::future::Future;
    use std::task::Poll;

    #[test]
    fn original_resource_close_blocks_new_work_and_retains_closed_admission() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let original = Arc::new(OriginalRegisteredExecutionLease::new(
            Arc::clone(&semaphore).try_acquire_owned().unwrap(),
        ));
        let work = original.borrow_execution().unwrap();
        assert!(original.close_resource_admission().is_err());
        drop(work);
        original.close_resource_admission().unwrap();
        assert!(original.borrow_execution().is_err());
        original.require_active().unwrap();
        // A dropped checkpoint observer cannot reopen admission on reentry.
        original.close_resource_admission().unwrap();
        assert!(original.borrow_execution().is_err());
        assert!(Arc::clone(&semaphore).try_acquire_owned().is_err());
        original.release_after_acknowledgment().unwrap();
        assert!(original.require_active().is_err());
        assert!(semaphore.try_acquire_owned().is_ok());
    }

    /// Primitive exclusion/Drop evidence only; real negative receipt validation
    /// remains in the producer's release_negative_execution_lease boundary.
    #[tokio::test]
    async fn original_execution_work_borrow_blocks_release_and_outer_drop_preserves_permit() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::clone(&semaphore).try_acquire_owned().unwrap();
        let original = Arc::new(OriginalRegisteredExecutionLease::new(permit));
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (_finish, finished) = tokio::sync::oneshot::channel::<()>();
        let retained = Arc::clone(&original);
        let mut actual_work = Box::pin(async move {
            let _actual_borrow = retained.borrow_execution().unwrap();
            entered.send(()).unwrap();
            let _ = finished.await;
        });
        let waker = futures::task::noop_waker();
        let mut context = std::task::Context::from_waker(&waker);
        assert!(matches!(
            actual_work.as_mut().poll(&mut context),
            Poll::Pending
        ));
        ready.await.unwrap();
        let source = original.release_after_acknowledgment().unwrap_err();
        let AuraError::PermissionDenied {
            source: Some(source),
            ..
        } = source
        else {
            panic!("actual active work must refuse permit release with its concrete source");
        };
        assert_eq!(
            source
                .downcast_ref::<OriginalExecutionStillBorrowed>()
                .unwrap()
                .0,
            1
        );
        assert!(matches!(
            Arc::clone(&semaphore).try_acquire_owned(),
            Err(tokio::sync::TryAcquireError::NoPermits)
        ));
        drop(actual_work);
        assert!(
            matches!(
                Arc::clone(&semaphore).try_acquire_owned(),
                Err(tokio::sync::TryAcquireError::NoPermits)
            ),
            "outer Drop releases only its work borrow"
        );
        original.release_after_acknowledgment().unwrap();
        let next_actual_permit = Arc::clone(&semaphore)
            .try_acquire_owned()
            .expect("the same canonical semaphore reopens after release");
        assert!(original.require_active().is_err());
        let source = match original.borrow_execution() {
            Ok(_) => panic!("retained original lease must never regain execution eligibility"),
            Err(source) => source,
        };
        let AuraError::PermissionDenied {
            source: Some(source),
            ..
        } = source
        else {
            panic!("released execution must retain its concrete terminal source");
        };
        assert!(source
            .downcast_ref::<OriginalExecutionLeaseReleased>()
            .is_some());
        drop(next_actual_permit);
    }
}
/// Test observation only: actual original VM and bytes after real send completion.
#[cfg(test)]
pub(crate) struct OriginalCancelledNoticeDeliveryObservation {
    pub(crate) session: uuid::Uuid,
    pub(crate) vm: telltale_machine::SessionId,
    pub(crate) bytes: Vec<u8>,
    pub(crate) first_failure: Option<AuraError>,
}
#[cfg(test)]
struct OriginalCancelledNoticeDeliveryGate {
    reached: tokio::sync::oneshot::Sender<OriginalCancelledNoticeDeliveryObservation>,
    resume: tokio::sync::oneshot::Receiver<()>,
}
#[cfg(test)]
impl OriginalCancellationTaskRegistration {
    /// Install on the original registration before its actual custody admission.
    /// The channel observes completion; it cannot authorize or replace any owner.
    pub(crate) fn observe_notice_delivery_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<OriginalCancelledNoticeDeliveryObservation>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let tracker = self.tracker.upgrade()?;
        let state = tracker
            .shared
            .state
            .try_read()
            .map_err(|source| AuraError::Internal {
                message: "install original delivery observation before admission".into(),
                source: Some(Arc::new(source)),
            })?;
        let entry = state
            .ceremonies
            .get(&self.ceremony)
            .ok_or_else(|| original_tracker_refusal(OriginalEnrollmentTrackerError::OwnerLost))?;
        let admitted = match self.role {
            OriginalCancellationTaskRole::Publication => entry.cancellation_task.is_some(),
            OriginalCancellationTaskRole::Notice => entry.cancelled_notice_task.is_some(),
        };
        if admitted || !Arc::ptr_eq(&entry.original_entry, &self.original_entry) {
            return Err(AuraError::permission_denied(
                "delivery gate requires original preadmission entry",
            ));
        }
        let mut gate = self.delivery_gate.lock();
        if gate.is_some() {
            return Err(AuraError::invalid(
                "original delivery gate already installed",
            ));
        }
        let (reached, observation) = tokio::sync::oneshot::channel();
        let (resume, resumed) = tokio::sync::oneshot::channel();
        *gate = Some(OriginalCancelledNoticeDeliveryGate {
            reached,
            resume: resumed,
        });
        Ok((observation, resume))
    }
    pub(crate) async fn notice_delivered_for_test(
        &self,
        session: &crate::runtime::session_ingress::OwnedVmSession,
        bytes: &[u8],
        first_failure: Option<AuraError>,
    ) {
        // Release the synchronous lock before the observation callback or await.
        let gate = self.delivery_gate.lock().take();
        if let Some(gate) = gate {
            let _ = gate
                .reached
                .send(OriginalCancelledNoticeDeliveryObservation {
                    session: session.owner().session_id.as_uuid(),
                    vm: session.vm_session_id(),
                    bytes: bytes.to_vec(),
                    first_failure,
                });
            let _ = gate.resume.await;
        }
    }
}

/// Resource closure only: this cannot acknowledge a domain terminal frame.
pub(super) struct OriginalIssuerResourceClosure {
    original: std::sync::Weak<RegisteredEnrollmentWindowCapability>,
}

#[cfg(test)]
impl RegisteredEnrollmentWindowCapability {
    pub(crate) fn observe_original_running_owner_for_test(
        self: &Arc<Self>,
        effects: &AuraEffectSystem,
        tracker: &CeremonyTracker,
    ) -> Result<OriginalIssuerDeliveryObservation, AuraError> {
        self.require_effects(effects)?;
        let original_tracker = self.tracker.upgrade()?;
        if !Arc::ptr_eq(&original_tracker.shared, &tracker.shared) {
            return Err(original_tracker_refusal(
                OriginalEnrollmentTrackerError::OwnerLost,
            ));
        }
        self.observe_original_delivery()
    }
}
#[cfg(test)]
pub(crate) fn assert_original_running_owner_refusal_for_test(
    source: &AuraError,
    foreign_provider: bool,
) {
    let actual = std::error::Error::source(source)
        .and_then(|source| source.downcast_ref::<OriginalEnrollmentTrackerError>())
        .expect("actual original runtime/tracker refusal cause");
    assert!(if foreign_provider {
        matches!(actual, OriginalEnrollmentTrackerError::ForeignProvider)
    } else {
        matches!(actual, OriginalEnrollmentTrackerError::OwnerLost)
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod retained_issuer_control_tests {
    use super::*;
    use aura_core::effects::task::{RequiredTaskCustody, RequiredTaskExit, RequiredTaskSettlement};
    use futures::FutureExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct ActualRequiredIssuer {
        execution: Option<tokio::sync::oneshot::Receiver<()>>,
        cleanup: Option<tokio::sync::oneshot::Receiver<()>>,
        cleanup_entered: Option<tokio::sync::oneshot::Sender<()>>,
        attempts: Arc<AtomicUsize>,
        identity: Option<crate::task_registry::OwnedRuntimeTaskIdentity>,
        executor_task: Option<tokio::task::Id>,
        resource: Option<Arc<()>>,
        execution_failure: Arc<std::io::Error>,
        cleanup_refusal: Arc<std::io::Error>,
        terminal_failure: Arc<std::io::Error>,
    }
    impl RequiredTaskCustody for ActualRequiredIssuer {
        fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async {
                self.identity = crate::task_registry::current_owned_runtime_task();
                assert!(self.identity.is_some(), "actual registered original owner");
                self.executor_task =
                    Some(tokio::task::try_id().expect("actual original native executor"));
                self.execution
                    .as_mut()
                    .expect("actual execution gate")
                    .await
                    .expect("release actual original execution");
                self.execution = None;
                Err(AuraError::Internal {
                    message: "actual original issuer execution failure".into(),
                    source: Some(self.execution_failure.clone()),
                })
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            _exit: &'a RequiredTaskExit,
        ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async {
                assert_eq!(
                    crate::task_registry::current_owned_runtime_task(),
                    self.identity
                );
                assert_eq!(tokio::task::try_id(), self.executor_task);
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(AuraError::Internal {
                        message: "actual original issuer first cleanup refusal".into(),
                        source: Some(self.cleanup_refusal.clone()),
                    });
                }
                if let Some(entered) = self.cleanup_entered.take() {
                    entered
                        .send(())
                        .expect("acknowledge actual original cleanup pending");
                }
                if let Some(gate) = self.cleanup.as_mut() {
                    gate.await.expect("release same retained cleanup gate");
                    self.cleanup = None;
                }
                assert_eq!(
                    crate::task_registry::current_owned_runtime_task(),
                    self.identity
                );
                assert_eq!(tokio::task::try_id(), self.executor_task);
                drop(self.resource.take());
                Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                    AuraError::Internal {
                        message: "actual negative issuer outcome".into(),
                        source: Some(self.terminal_failure.clone()),
                    },
                ))
            })
        }
    }
    #[tokio::test]
    async fn cancelled_issuer_reconciliation_retains_same_control_until_actual_retry_ack() {
        let supervisor = crate::task_registry::TaskSupervisor::new();
        let group = supervisor.group("retained-issuer-reconciliation");
        let (execute, execution) = tokio::sync::oneshot::channel();
        let (release, cleanup) = tokio::sync::oneshot::channel();
        let (cleanup_started, cleanup_entered) = tokio::sync::oneshot::channel();
        let attempts = Arc::new(AtomicUsize::new(0));
        let resource = Arc::new(());
        let observed_resource = Arc::downgrade(&resource);
        let execution_failure = Arc::new(std::io::Error::other("actual execution native cause"));
        let cleanup_refusal = Arc::new(std::io::Error::other("actual cleanup native cause"));
        let terminal_failure = Arc::new(std::io::Error::other("actual negative terminal cause"));
        let original = group
            .spawn_original_required_custody_with_resident_cleanup(
                "same-original-issuer".into(),
                Box::new(ActualRequiredIssuer {
                    execution: Some(execution),
                    cleanup: Some(cleanup),
                    cleanup_entered: Some(cleanup_started),
                    attempts: Arc::clone(&attempts),
                    identity: None,
                    executor_task: None,
                    resource: Some(resource),
                    execution_failure: execution_failure.clone(),
                    cleanup_refusal: cleanup_refusal.clone(),
                    terminal_failure: terminal_failure.clone(),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual original required admission"));
        let holder = RetainedOriginalIssuerTask {
            control: tokio::sync::Mutex::new(original),
        };
        let observer = holder
            .disposal_observer()
            .expect("actual original disposal observer");
        assert!(
            holder
                .reconcile_original_disposal()
                .now_or_never()
                .is_none(),
            "live first attempt has no retry disposition"
        );
        execute.send(()).expect("release actual first execution");
        holder
            .control
            .lock()
            .await
            .await_original_attempt_disposition()
            .await
            .expect("actual failed first cleanup yielded original custody");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        let first_refusal = holder
            .control
            .lock()
            .await
            .await_original_cleanup_decision()
            .await
            .expect_err("actual first refused cleanup diagnostic before retry");
        let diagnostic = std::error::Error::source(&first_refusal)
            .and_then(|source| {
                source.downcast_ref::<crate::task_registry::OriginalRequiredCleanupDiagnostic>()
            })
            .expect("canonical actual first cleanup diagnostic");

        let mut retry = Box::pin(holder.reconcile_original_disposal());
        assert!(
            futures::poll!(retry.as_mut()).is_pending(),
            "actual retry waits on retained domain cleanup gate"
        );
        cleanup_entered
            .await
            .expect("original executor entered actual pending cleanup");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        drop(retry);
        assert!(
            holder.control.try_lock().is_ok(),
            "dropped retry returns exact control to retained slot"
        );
        assert!(
            observer.await_disposal().now_or_never().is_none(),
            "dropping retry cannot issue resource ACK"
        );
        assert!(
            observed_resource.upgrade().is_some(),
            "dropping observer retains actual original resource until cleanup ACK"
        );
        release
            .send(())
            .expect("same domain gate survives dropped retry");
        let settlement = holder
            .reconcile_original_disposal()
            .await
            .expect("same retained control completes actual original cleanup");
        holder
            .require_disposal(&settlement.disposal)
            .expect("actual physical API retains exact original disposition");
        assert!(
            observed_resource.upgrade().is_none(),
            "actual original resource retired before physical API returned disposal"
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            2,
            "abandoned observer cannot restart the pending original cleanup future"
        );
        let retained_failure = settlement
            .cleanup_failure()
            .expect("actual failed issuer domain remains distinct from resource disposal");
        let terminal = std::error::Error::source(&retained_failure)
            .and_then(|source| {
                source.downcast_ref::<aura_core::effects::task::RequiredTaskTerminalFailure>()
            })
            .expect("actual refined execution and terminal domain outcome");
        let RequiredTaskExit::Failed(execution) = terminal.execution() else {
            panic!("actual failed original execution remains failed after cleanup ACK");
        };
        let original_execution = std::error::Error::source(execution)
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .expect("original execution native source");
        assert!(std::ptr::eq(original_execution, execution_failure.as_ref()));
        let original_terminal = std::error::Error::source(terminal.terminal())
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .expect("independent actual terminal native source");
        assert!(std::ptr::eq(original_terminal, terminal_failure.as_ref()));
        assert!(
            settlement.independent_failures().is_empty(),
            "successful physical retry adds no new refusal observation"
        );
        assert_eq!(
            diagnostic.cleanup_failures().count(),
            1,
            "one actual refused attempt remains recorded after same-future successful disposal"
        );
        let refusal = diagnostic
            .cleanup_failures()
            .next()
            .expect("actual original refusal");
        let original_refusal = std::error::Error::source(refusal.cleanup_failure())
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .expect("retained independent first cleanup native cause");
        assert!(std::ptr::eq(original_refusal, cleanup_refusal.as_ref()));
        let disposal = observer
            .await_disposal()
            .now_or_never()
            .expect("actual completed retry publishes its exact disposition before returning");
        let repeated = observer
            .await_disposal()
            .now_or_never()
            .expect("actual acknowledged disposition remains immutable and observable");
        holder
            .require_disposal(&repeated)
            .expect("repeated observation belongs to exact same control");
        holder
            .require_disposal(&disposal)
            .expect("ACK belongs to exact retained original control");
        assert!(
            matches!(disposal.outcome(), RequiredTaskExit::Failed(_)),
            "negative original execution remains negative after resource ACK"
        );
    }
}

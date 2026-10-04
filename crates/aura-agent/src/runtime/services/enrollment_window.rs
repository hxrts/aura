//! Sealed durable clock owner for enrollment execution.
use super::ceremony_tracker::RegisteredEnrollmentWindowCapability;
use crate::handlers::invitation::enrollment_manifest_admission::{
    AdmittedEnrollmentManifest, NewEnrollmentAdmissionCapability,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{
    PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::time::PhysicalTime;
use aura_core::{AuraError, TimeoutBudget, TimeoutBudgetError, TimeoutRunError};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit};

/// Preparation authority is separate from protocol execution admission.
pub(crate) enum EnrollmentCancellationPreparationCapability {
    Active(EnrollmentCancellationWindowCapability),
    Decided(super::ceremony_tracker::VerifiedEnrollmentCancellationCapability),
}
pub(crate) struct EnrollmentCancellationWindowCapability {
    active: TimeoutBudget,
    owner: Arc<super::ceremony_tracker::CancellationClockObservationCapability>,
}
impl EnrollmentCancellationWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_cancellation_window",
        capability_type = CancellationClockObservationCapability,
        family = "runtime_helper"
    )]
    pub(super) async fn registered(
        capability: super::ceremony_tracker::CancellationClockObservationCapability,
        effects: &AuraEffectSystem,
    ) -> Result<EnrollmentCancellationWindowCapability, AuraError> {
        capability.require_effects(effects)?;
        let owner = Arc::new(capability);
        let _observation = owner.budget().acquire_observation().await;
        let now = effects
            .physical_time()
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        let active = owner.budget().remaining_at(&now);
        owner
            .checkpoint()
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        active?;
        let active = owner.budget().child_budget(
            &now,
            signed_notice_validity_remaining(&now, owner.signed_expiry_ms())?,
        )?;
        drop(_observation);
        Ok(Self { active, owner })
    }
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<T, TimeoutRunError<crate::core::AgentError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = crate::core::AgentResult<T>>,
    {
        self.owner.require_effects(effects).map_err(|source| {
            TimeoutRunError::Timeout(TimeoutBudgetError::checkpoint_failure(source))
        })?;
        aura_core::time::timeout::execute_with_timeout_budget_and_checkpoint(
            effects,
            &self.active,
            || async {
                self.owner
                    .checkpoint()
                    .await
                    .map_err(TimeoutBudgetError::checkpoint_failure)
            },
            operation,
        )
        .await
    }
    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        map_enrollment_run_error(stage, &self.active, source)
    }
}

/// No-send is an expected terminal recovery disposition, not service failure.
/// It carries the concrete eligibility cause without reopening admission.
pub(crate) enum CancelledNoticeWindowAdmission {
    Eligible(CancelledEnrollmentNoticeWindowCapability),
    EligibilityEnded { cause: AuraError },
}
pub(crate) fn cancelled_notice_eligibility_ended(
    error: &(impl std::error::Error + 'static),
) -> bool {
    let mut next: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(source) = next {
        if source.is::<crate::handlers::invitation::EnrollmentVmTeardownFailure>() {
            return false;
        }
        if let Some(budget) = source.downcast_ref::<TimeoutBudgetError>() {
            // Required clock/checkpoint/ownership failures cannot be normalized
            // into ordinary expiry by inspecting a nested diagnostic cause.
            return match budget {
                TimeoutBudgetError::DeadlineExceeded { .. } => true,
                TimeoutBudgetError::ClockRollback { .. }
                | TimeoutBudgetError::ObservationUnavailable
                | TimeoutBudgetError::CheckpointDiscontinuity { .. }
                | TimeoutBudgetError::CheckpointFailure { .. }
                | TimeoutBudgetError::InvalidPolicy { .. }
                | TimeoutBudgetError::TimeSourceUnavailable { .. }
                | TimeoutBudgetError::AttemptBudgetExhausted { .. } => false,
            };
        }
        if let Some(
            AuraError::Storage { .. }
            | AuraError::Serialization { .. }
            | AuraError::Crypto { .. }
            | AuraError::PermissionDenied { .. },
        ) = source.downcast_ref::<AuraError>()
        {
            return false;
        }
        if matches!(
            source.downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>(),
            Some(aura_invitation::enrollment_manifest::EnrollmentManifestError::Expired)
        ) {
            return true;
        }
        next = source.source();
    }
    false
}
/// Finite negative notice custody recovered from an original Cancelled decision.
/// It cannot be used as an active enrollment execution/admission window.
pub(crate) struct CancelledEnrollmentNoticeWindowCapability {
    window: EnrollmentCancellationWindowCapability,
    _lease: Arc<OwnedSemaphorePermit>,
    cancelled: super::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    manifest_digest: [u8; 32],
}
impl CancelledEnrollmentNoticeWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeWindowCapability",
        family = "runtime_helper"
    )]
    pub(super) async fn registered(
        capability: super::ceremony_tracker::RegisteredCancelledNoticeCapability,
        effects: &AuraEffectSystem,
    ) -> Result<CancelledEnrollmentNoticeWindowCapability, AuraError> {
        let (observation, lease, cancelled, manifest_digest) = capability.into_parts();
        let window =
            EnrollmentCancellationWindowCapability::registered(observation, effects).await?;
        Ok(Self {
            window,
            _lease: lease,
            cancelled,
            manifest_digest,
        })
    }
    pub(crate) fn require_issued_owner(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.window.owner.require_effects(effects)?;
        issued.require_runtime_owner(effects)?;
        if self.manifest_digest != issued.digest()
            || self.cancelled.invitation() != &issued.manifest().invitation
            || self.cancelled.ceremony() != &issued.manifest().ceremony
            || self.window.owner.signed_expiry_ms() != issued.manifest().expires_at_ms
        {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "cancelled notice has another original issuer binding".into(),
                },
            ));
        }
        Ok(())
    }
    pub(crate) fn cancelled(
        &self,
    ) -> &super::ceremony_tracker::VerifiedEnrollmentCancellationCapability {
        &self.cancelled
    }
    pub(crate) async fn remaining_ms(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<u64, TimeoutBudgetError> {
        self.window
            .owner
            .require_effects(effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let _observation = self.window.active.acquire_observation().await;
        let now = effects
            .physical_time()
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        let remaining = self.window.active.remaining_at(&now);
        self.window
            .owner
            .checkpoint()
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        u64::try_from(remaining?.as_millis())
            .map_err(|error| TimeoutBudgetError::invalid_policy(error.to_string()))
    }
    pub(crate) async fn retry_delay(
        &self,
        effects: &AuraEffectSystem,
        delay_ms: u64,
    ) -> Result<(), TimeoutBudgetError> {
        let remaining = self.remaining_ms(effects).await?;
        effects
            .sleep_ms(delay_ms.min(remaining))
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        self.remaining_ms(effects).await?;
        Ok(())
    }
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<T, TimeoutRunError<crate::core::AgentError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = crate::core::AgentResult<T>>,
    {
        self.window.execute(effects, operation).await
    }
    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        self.window.map_run_error(stage, source)
    }
}

/// A child retains both the original persisted window and its admission lease.
/// There is no raw-budget extraction or caller-supplied checkpoint constructor.
#[derive(Clone)]
pub(crate) struct EnrollmentWindowCapability {
    active: TimeoutBudget,
    original: TimeoutBudget,
    checkpoint: Arc<WindowCheckpoint>,
    _lease: Arc<OwnedSemaphorePermit>,
}
/// Move-only acknowledgment of an actual persisted live invitee window.
/// Frozen bytes do not share mutable clock state with subsequent observations.
pub(crate) struct AcknowledgedEnrollmentWindow {
    binding: AdmittedWindowBinding,
    frozen_budget: Vec<u8>,
    acknowledged_at_ms: u64,
}
impl AcknowledgedEnrollmentWindow {
    pub(crate) fn frozen_budget_bytes(&self) -> &[u8] {
        &self.frozen_budget
    }
    pub(crate) fn manifest_digest(&self) -> [u8; 32] {
        self.binding.manifest_digest
    }
    pub(crate) fn ceremony(&self) -> &aura_core::CeremonyId {
        &self.binding.ceremony
    }
    pub(crate) fn invitation(&self) -> &aura_core::InvitationId {
        &self.binding.invitation
    }
    pub(crate) fn device(&self) -> aura_core::DeviceId {
        self.binding.device
    }
    pub(crate) fn acknowledged_at_ms(&self) -> u64 {
        self.acknowledged_at_ms
    }
}
struct FrozenAdmittedCheckpoint {
    binding: AdmittedWindowBinding,
    budget_bytes: Vec<u8>,
}
enum WindowCheckpoint {
    Registered {
        capability: Arc<RegisteredEnrollmentWindowCapability>,
    },
    Admitted {
        effects: Arc<AuraEffectSystem>,
        binding: AdmittedWindowBinding,
        writes: Mutex<()>,
    },
}
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AdmittedWindowBinding {
    ceremony: aura_core::CeremonyId,
    invitation: aura_core::InvitationId,
    manifest_digest: [u8; 32],
    device: aura_core::DeviceId,
    expires_at_ms: u64,
    admitted_at_ms: u64,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAdmittedWindow {
    binding: AdmittedWindowBinding,
    budget: TimeoutBudget,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAdmittedAnchor {
    version: u16,
    legacy_v1: bool,
    window: StoredAdmittedWindow,
}
fn admitted_location(namespace: &str, binding: &AdmittedWindowBinding) -> SecureStorageLocation {
    SecureStorageLocation::new(namespace, binding.ceremony.to_string())
}
impl EnrollmentWindowCapability {
    fn admitted_binding(witness: &AdmittedEnrollmentManifest) -> AdmittedWindowBinding {
        let manifest = witness.manifest();
        AdmittedWindowBinding {
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            manifest_digest: witness.manifest_digest(),
            device: manifest.invitee_device,
            expires_at_ms: manifest.expires_at_ms,
            admitted_at_ms: witness.admitted_at_ms(),
        }
    }
    fn validate_admitted_record(
        record: &StoredAdmittedWindow,
        binding: &AdmittedWindowBinding,
        legacy: bool,
    ) -> Result<(), AuraError> {
        let full = binding
            .expires_at_ms
            .checked_sub(binding.admitted_at_ms)
            .ok_or_else(|| AuraError::invalid("invalid original admitted interval"))?;
        let allowance = if legacy { full.min(240_000) } else { full };
        if record.binding != *binding
            || record.budget.started_at_ms() != binding.admitted_at_ms
            || record.budget.timeout_ms() != allowance
        {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "admitted clock differs from protected original interval".into(),
                },
            ));
        }
        Ok(())
    }
    async fn decode_admitted_record(
        effects: &AuraEffectSystem,
        key: &SecureStorageLocation,
    ) -> Result<StoredAdmittedWindow, AuraError> {
        let bytes = effects
            .secure_retrieve(key, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized admitted clock"));
        }
        serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
            message: "decode original admitted clock".into(),
            source: Some(Arc::new(source)),
        })
    }
    async fn read_admitted_anchor(
        effects: &AuraEffectSystem,
        binding: &AdmittedWindowBinding,
    ) -> Result<StoredAdmittedWindow, AuraError> {
        let key = admitted_location("admitted_enrollment_clock_anchor_v2", binding);
        if effects
            .secure_exists(&admitted_location(
                "admitted_enrollment_clock_ever_live_v2",
                binding,
            ))
            .await?
        {
            // Every reader preserves the required v2 missing-record cause after
            // live admission; no frozen receipt reader can select v1 instead.
            effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await?;
        }
        if !effects.secure_exists(&key).await? {
            let legacy = Self::decode_admitted_record(
                effects,
                &admitted_location("admitted_enrollment_clock_v1", binding),
            )
            .await?;
            Self::validate_admitted_record(&legacy, binding, true)?;
            return Ok(legacy);
        }
        let bytes = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized original admitted anchor"));
        }
        let anchor: StoredAdmittedAnchor =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode protected admitted anchor".into(),
                source: Some(Arc::new(source)),
            })?;
        if anchor.version != 2 {
            return Err(AuraError::invalid("unsupported admitted anchor"));
        }
        Self::validate_admitted_record(&anchor.window, binding, anchor.legacy_v1)?;
        Ok(anchor.window)
    }
    async fn publish_admitted_anchor(
        effects: &AuraEffectSystem,
        record: &StoredAdmittedWindow,
        legacy: bool,
    ) -> Result<(), AuraError> {
        let anchor = StoredAdmittedAnchor {
            version: 2,
            legacy_v1: legacy,
            window: serde_json::from_slice(
                &serde_json::to_vec(record).map_err(TimeoutBudgetError::checkpoint_failure)?,
            )
            .map_err(TimeoutBudgetError::checkpoint_failure)?,
        };
        let bytes = serde_json::to_vec(&anchor).map_err(TimeoutBudgetError::checkpoint_failure)?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized admitted anchor publication"));
        }
        let key = admitted_location("admitted_enrollment_clock_anchor_v2", &record.binding);
        effects
            .secure_store_immutable(
                &key,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        let retained = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if retained != bytes {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "original admitted anchor already differs".into(),
                },
            ));
        }
        let checkpoint =
            serde_json::to_vec(record).map_err(TimeoutBudgetError::checkpoint_failure)?;
        effects
            .secure_create_mutable(
                &admitted_location("admitted_enrollment_clock_checkpoint_v2", &record.binding),
                &checkpoint,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        let retained_checkpoint = Self::decode_admitted_record(
            effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &record.binding),
        )
        .await?;
        if retained_checkpoint.binding != record.binding {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "initial admitted checkpoint has another owner".into(),
                },
            ));
        }
        retained_checkpoint
            .budget
            .validate_checkpoint_continuation_from(&record.budget)
            .map_err(AuraError::from)?;
        Ok(())
    }

    async fn require_original_admission_publication(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
        lease: &crate::runtime::effects::AdmittedEnrollmentWindowLeaseCapability<'_>,
    ) -> Result<(), AuraError> {
        lease.require_effects(effects)?;
        let original =
            crate::handlers::invitation::enrollment_manifest_admission::load_admitted_baseline(
                effects,
                effects.runtime_authority_id(),
                witness.canonical_invitation(),
            )
            .await
            .map_err(|source| AuraError::Internal {
                message: "reverify protected initial admission publication".into(),
                source: Some(Arc::new(source)),
            })?;
        if original.manifest_digest() != witness.manifest_digest()
            || original.admitted_at_ms() != witness.admitted_at_ms()
        {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "initial publication belongs to another protected admission".into(),
                },
            ));
        }
        Ok(())
    }
    async fn require_live_admitted_publication(
        effects: &AuraEffectSystem,
        binding: &AdmittedWindowBinding,
    ) -> Result<bool, AuraError> {
        let live = admitted_location("admitted_enrollment_clock_ever_live_v2", binding);
        if !effects.secure_exists(&live).await? {
            return Ok(false);
        }
        let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", binding);
        let checkpoint_key = admitted_location("admitted_enrollment_clock_checkpoint_v2", binding);
        // A prior live decision forbids repairing a lost immutable anchor.
        effects
            .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
            .await?;
        let anchor = Self::read_admitted_anchor(effects, binding).await?;
        let expected =
            serde_json::to_vec(&anchor).map_err(TimeoutBudgetError::checkpoint_failure)?;
        let retained = effects
            .secure_retrieve(&live, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 16_384 || retained != expected {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "admitted ever-live evidence differs from original publication".into(),
                },
            ));
        }
        // Required read; absence after prior admission must never initialize.
        let checkpoint = Self::decode_admitted_record(effects, &checkpoint_key).await?;
        if checkpoint.binding != *binding {
            return Err(AuraError::invalid("ever-live checkpoint binding differs"));
        }
        checkpoint
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        Ok(true)
    }
    /// Restore only the original protected publication while its actual execution
    /// lease is held. Required reimport reads never call this completion owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "AdmittedEnrollmentWindowLeaseCapability",
        family = "runtime_helper"
    )]
    async fn finish_original_initial_publication(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
        lease: &crate::runtime::effects::AdmittedEnrollmentWindowLeaseCapability<'_>,
    ) -> Result<(), AuraError> {
        Self::require_original_admission_publication(effects, witness, lease).await?;
        let binding = Self::admitted_binding(witness);
        let live = admitted_location("admitted_enrollment_clock_ever_live_v2", &binding);
        let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", &binding);
        let checkpoint_key = admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding);
        if Self::require_live_admitted_publication(effects, &binding).await? {
            return Ok(());
        }
        let anchor = if effects.secure_exists(&anchor_key).await? {
            Self::read_admitted_anchor(effects, &binding).await?
        } else {
            let legacy_key = admitted_location("admitted_enrollment_clock_v1", &binding);
            if effects.secure_exists(&legacy_key).await? {
                let legacy = Self::decode_admitted_record(effects, &legacy_key).await?;
                Self::validate_admitted_record(&legacy, &binding, true)?;
                Self::publish_admitted_anchor(effects, &legacy, true).await?;
                legacy
            } else {
                // The witness was reverified from the actual immutable original
                // admission record; its timestamp is never supplied by a caller.
                let allowance = binding
                    .expires_at_ms
                    .checked_sub(binding.admitted_at_ms)
                    .ok_or_else(|| AuraError::invalid("invalid original admission publication"))?;
                let budget = TimeoutBudget::from_start_and_timeout(
                    &PhysicalTime::exact(binding.admitted_at_ms),
                    Duration::from_millis(allowance),
                )?;
                let original = StoredAdmittedWindow {
                    binding: binding.clone(),
                    budget,
                };
                Self::publish_admitted_anchor(effects, &original, false).await?;
                original
            }
        };
        if !effects.secure_exists(&checkpoint_key).await? {
            let bytes =
                serde_json::to_vec(&anchor).map_err(TimeoutBudgetError::checkpoint_failure)?;
            effects
                .secure_create_mutable(
                    &checkpoint_key,
                    &bytes,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await?;
        }
        let checkpoint = Self::decode_admitted_record(effects, &checkpoint_key).await?;
        if checkpoint.binding != binding {
            return Err(AuraError::invalid("initial checkpoint binding differs"));
        }
        checkpoint
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        // Record first admission before execution. Once this is ACKed, neither
        // startup nor reimport can complete missing clock records as initial.
        let bytes = serde_json::to_vec(&anchor).map_err(TimeoutBudgetError::checkpoint_failure)?;
        effects
            .secure_store_immutable(
                &live,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        let retained = effects
            .secure_retrieve(&live, &[SecureStorageCapability::Read])
            .await?;
        if retained != bytes {
            return Err(AuraError::invalid("admitted first-live decision differs"));
        }
        Ok(())
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "registered_enrollment_window",
        capability_type = RegisteredEnrollmentWindowCapability,
        family = "runtime_helper"
    )]
    pub(super) async fn registered(
        capability: RegisteredEnrollmentWindowCapability,
    ) -> Result<Self, AuraError> {
        let capability = Arc::new(capability);
        capability.checkpoint().await?;
        Ok(Self {
            active: capability.budget().clone(),
            original: capability.budget().clone(),
            _lease: capability.lease(),
            checkpoint: Arc::new(WindowCheckpoint::Registered { capability }),
        })
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "admitted_enrollment_window",
        capability_type = AdmittedEnrollmentManifest,
        family = "runtime_helper"
    )]
    pub(crate) async fn admitted(
        effects: Arc<AuraEffectSystem>,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<Self, AuraError> {
        let lease_owner = effects.acquire_admitted_enrollment_window_owner(witness)?;
        Self::finish_original_initial_publication(&effects, witness, &lease_owner).await?;
        let lease = lease_owner.into_permit();
        let manifest = witness.manifest();
        let binding = AdmittedWindowBinding {
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            manifest_digest: witness.manifest_digest(),
            device: manifest.invitee_device,
            expires_at_ms: manifest.expires_at_ms,
            admitted_at_ms: witness.admitted_at_ms(),
        };
        let anchor = Self::read_admitted_anchor(&effects, &binding).await?;
        let record = Self::decode_admitted_record(
            &effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding),
        )
        .await?;
        record
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        if record.binding != binding {
            return Err(AuraError::invalid(
                "admitted checkpoint belongs to another owner",
            ));
        }
        let budget = record.budget;
        let owner = Self {
            active: budget.clone(),
            original: budget,
            checkpoint: Arc::new(WindowCheckpoint::Admitted {
                effects,
                binding,
                writes: Mutex::new(()),
            }),
            _lease: Arc::new(lease),
        };
        owner.checkpoint().await.map_err(AuraError::from)?;
        Ok(owner)
    }
    /// Called only after a new explicit user-transfer admission is committed.
    /// Reimporting an existing admission never allocates a fresh clock owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "retain_new_admitted_window_enrollment_window",
        capability_type = NewEnrollmentAdmissionCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn retain_new_admitted_window(
        effects: &AuraEffectSystem,
        admission: NewEnrollmentAdmissionCapability<'_>,
    ) -> Result<(), AuraError> {
        let witness = admission.witness();
        let manifest = witness.manifest();
        let binding = AdmittedWindowBinding {
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            manifest_digest: witness.manifest_digest(),
            device: manifest.invitee_device,
            expires_at_ms: manifest.expires_at_ms,
            admitted_at_ms: witness.admitted_at_ms(),
        };
        let allowance = binding
            .expires_at_ms
            .checked_sub(binding.admitted_at_ms)
            .ok_or_else(|| AuraError::invalid("invalid initial admitted window"))?;
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(binding.admitted_at_ms),
            Duration::from_millis(allowance),
        )
        .map_err(AuraError::from)?;
        let record = StoredAdmittedWindow { binding, budget };
        Self::publish_admitted_anchor(effects, &record, false).await
    }
    pub(crate) async fn require_retained_admitted_window(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<(), AuraError> {
        let binding = Self::admitted_binding(witness);
        let anchor = Self::read_admitted_anchor(effects, &binding).await?;
        let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", &binding);
        if effects.secure_exists(&anchor_key).await? {
            let checkpoint = Self::decode_admitted_record(
                effects,
                &admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding),
            )
            .await?;
            if checkpoint.binding != binding {
                return Err(AuraError::invalid("retained checkpoint binding differs"));
            }
            checkpoint
                .budget
                .validate_checkpoint_continuation_from(&anchor.budget)
                .map_err(AuraError::from)?;
        }
        Ok(())
    }
    pub(crate) async fn acknowledge_confirmation<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        if !matches!(self.checkpoint.as_ref(), WindowCheckpoint::Admitted { .. }) {
            return Err(AuraError::invalid(
                "confirmation acknowledgment requires the actual admitted invitee owner",
            ));
        }
        let (now, _observation) = self.observe(effects).await.map_err(AuraError::from)?;
        let remaining = self.active.remaining_at(&now);
        if let Err(source) = remaining {
            self.checkpoint().await.map_err(AuraError::from)?;
            return Err(AuraError::from(source));
        }
        let acknowledged = self
            .checkpoint_acknowledged(Some(&now))
            .await
            .map_err(AuraError::from)?
            .ok_or_else(|| {
                AuraError::invalid("confirmation requires an admitted checkpoint acknowledgment")
            })?;

        let frozen: TimeoutBudget =
            serde_json::from_slice(&acknowledged.budget_bytes).map_err(|source| {
                AuraError::Internal {
                    message: "validate exact acknowledged enrollment snapshot".into(),
                    source: Some(Arc::new(source)),
                }
            })?;
        frozen
            .validate_recorded_observation_at(&now)
            .map_err(AuraError::from)?;
        Ok(AcknowledgedEnrollmentWindow {
            binding: acknowledged.binding,
            frozen_budget: acknowledged.budget_bytes,
            acknowledged_at_ms: now.ts_ms,
        })
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "verified_enrollment_failure_ack",
        capability_type = VerifiedEnrollmentFailureCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_failure<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
        failure: &crate::handlers::invitation::VerifiedEnrollmentFailureCapability,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        let WindowCheckpoint::Admitted { binding, .. } = self.checkpoint.as_ref() else {
            return Err(AuraError::invalid(
                "failure acknowledgment requires actual admitted invitee window",
            ));
        };
        let manifest = failure.manifest();
        if binding.manifest_digest != failure.manifest_digest()
            || binding.ceremony != manifest.ceremony
            || binding.invitation != manifest.invitation
            || binding.device != manifest.invitee_device
        {
            return Err(AuraError::invalid(
                "verified failure belongs to another admitted window",
            ));
        }
        self.acknowledge_confirmation(effects).await
    }

    /// Validate original clock evidence of a separately authenticated retained
    /// receipt. Later live expiry cannot rewrite immutable completed evidence.
    pub(crate) async fn verify_retained_confirmation_clock(
        effects: &AuraEffectSystem,
        retained: &crate::handlers::invitation::enrollment_manifest_admission::RetainedConfirmationAdmission,
    ) -> Result<(), AuraError> {
        let admitted = retained.admitted();
        Self::require_retained_admitted_window(effects, admitted).await?;
        if retained.budget_bytes().len() > 16_384 {
            return Err(AuraError::invalid("oversized retained confirmation clock"));
        }
        let frozen: TimeoutBudget =
            serde_json::from_slice(retained.budget_bytes()).map_err(|source| {
                AuraError::Internal {
                    message: "decode immutable confirmation clock evidence".into(),
                    source: Some(Arc::new(source)),
                }
            })?;
        let original_start = admitted.admitted_at_ms();
        let allowance = Self::read_admitted_anchor(effects, &Self::admitted_binding(admitted))
            .await?
            .budget
            .timeout_ms();
        if frozen.started_at_ms() != original_start
            || frozen.timeout_ms() != allowance
            || retained.confirmed_at_ms() < original_start
            || retained.confirmed_at_ms() > retained.acknowledged_at_ms()
        {
            return Err(AuraError::invalid(
                "retained confirmation clock binding mismatch",
            ));
        }
        frozen
            .validate_recorded_observation_at(&PhysicalTime::exact(retained.acknowledged_at_ms()))
            .map_err(AuraError::from)?;
        if retained.confirmed_at_ms() >= frozen.deadline_at_ms() {
            return Err(AuraError::from(TimeoutBudgetError::deadline_exceeded(
                frozen.deadline_at_ms(),
                retained.confirmed_at_ms(),
            )));
        }
        Ok(())
    }
    pub(crate) async fn verify_retained_failure_clock(
        effects: &AuraEffectSystem,
        retained: &crate::handlers::invitation::enrollment_manifest_admission::RetainedFailureAdmissionCapability,
    ) -> Result<(), AuraError> {
        let admitted = retained.admitted();
        Self::require_retained_admitted_window(effects, admitted).await?;
        if retained.budget_bytes().len() > 16_384 {
            return Err(AuraError::invalid("oversized retained failure clock"));
        }
        let frozen: TimeoutBudget =
            serde_json::from_slice(retained.budget_bytes()).map_err(|source| {
                AuraError::Internal {
                    message: "decode immutable failure clock evidence".into(),
                    source: Some(Arc::new(source)),
                }
            })?;
        let original_start = admitted.admitted_at_ms();
        let allowance = Self::read_admitted_anchor(effects, &Self::admitted_binding(admitted))
            .await?
            .budget
            .timeout_ms();
        if frozen.started_at_ms() != original_start
            || frozen.timeout_ms() != allowance
            || retained.observed_at_ms() < original_start
            || retained.observed_at_ms() > retained.acknowledged_at_ms()
        {
            return Err(AuraError::invalid(
                "retained failure clock binding mismatch",
            ));
        }
        frozen
            .validate_recorded_observation_at(&PhysicalTime::exact(retained.acknowledged_at_ms()))
            .map_err(AuraError::from)?;
        if retained.observed_at_ms() >= frozen.deadline_at_ms() {
            return Err(AuraError::from(TimeoutBudgetError::deadline_exceeded(
                frozen.deadline_at_ms(),
                retained.observed_at_ms(),
            )));
        }
        Ok(())
    }
    async fn read_bound_admitted_checkpoint(
        effects: &AuraEffectSystem,
        binding: &AdmittedWindowBinding,
        original: &TimeoutBudget,
    ) -> Result<StoredAdmittedWindow, TimeoutBudgetError> {
        let bytes = effects
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "admitted_enrollment_clock_checkpoint_v2",
                    binding.ceremony.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if bytes.len() > 16_384 {
            return Err(TimeoutBudgetError::CheckpointDiscontinuity {
                detail: "oversized retained admitted checkpoint".into(),
            });
        }
        let record: StoredAdmittedWindow =
            serde_json::from_slice(&bytes).map_err(TimeoutBudgetError::checkpoint_failure)?;
        if record.binding != *binding {
            return Err(TimeoutBudgetError::CheckpointDiscontinuity {
                detail: "retained admitted checkpoint belongs to another allocation".into(),
            });
        }
        original.validate_checkpoint_continuation_from(&record.budget)?;
        Ok(record)
    }

    async fn checkpoint(&self) -> Result<(), TimeoutBudgetError> {
        self.checkpoint_acknowledged(None).await.map(|_| ())
    }
    // Only this admitted owner can request confirmation acknowledgment. The
    // observation comes from its required effect read, never a public timestamp.
    async fn checkpoint_acknowledged(
        &self,
        confirmation_at: Option<&PhysicalTime>,
    ) -> Result<Option<FrozenAdmittedCheckpoint>, TimeoutBudgetError> {
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                if confirmation_at.is_some() {
                    return Err(TimeoutBudgetError::CheckpointDiscontinuity {
                        detail: "issuer checkpoint cannot acknowledge invitee confirmation".into(),
                    });
                }
                capability
                    .checkpoint()
                    .await
                    .map_err(TimeoutBudgetError::checkpoint_failure)?;
                Ok(None)
            }
            WindowCheckpoint::Admitted {
                effects,
                binding,
                writes,
            } => {
                let _write = writes.lock().await;
                Self::read_bound_admitted_checkpoint(effects, binding, &self.original).await?;
                let record = StoredAdmittedWindow {
                    binding: binding.clone(),
                    budget: self.original.clone(),
                };
                // Freeze the original parent's snapshot once, before its write.
                // A later clone observation cannot change these detached bytes.
                let bytes =
                    serde_json::to_vec(&record).map_err(TimeoutBudgetError::checkpoint_failure)?;
                let frozen: StoredAdmittedWindow = serde_json::from_slice(&bytes)
                    .map_err(TimeoutBudgetError::checkpoint_failure)?;
                let confirmation_validation = confirmation_at.map(|now| {
                    self.active
                        .validate_recorded_observation_at(now)
                        .and_then(|()| frozen.budget.validate_recorded_observation_at(now))
                });
                let budget_bytes = serde_json::to_vec(&frozen.budget)
                    .map_err(TimeoutBudgetError::checkpoint_failure)?;
                effects
                    .secure_store(
                        &SecureStorageLocation::new(
                            "admitted_enrollment_clock_checkpoint_v2",
                            binding.ceremony.to_string(),
                        ),
                        &bytes,
                        &[SecureStorageCapability::Write],
                    )
                    .await
                    .map_err(TimeoutBudgetError::checkpoint_failure)?;
                if let Some(validation) = confirmation_validation {
                    validation?;
                }
                Ok(Some(FrozenAdmittedCheckpoint {
                    binding: frozen.binding,
                    budget_bytes,
                }))
            }
        }
    }
    async fn observe<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
    ) -> Result<
        (
            PhysicalTime,
            aura_core::time::timeout::TimeoutObservationLease,
        ),
        TimeoutBudgetError,
    > {
        let lease = self.original.acquire_observation().await;
        let now = effects
            .physical_time()
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        Ok((now, lease))
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentNoticeBindingCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn bind_registered_notice_control(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<Arc<super::ceremony_tracker::RegisteredEnrollmentNoticeBindingCapability>, AuraError>
    {
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.bind_issued_notice_control(issued)
            }
            _ => Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "notice requires original registered issuer window".into(),
                },
            )),
        }
    }
    pub(crate) fn require_issued_notice_owner(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.require_issued_notice_control(issued)
            }
            _ => Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "terminal notice has another registered original owner".into(),
                },
            )),
        }
    }
    pub(crate) fn require_admitted_notice_owner(
        &self,
        admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<(), AuraError> {
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Admitted {
                binding,
                effects: owner,
                ..
            } if Arc::ptr_eq(owner, effects)
                && binding.manifest_digest == admitted.manifest_digest()
                && binding.ceremony == admitted.manifest().ceremony
                && binding.invitation == admitted.manifest().invitation
                && binding.device == admitted.manifest().invitee_device
                && binding.admitted_at_ms == admitted.admitted_at_ms()
                && binding.expires_at_ms == admitted.manifest().expires_at_ms =>
            {
                Ok(())
            }
            _ => Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "terminal notice has another admitted original owner".into(),
                },
            )),
        }
    }

    pub(crate) async fn remaining_ms<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
    ) -> Result<u64, TimeoutBudgetError> {
        let (now, _observation) = self.observe(effects).await?;
        let remaining = self.active.remaining_at(&now);
        self.checkpoint().await?;
        u64::try_from(remaining?.as_millis())
            .map_err(|error| TimeoutBudgetError::invalid_policy(error.to_string()))
    }
    pub(crate) async fn child<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
        requested: Duration,
    ) -> Result<Self, TimeoutBudgetError> {
        let (now, _observation) = self.observe(effects).await?;
        let active = self.active.child_budget(&now, requested);
        self.checkpoint().await?;
        Ok(Self {
            active: active?,
            original: self.original.clone(),
            checkpoint: self.checkpoint.clone(),
            _lease: self._lease.clone(),
        })
    }
    /// Attenuate the existing owner to signed manifest validity using one owned
    /// physical observation. No second read can shift the derived endpoint later.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "issued_notice_validity_window",
        capability_type = EnrollmentWindowCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn issued_notice_validity_child(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<EnrollmentWindowCapability, AuraError> {
        self.require_issued_notice_owner(issued)?;
        let (now, _observation) = self.observe(effects).await.map_err(AuraError::from)?;
        let active = self.active.remaining_at(&now);
        self.checkpoint().await.map_err(AuraError::from)?;
        active.map_err(AuraError::from)?;
        let remaining = signed_notice_validity_remaining(&now, issued.manifest().expires_at_ms)?;
        let active = self
            .active
            .child_budget(&now, remaining)
            .map_err(AuraError::from)?;
        Ok(Self {
            active,
            original: self.original.clone(),
            checkpoint: self.checkpoint.clone(),
            _lease: self._lease.clone(),
        })
    }

    pub(crate) async fn retry_delay<E: PhysicalTimeEffects + ?Sized>(
        &self,
        effects: &E,
        delay_ms: u64,
    ) -> Result<(), TimeoutBudgetError> {
        let remaining = self.remaining_ms(effects).await?;
        effects
            .sleep_ms(delay_ms.min(remaining))
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        self.remaining_ms(effects).await?;
        Ok(())
    }
    pub(crate) async fn execute<ETime, F, Fut, T, E>(
        &self,
        time: &ETime,
        operation: F,
    ) -> Result<T, TimeoutRunError<E>>
    where
        ETime: PhysicalTimeEffects + Sync,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        aura_core::time::timeout::execute_with_timeout_budget_and_checkpoint(
            time,
            &self.active,
            || self.checkpoint(),
            operation,
        )
        .await
    }
    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        map_enrollment_run_error(stage, &self.active, source)
    }
}

pub(crate) fn map_enrollment_run_error(
    stage: &'static str,
    active: &TimeoutBudget,
    source: TimeoutRunError<crate::core::AgentError>,
) -> crate::core::AgentError {
    match source {
        TimeoutRunError::Operation(source) => source,
        TimeoutRunError::Timeout(source) => {
            use crate::core::AgentError;
            let deadline = matches!(&source, TimeoutBudgetError::DeadlineExceeded { .. });
            let invalid = match &source {
                TimeoutBudgetError::InvalidPolicy { .. }
                | TimeoutBudgetError::AttemptBudgetExhausted { .. } => true,
                TimeoutBudgetError::DeadlineExceeded { .. }
                | TimeoutBudgetError::ClockRollback { .. }
                | TimeoutBudgetError::ObservationUnavailable
                | TimeoutBudgetError::CheckpointDiscontinuity { .. }
                | TimeoutBudgetError::CheckpointFailure { .. }
                | TimeoutBudgetError::TimeSourceUnavailable { .. } => false,
            };
            let message = format!("{stage}: {source}");
            let source = Some(Arc::new(source) as Arc<dyn std::error::Error + Send + Sync>);
            if deadline {
                AgentError::TimeoutWithSource {
                    message: format!("{stage} exceeded {}ms overall timeout", active.timeout_ms()),
                    source: AuraError::Internal { message, source },
                }
            } else if invalid {
                AgentError::Aura(AuraError::Invalid { message, source })
            } else {
                AgentError::Aura(AuraError::Internal { message, source })
            }
        }
    }
}

/// Pure arithmetic only. A raw timestamp is never a notice admission input.
fn signed_notice_validity_remaining(
    now: &PhysicalTime,
    expires_at_ms: u64,
) -> Result<Duration, AuraError> {
    let remaining = expires_at_ms
        .checked_sub(now.ts_ms)
        .filter(|remaining| *remaining > 0)
        .ok_or_else(|| AuraError::Invalid {
            message: "issued enrollment manifest validity ended".into(),
            source: Some(Arc::new(
                aura_invitation::enrollment_manifest::EnrollmentManifestError::Expired,
            )),
        })?;
    Ok(Duration::from_millis(remaining))
}

#[cfg(test)]
mod notice_validity_tests {
    use super::*;
    #[test]
    fn signed_validity_child_keeps_exact_endpoint_and_original_owner() {
        let observed = PhysicalTime::exact(100);
        let original =
            TimeoutBudget::from_start_and_timeout(&observed, Duration::from_millis(1_000))
                .expect("valid original registration interval");
        let remaining =
            signed_notice_validity_remaining(&observed, 350).expect("live signed validity");
        let child = original
            .child_budget(&observed, remaining)
            .expect("attenuation uses the same actual observation");
        assert_eq!(child.deadline_at_ms(), 350);
        assert_eq!(original.deadline_at_ms(), 1_100);
        assert_eq!(
            child
                .remaining_at(&PhysicalTime::exact(340))
                .expect("same fixed endpoint after elapsed work"),
            Duration::from_millis(10)
        );
        assert_eq!(
            original
                .remaining_at(&PhysicalTime::exact(340))
                .expect("same observed physical position remains usable"),
            Duration::from_millis(760)
        );
        assert!(
            matches!(
                original.remaining_at(&PhysicalTime::exact(339)),
                Err(TimeoutBudgetError::ClockRollback { .. })
            ),
            "child progress must advance the original shared observation high-water"
        );
        let shorter = TimeoutBudget::from_start_and_timeout(&observed, Duration::from_millis(100))
            .expect("valid shorter original interval");
        let child = shorter
            .child_budget(&observed, remaining)
            .expect("intersection is valid");
        assert_eq!(
            child.deadline_at_ms(),
            200,
            "signed validity cannot extend original deadline"
        );
    }
    #[test]
    fn elapsed_signed_validity_is_domain_expiry_with_original_source() {
        for observed in [350, 351, u64::MAX] {
            let error = signed_notice_validity_remaining(&PhysicalTime::exact(observed), 350)
                .expect_err("zero or negative signed remaining lifetime is not admissible");
            let source = std::error::Error::source(&error).expect("typed expiry is retained");
            assert!(matches!(
                source
                    .downcast_ref::<aura_invitation::enrollment_manifest::EnrollmentManifestError>(
                    ),
                Some(aura_invitation::enrollment_manifest::EnrollmentManifestError::Expired)
            ));
            assert!(
                !source.is::<TimeoutBudgetError>(),
                "no clock failure or fabricated deadline"
            );
        }
    }
}

#[cfg(test)]
mod admitted_clock_split_tests {
    use super::*;

    #[cfg(unix)]
    async fn assert_live_anchor_loss_rejects_legacy(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
        binding: &AdmittedWindowBinding,
        anchor: &[u8],
        retained_checkpoint: &[u8],
    ) {
        let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", binding);
        let checkpoint_key = admitted_location("admitted_enrollment_clock_checkpoint_v2", binding);
        effects
            .secure_create_mutable(
                &checkpoint_key,
                retained_checkpoint,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .unwrap();
        let protected: StoredAdmittedAnchor = serde_json::from_slice(anchor).unwrap();
        let legacy = StoredAdmittedWindow {
            binding: binding.clone(),
            budget: TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(binding.admitted_at_ms),
                Duration::from_millis(
                    (binding.expires_at_ms - binding.admitted_at_ms).min(240_000),
                ),
            )
            .unwrap(),
        };
        assert!(protected.window.binding == *binding);
        effects
            .secure_store_immutable(
                &admitted_location("admitted_enrollment_clock_v1", binding),
                &serde_json::to_vec(&legacy).unwrap(),
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .unwrap();
        // Actual backing loss, not an exemption from immutable deletion.
        assert!(effects
            .fault_remove_secure_record_for_test(&anchor_key)
            .await
            .unwrap());
        assert!(
            EnrollmentWindowCapability::read_admitted_anchor(effects, binding)
                .await
                .is_err()
        );
        assert!(
            EnrollmentWindowCapability::require_retained_admitted_window(effects, witness)
                .await
                .is_err()
        );
        assert!(!effects.secure_exists(&anchor_key).await.unwrap());
    }
    #[tokio::test]
    async fn actual_admitted_checkpoint_updates_without_mutating_anchor_and_never_repairs_live_loss(
    ) {
        Box::pin(async {
            let (_issuer, invitee, invitation, _start, _acceptance, _response) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "admitted-clock-split",
                )
                .await;
            let effects = invitee.runtime().effects();
            let witness =
                crate::handlers::invitation::enrollment_manifest_admission::load_admitted_baseline(
                    effects.as_ref(),
                    invitee.authority_id(),
                    &invitation,
                )
                .await
                .unwrap();
            let binding = EnrollmentWindowCapability::admitted_binding(&witness);
            let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", &binding);
            let checkpoint_key =
                admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding);
            let anchor = effects
                .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            // Interrupted INITIAL publication is repairable only inside the
            // original reverified owner's actual exclusive execution lease.
            effects
                .secure_delete(&checkpoint_key, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
            assert!(
                EnrollmentWindowCapability::require_retained_admitted_window(
                    effects.as_ref(),
                    &witness
                )
                .await
                .is_err()
            );
            let window = EnrollmentWindowCapability::admitted(effects.clone(), &witness)
                .await
                .unwrap();
            window.checkpoint().await.unwrap();
            window.checkpoint().await.unwrap();
            assert_eq!(
                effects
                    .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                anchor
            );
            #[cfg(unix)]
            let retained_checkpoint = effects
                .secure_retrieve(&checkpoint_key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let original_deadline = window.original.deadline_at_ms();
            drop(window);
            let restored = EnrollmentWindowCapability::admitted(effects.clone(), &witness)
                .await
                .unwrap();
            assert_eq!(restored.original.deadline_at_ms(), original_deadline);
            drop(restored);
            effects
                .secure_delete(&checkpoint_key, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
            assert!(
                EnrollmentWindowCapability::admitted(effects.clone(), &witness)
                    .await
                    .is_err()
            );
            assert!(!effects.secure_exists(&checkpoint_key).await.unwrap());
            assert_eq!(
                effects
                    .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                anchor
            );
            #[cfg(unix)]
            assert_live_anchor_loss_rejects_legacy(
                effects.as_ref(),
                &witness,
                &binding,
                &anchor,
                &retained_checkpoint,
            )
            .await;
        })
        .await;
    }

    #[test]
    fn original_legacy_interval_is_attenuated_without_clamping_fresh_signed_window() {
        let binding = AdmittedWindowBinding {
            ceremony: aura_core::CeremonyId::new("pure-clock-interval"),
            invitation: aura_core::InvitationId::new("pure-clock-invitation"),
            manifest_digest: [7; 32],
            device: aura_core::DeviceId::new_from_entropy([8; 32]),
            admitted_at_ms: 100,
            expires_at_ms: 900_100,
        };
        let fresh = StoredAdmittedWindow {
            binding: binding.clone(),
            budget: TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(100),
                Duration::from_millis(900_000),
            )
            .unwrap(),
        };
        let legacy = StoredAdmittedWindow {
            binding: binding.clone(),
            budget: TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(100),
                Duration::from_millis(240_000),
            )
            .unwrap(),
        };
        EnrollmentWindowCapability::validate_admitted_record(&fresh, &binding, false).unwrap();
        EnrollmentWindowCapability::validate_admitted_record(&legacy, &binding, true).unwrap();
        assert!(
            EnrollmentWindowCapability::validate_admitted_record(&fresh, &binding, true).is_err()
        );
        assert!(
            EnrollmentWindowCapability::validate_admitted_record(&legacy, &binding, false).is_err()
        );
    }
}

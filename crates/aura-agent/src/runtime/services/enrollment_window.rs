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
impl EnrollmentWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "registered_enrollment_window",
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
        family = "runtime_helper"
    )]
    pub(crate) async fn admitted(
        effects: Arc<AuraEffectSystem>,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<Self, AuraError> {
        let lease = effects
            .acquire_admitted_enrollment_window_owner(witness)?
            .into_permit();
        let manifest = witness.manifest();
        let binding = AdmittedWindowBinding {
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            manifest_digest: witness.manifest_digest(),
            device: manifest.invitee_device,
            expires_at_ms: manifest.expires_at_ms,
            admitted_at_ms: witness.admitted_at_ms(),
        };
        let location = SecureStorageLocation::new(
            "admitted_enrollment_clock_v1",
            binding.ceremony.to_string(),
        );
        let bytes = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized admitted enrollment clock"));
        }
        let record: StoredAdmittedWindow =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Internal {
                message: "decode admitted enrollment clock".into(),
                source: Some(Arc::new(source)),
            })?;
        let expected_allowance = binding
            .expires_at_ms
            .checked_sub(binding.admitted_at_ms)
            .ok_or_else(|| AuraError::invalid("invalid original admitted window"))?
            .min(240_000);
        if record.binding != binding
            || record.budget.started_at_ms() != binding.admitted_at_ms
            || record.budget.timeout_ms() != expected_allowance
        {
            return Err(AuraError::invalid(
                "admitted enrollment clock binding mismatch",
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
            .ok_or_else(|| AuraError::invalid("invalid initial admitted window"))?
            .min(240_000);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(binding.admitted_at_ms),
            Duration::from_millis(allowance),
        )
        .map_err(AuraError::from)?;
        let record = StoredAdmittedWindow { binding, budget };
        let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Internal {
            message: "encode original admitted clock".into(),
            source: Some(Arc::new(source)),
        })?;
        let location = SecureStorageLocation::new(
            "admitted_enrollment_clock_v1",
            record.binding.ceremony.to_string(),
        );
        let outcome = effects
            .secure_store_immutable(
                &location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        if matches!(
            outcome,
            aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
        ) {
            Self::require_retained_admitted_window(effects, witness).await?;
        }
        Ok(())
    }
    pub(crate) async fn require_retained_admitted_window(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<(), AuraError> {
        let bytes = effects
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "admitted_enrollment_clock_v1",
                    witness.manifest().ceremony.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 16_384 {
            return Err(AuraError::invalid("oversized retained admitted clock"));
        }
        let record: StoredAdmittedWindow =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Internal {
                message: "decode required admitted clock".into(),
                source: Some(Arc::new(source)),
            })?;
        let manifest = witness.manifest();
        let allowance = manifest
            .expires_at_ms
            .checked_sub(witness.admitted_at_ms())
            .ok_or_else(|| AuraError::invalid("invalid original admitted window"))?
            .min(240_000);
        if record.binding.ceremony != manifest.ceremony
            || record.binding.invitation != manifest.invitation
            || record.binding.manifest_digest != witness.manifest_digest()
            || record.binding.device != manifest.invitee_device
            || record.binding.expires_at_ms != manifest.expires_at_ms
            || record.binding.admitted_at_ms != witness.admitted_at_ms()
            || record.budget.started_at_ms() != witness.admitted_at_ms()
            || record.budget.timeout_ms() != allowance
        {
            return Err(AuraError::invalid(
                "retained admitted clock has another owner binding",
            ));
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
        let allowance = admitted
            .manifest()
            .expires_at_ms
            .checked_sub(original_start)
            .ok_or_else(|| AuraError::invalid("invalid original confirmation window"))?
            .min(240_000);
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
        let allowance = admitted
            .manifest()
            .expires_at_ms
            .checked_sub(original_start)
            .ok_or_else(|| AuraError::invalid("invalid original failure window"))?
            .min(240_000);
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
                    "admitted_enrollment_clock_v1",
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
                            "admitted_enrollment_clock_v1",
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
                        message: format!(
                            "{stage} exceeded {}ms overall timeout",
                            self.active.timeout_ms()
                        ),
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
}

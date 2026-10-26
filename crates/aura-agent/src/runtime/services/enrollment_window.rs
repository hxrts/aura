//! Sealed durable clock owner for enrollment execution.
mod cancelled_notice;
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
pub(crate) use cancelled_notice::OriginalCancelledNoticeWindowCapability;
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedSemaphorePermit};

mod signing_checkpoint;
mod transport_custody;
pub(crate) use transport_custody::{
    OriginalAdmittedAckBorrow, OriginalAdmittedAckFailure, OriginalAdmittedTransportCustody,
    OriginalAdmittedTransportGate,
};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod launch_contract_tests;

#[cfg(test)]
mod signing_contract_guards {
    use super::HeldIssuerCompletionObserver;

    #[test]
    fn completion_observer_cannot_duplicate_or_deserialize_original_custody() {
        struct CloneImplemented;
        trait AmbiguousIfClone<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfClone<()> for T {}
        impl<T: Clone> AmbiguousIfClone<CloneImplemented> for T {}
        let _ = <HeldIssuerCompletionObserver as AmbiguousIfClone<_>>::marker;
        let _ = <super::OriginalAdmittedAckBorrow as AmbiguousIfClone<_>>::marker;
        let _ =
            <crate::handlers::invitation::AcknowledgedOriginalNoticeDisposal as AmbiguousIfClone<
                _,
            >>::marker;
        let _ = <crate::runtime::effects::OwnedSecretBirthCapability<'static, 'static> as AmbiguousIfClone<_>>::marker;
        let _ = <super::OriginalIssuerLaunchWindow as AmbiguousIfClone<_>>::marker;
        let _ = <super::OriginalIssuerLaunchObservation as AmbiguousIfClone<_>>::marker;
        let _ = <super::OriginalAllocationCleanupAcknowledgment as AmbiguousIfClone<_>>::marker;
        let _ = <crate::handlers::invitation::IssuerEnrollmentTerminalReceipt as AmbiguousIfClone<_>>::marker;
        let _ = <super::EnrollmentExecutionRoot<super::RegisteredEnrollmentWindowCapability> as AmbiguousIfClone<_>>::marker;
        let _ = <super::AcknowledgedIssuerEnrollmentWindow as AmbiguousIfClone<_>>::marker;
        let _ = <super::super::ceremony_tracker::OriginalEnrollmentNegativeDecisionReceipt as AmbiguousIfClone<_>>::marker;
        let _ = <super::super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt as AmbiguousIfClone<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalNegativeTerminalRequest as AmbiguousIfClone<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalNegativeTerminalSigningMaterial as AmbiguousIfClone<_>>::marker;
        let _ = <super::OriginalConditionalSigningWindowIdentity as AmbiguousIfClone<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalConditionalTransport<'static> as AmbiguousIfClone<_>>::marker;

        struct DeserializeImplemented;
        trait AmbiguousIfDeserialize<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
        impl<T: serde::de::DeserializeOwned> AmbiguousIfDeserialize<DeserializeImplemented> for T {}
        let _ = <HeldIssuerCompletionObserver as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::OriginalAdmittedAckBorrow as AmbiguousIfDeserialize<_>>::marker;
        let _ = <crate::handlers::invitation::AcknowledgedOriginalNoticeDisposal as AmbiguousIfDeserialize<_>>::marker;
        let _ = <crate::runtime::effects::OwnedSecretBirthCapability<'static, 'static> as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::OriginalIssuerLaunchWindow as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::OriginalIssuerLaunchObservation as AmbiguousIfDeserialize<_>>::marker;
        let _ =
            <super::OriginalAllocationCleanupAcknowledgment as AmbiguousIfDeserialize<_>>::marker;
        let _ = <crate::handlers::invitation::IssuerEnrollmentTerminalReceipt as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::EnrollmentExecutionRoot<super::RegisteredEnrollmentWindowCapability> as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::AcknowledgedIssuerEnrollmentWindow as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::super::ceremony_tracker::OriginalEnrollmentNegativeDecisionReceipt as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalNegativeTerminalRequest as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalNegativeTerminalSigningMaterial as AmbiguousIfDeserialize<_>>::marker;
        let _ =
            <super::OriginalConditionalSigningWindowIdentity as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::super::threshold_signing::OriginalConditionalTransport<'static> as AmbiguousIfDeserialize<_>>::marker;
    }
}

/// Restricted original issuer completion observation. No execution permit,
/// Clone/Deserialize, arbitrary executor, child, nonce or session API is exposed.
pub(crate) struct HeldIssuerCompletionObserver {
    active: TimeoutBudget,
    original: super::ceremony_tracker::HeldIssuerClockObservationCapability,
}
impl HeldIssuerCompletionObserver {
    pub(super) async fn observe_original_retirement(
        &self,
        effects: &AuraEffectSystem,
        original: &Arc<RegisteredEnrollmentWindowCapability>,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        destination: &crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
    ) -> crate::core::AgentResult<()> {
        self.original.require_effects(effects)?;
        original.require_issued_notice_control(issued)?;
        aura_core::execute_with_timeout_budget(effects, &self.active, || async {
            destination
                .await_original_retirement(original, issued)
                .await
                .map_err(crate::core::AgentError::from)
        })
        .await
        .map_err(|source| {
            map_enrollment_run_error("observe original required retirement", &self.active, source)
        })
    }
    pub(crate) async fn observe_registered_admission(
        &self,
        effects: &AuraEffectSystem,
        initialized: tokio::sync::oneshot::Receiver<Result<(), AuraError>>,
    ) -> crate::core::AgentResult<()> {
        self.original.require_effects(effects)?;
        aura_core::time::timeout::execute_with_timeout_budget(effects, &self.active, || async {
            initialized.await.map_err(|source| AuraError::Internal {
                message: "original issuer initialization observation lost".into(),
                source: Some(Arc::new(source)),
            })?
        })
        .await
        .map_err(|source| {
            map_enrollment_run_error(
                "observe original registered admission",
                &self.active,
                match source {
                    TimeoutRunError::Operation(source) => {
                        TimeoutRunError::Operation(crate::core::AgentError::from(source))
                    }
                    TimeoutRunError::Timeout(source) => TimeoutRunError::Timeout(source),
                },
            )
        })
    }
    #[cfg(test)]
    pub(super) async fn observe_original_terminal(
        &self,
        effects: &AuraEffectSystem,
        runner: &super::ceremony_runner::CeremonyRunner,
    ) -> crate::core::AgentResult<()> {
        self.original.require_effects(effects)?;
        aura_core::time::timeout::execute_with_timeout_budget(effects, &self.active, || async {
            match runner
                .await_enrollment_terminal_outcome(self.original.ceremony_id())
                .await?
            {
                aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed => Ok(()),
                aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(reason) => {
                    Err(crate::core::AgentError::from(AuraError::PermissionDenied {
                        message: "original issuer finalization was negative".into(),
                        source: Some(Arc::new(
                            crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalFailed(
                                reason,
                            ),
                        )),
                    }))
                }
            }
        })
        .await
        .map_err(|source| {
            map_enrollment_run_error("observe original issuer finalization", &self.active, source)
        })
    }
    pub(crate) async fn receive_issuer_result(
        &self,
        effects: &AuraEffectSystem,
        receiver: &mut Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> Result<(), AuraError> {
        self.original.require_effects(effects)?;
        aura_core::time::timeout::execute_with_timeout_budget(effects, &self.active, || async {
            self.original.require_pending().await?;
            if let Some(original) = receiver.as_mut() {
                original.await.map_err(|source| AuraError::Internal {
                    message: "original issuer result readiness channel stopped".into(),
                    source: Some(Arc::new(source)),
                })?;
                // This slot belongs to the retained entry, not the observation
                // future. Preserve successful readiness through outer Drop.
                *receiver = None;
            }
            Ok::<(), AuraError>(())
        })
        .await
        .map_err(|source| AuraError::Internal {
            message: "original issuer completion observation failed".into(),
            source: Some(Arc::new(source)),
        })
    }

    pub(crate) async fn wait_owned_group(
        &self,
        effects: &AuraEffectSystem,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<(), crate::task_registry::TaskSupervisionError> {
        use crate::task_registry::TaskSupervisionError;
        self.original
            .require_effects(effects)
            .map_err(|source| TaskSupervisionError::Budget {
                group: group.name().into(),
                source: Box::new(TimeoutBudgetError::checkpoint_failure(source)),
            })?;
        let outcome = aura_core::time::timeout::execute_with_timeout_budget(
            effects,
            &self.active,
            || async {
                self.original.require_pending().await.map_err(|source| {
                    TaskSupervisionError::Budget {
                        group: group.name().into(),
                        source: Box::new(TimeoutBudgetError::checkpoint_failure(source)),
                    }
                })?;
                group.wait_with_original_budget(effects, &self.active).await
            },
        )
        .await;
        match outcome {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(source)) => {
                let failure = match source {
                    source @ TimeoutBudgetError::DeadlineExceeded { .. } => {
                        TaskSupervisionError::Timeout {
                            group: group.name().into(),
                            active_tasks: group.active_tasks(),
                            source: Box::new(source),
                        }
                    }
                    source => TaskSupervisionError::Budget {
                        group: group.name().into(),
                        source: Box::new(source),
                    },
                };
                group.request_cancellation();
                match group.abort_remaining() {
                    Ok(()) => Err(failure),
                    Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        ..
                    }) => Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        cause: Some(Box::new(failure)),
                    }),
                    Err(source) => Err(source),
                }
            }
        }
    }

    pub(crate) async fn shutdown_owned_group(
        &self,
        effects: &AuraEffectSystem,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<(), crate::task_registry::TaskSupervisionError> {
        group.request_cancellation();
        self.wait_owned_group(effects, group).await
    }
}

/// Preparation authority is separate from protocol execution admission.
pub(crate) enum EnrollmentCancellationPreparationCapability {
    Active(Box<EnrollmentCancellationWindowCapability>),
    Decided(super::ceremony_tracker::VerifiedEnrollmentCancellationCapability),
}
pub(crate) struct EnrollmentCancellationWindowCapability {
    active: TimeoutBudget,
    owner: Arc<super::ceremony_tracker::CancellationClockObservationCapability>,
    admission: Option<aura_core::time::timeout::CheckpointExecution<(), AuraError, ()>>,
}
impl EnrollmentCancellationWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_cancellation_window",
        capability_type = CancellationClockObservationCapability,
        family = "runtime_helper"
    )]
    pub(super) fn registered(
        capability: super::ceremony_tracker::CancellationClockObservationCapability,
        effects: &AuraEffectSystem,
    ) -> Result<Self, AuraError> {
        Self::from_original_observation(Arc::new(capability), effects)
    }
    fn from_original_observation(
        owner: Arc<super::ceremony_tracker::CancellationClockObservationCapability>,
        effects: &AuraEffectSystem,
    ) -> Result<Self, AuraError> {
        owner.require_effects(effects)?;
        Ok(Self {
            active: owner.budget().clone(),
            owner,
            admission: None,
        })
    }
    pub(crate) async fn admit_original(&mut self, effects: &AuraEffectSystem) {
        // The original role holder owns this entire value before this await.
        // A derived interval is retained before callback Ready, but cannot
        // authorize work unless the complete original observation succeeds.
        if self.admission.is_some() {
            return;
        }
        let owner = &self.owner;
        let active = &mut self.active;
        let actual = aura_core::time::timeout::execute_with_timeout_budget_and_checkpoint(
            effects,
            owner.budget(),
            || async {
                owner
                    .checkpoint()
                    .await
                    .map_err(TimeoutBudgetError::checkpoint_failure)
            },
            || async {
                owner.require_effects(effects)?;
                let now = effects
                    .physical_time()
                    .await
                    .map_err(TimeoutBudgetError::time_source_failure)?;
                *active = owner.budget().child_budget(
                    &now,
                    signed_notice_validity_remaining(&now, owner.signed_expiry_ms())?,
                )?;
                Ok::<(), AuraError>(())
            },
        )
        .await;
        self.admission = Some(actual);
    }
    pub(crate) fn admitted_observation(
        &self,
    ) -> Option<&aura_core::time::timeout::CheckpointExecution<(), AuraError, ()>> {
        self.admission.as_ref()
    }
    pub(crate) async fn execute<F, Fut, T, E>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<aura_core::time::timeout::CheckpointExecution<T, E, ()>, TimeoutBudgetError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.owner
            .require_effects(effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        if !matches!(
            self.admission.as_ref(),
            Some(aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            })
        ) {
            return Err(TimeoutBudgetError::checkpoint_failure(
                DurableEnrollmentExecutionError::ForeignAdmission.refusal(),
            ));
        }
        Ok(
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
            .await,
        )
    }
    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        map_enrollment_run_error(stage, &self.active, source)
    }
}

#[cfg(test)]
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
    cancelled: Arc<super::ceremony_tracker::VerifiedEnrollmentCancellationCapability>,
    // Retains the actual registered task's original owner through window disposal.
    _original: Arc<super::ceremony_tracker::RegisteredCancelledNoticeCapability>,
    manifest_digest: [u8; 32],
}
impl CancelledEnrollmentNoticeWindowCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "CancelledEnrollmentNoticeWindowCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn registered(
        capability: Arc<super::ceremony_tracker::RegisteredCancelledNoticeCapability>,
        effects: &AuraEffectSystem,
    ) -> Result<CancelledEnrollmentNoticeWindowCapability, AuraError> {
        let (observation, lease, cancelled, manifest_digest) = capability.original_parts();
        let window = EnrollmentCancellationWindowCapability::from_original_observation(
            observation,
            effects,
        )?;
        Ok(Self {
            window,
            _lease: lease,
            cancelled,
            manifest_digest,
            _original: capability,
        })
    }
    pub(crate) async fn admit_original(&mut self, effects: &AuraEffectSystem) {
        self.window.admit_original(effects).await;
    }
    #[cfg(test)]
    pub(crate) fn admitted_observation(
        &self,
    ) -> Option<&aura_core::time::timeout::CheckpointExecution<(), AuraError, ()>> {
        self.window.admitted_observation()
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
    pub(crate) async fn execute<F, Fut, T>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::CheckpointExecution<T, crate::core::AgentError, ()>,
        TimeoutBudgetError,
    >
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

/// Restart custody for observing a signed negative notice only. Protected
/// clock bytes cannot reconstruct the original positive execution root.
pub(crate) struct RecoveredEnrollmentNoticeWindowCapability {
    effects: Arc<AuraEffectSystem>,
    admitted: Arc<AdmittedEnrollmentManifest>,
    binding: AdmittedWindowBinding,
    active: TimeoutBudget,
    _lease: OwnedSemaphorePermit,
    prepared: Option<crate::handlers::invitation::enrollment_manifest_admission::PreparedRecoveredEnrollmentFailure>,
    restored_prepared: bool,
    closing: Mutex<Option<(Vec<u8>, u64)>>,
}

impl RecoveredEnrollmentNoticeWindowCapability {
    #[cfg(test)]
    pub(crate) fn original_admitted_anchor_location_for_test(
        admitted: &AdmittedEnrollmentManifest,
    ) -> SecureStorageLocation {
        admitted_location(
            "admitted_enrollment_clock_anchor_v2",
            &EnrollmentExecutionChild::admitted_binding(admitted),
        )
    }

    #[cfg(test)]
    pub(crate) fn original_admitted_checkpoint_location_for_test(
        admitted: &AdmittedEnrollmentManifest,
    ) -> SecureStorageLocation {
        admitted_location(
            "admitted_enrollment_clock_checkpoint_v2",
            &EnrollmentExecutionChild::admitted_binding(admitted),
        )
    }

    #[cfg(test)]
    pub(crate) fn assert_recovered_notice_missing_checkpoint_for_test(error: &AuraError) {
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
        while let Some(actual) = source {
            if matches!(
                actual.downcast_ref::<DurableEnrollmentExecutionError>(),
                Some(DurableEnrollmentExecutionError::MissingTerminalCheckpoint)
            ) {
                return;
            }
            source = actual.source();
        }
        panic!("missing original terminal checkpoint source required: {error:?}");
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "recovered_negative_enrollment_notice_window",
        capability_type = AdmittedEnrollmentManifest,
        family = "runtime_helper"
    )]
    pub(crate) async fn recover(
        effects: Arc<AuraEffectSystem>,
        admitted: Arc<AdmittedEnrollmentManifest>,
    ) -> Result<Self, AuraError> {
        let binding = EnrollmentExecutionChild::admitted_binding(&admitted);
        if binding.device != effects.device_id() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let lease = effects.acquire_admitted_enrollment_window_owner(&admitted)?;
        lease.require_effects(effects.as_ref())?;
        let anchor = EnrollmentExecutionChild::read_admitted_anchor(&effects, &binding).await?;
        let checkpoint = EnrollmentExecutionChild::decode_admitted_record(
            &effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding),
        )
        .await?;
        EnrollmentExecutionChild::validate_admitted_record(&checkpoint, &binding)?;
        let prepared = crate::handlers::invitation::enrollment_manifest_admission::load_prepared_recovered_enrollment_failure(
            &effects, &admitted,
        ).await?;
        if anchor.execution != DurableEnrollmentExecutionState::Pending
            || !(matches!(
                checkpoint.execution,
                DurableEnrollmentExecutionState::Pending | DurableEnrollmentExecutionState::Refused
            ) || (checkpoint.execution == DurableEnrollmentExecutionState::Closed
                && prepared.is_some()))
        {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        checkpoint
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        let lease = lease.into_permit();
        let owner = Self {
            effects,
            admitted,
            binding,
            active: checkpoint.budget,
            _lease: lease,
            restored_prepared: prepared.is_some(),
            prepared,
            closing: Mutex::new(None),
        };
        owner
            .remaining_ms(owner.effects.as_ref())
            .await
            .map_err(AuraError::from)?;
        Ok(owner)
    }

    pub(crate) fn require_admitted_owner(
        &self,
        admitted: &AdmittedEnrollmentManifest,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.as_ref(), effects) {
            return Err(DurableEnrollmentExecutionError::ForeignProvider.refusal());
        }
        if !std::ptr::eq(self.admitted.as_ref(), admitted)
            || self.binding != EnrollmentExecutionChild::admitted_binding(admitted)
        {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        Ok(())
    }

    async fn require_retained_checkpoint(&self) -> Result<(), TimeoutBudgetError> {
        let checkpoint = EnrollmentExecutionChild::read_bound_admitted_checkpoint(
            self.effects.as_ref(),
            &self.binding,
            &self.active,
        )
        .await?;
        let closing_retained = self
            .closing
            .try_lock()
            .map_err(TimeoutBudgetError::checkpoint_failure)?
            .is_some();
        if !(matches!(
            checkpoint.execution,
            DurableEnrollmentExecutionState::Pending | DurableEnrollmentExecutionState::Refused
        ) || (checkpoint.execution == DurableEnrollmentExecutionState::Closed
            && (self.restored_prepared || closing_retained)))
        {
            return Err(TimeoutBudgetError::checkpoint_failure(
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal(),
            ));
        }
        Ok(())
    }

    pub(crate) fn require_notice_ingress(
        &self,
        admitted: &AdmittedEnrollmentManifest,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.require_admitted_owner(admitted, effects)?;
        let closing = self
            .closing
            .try_lock()
            .map_err(|source| AuraError::Internal {
                message: "original negative notice closure is in progress".into(),
                source: Some(Arc::new(source)),
            })?;
        if self.restored_prepared || closing.is_some() {
            return Err(AuraError::permission_denied(
                "recovered negative notice permits finalization only",
            ));
        }
        Ok(())
    }

    pub(crate) async fn remaining_ms(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<u64, TimeoutBudgetError> {
        self.require_admitted_owner(&self.admitted, effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let result = aura_core::execute_with_timeout_budget(effects, &self.active, || async {
            self.require_retained_checkpoint().await?;
            let now = effects
                .physical_time()
                .await
                .map_err(TimeoutBudgetError::time_source_failure)?;
            let remaining = self.active.remaining_at(&now)?;
            u64::try_from(remaining.as_millis())
                .map_err(|source| TimeoutBudgetError::invalid_policy(source.to_string()))
        })
        .await;
        match result {
            Ok(remaining) => Ok(remaining),
            Err(TimeoutRunError::Operation(source) | TimeoutRunError::Timeout(source)) => {
                Err(source)
            }
        }
    }

    pub(crate) async fn execute<F, Fut, T, E>(
        &self,
        effects: &AuraEffectSystem,
        operation: F,
    ) -> Result<T, TimeoutRunError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.require_admitted_owner(&self.admitted, effects)
            .map_err(|source| {
                TimeoutRunError::Timeout(TimeoutBudgetError::checkpoint_failure(source))
            })?;
        let result = aura_core::execute_with_timeout_budget(effects, &self.active, || async {
            self.require_retained_checkpoint().await?;
            Ok(operation().await)
        })
        .await;
        match result {
            Ok(result) => result.map_err(TimeoutRunError::Operation),
            Err(TimeoutRunError::Operation(source) | TimeoutRunError::Timeout(source)) => {
                Err(TimeoutRunError::Timeout(source))
            }
        }
    }

    pub(crate) fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<crate::core::AgentError>,
    ) -> crate::core::AgentError {
        map_enrollment_run_error(stage, &self.active, source)
    }

    pub(crate) fn take_prepared_failure(&mut self) -> Option<crate::handlers::invitation::enrollment_manifest_admission::PreparedRecoveredEnrollmentFailure>{
        self.prepared.take()
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "prepared_recovered_negative_enrollment_failure",
        capability_type = PreparedRecoveredEnrollmentFailure,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_failure(
        &self,
        effects: &AuraEffectSystem,
        prepared: &crate::handlers::invitation::enrollment_manifest_admission::PreparedRecoveredEnrollmentFailure,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_admitted_owner(&self.admitted, effects)?;
        prepared.require_original(&self.admitted, effects)?;
        let proof = prepared.proof();
        let encode = |invitation| {
            aura_core::util::serialization::to_vec(invitation).map_err(|source| {
                AuraError::Serialization {
                    message: "encode original negative notice binding".into(),
                    source: Some(Arc::new(source)),
                }
            })
        };
        if proof.manifest_digest() != self.binding.manifest_digest
            || encode(proof.canonical_invitation())?
                != encode(self.admitted.canonical_invitation())?
        {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let result = aura_core::execute_with_timeout_budget(effects, &self.active, || async {
            self.require_retained_checkpoint()
                .await
                .map_err(AuraError::from)?;
            let mut closing = self.closing.lock().await;
            if closing.is_none() {
                let now = effects
                    .physical_time()
                    .await
                    .map_err(TimeoutBudgetError::time_source_failure)
                    .map_err(AuraError::from)?;
                self.active.remaining_at(&now).map_err(AuraError::from)?;
                let record = StoredAdmittedWindow {
                    binding: self.binding.clone(),
                    budget: self.active.clone(),
                    execution: DurableEnrollmentExecutionState::Closed,
                };
                let bytes = serde_json::to_vec(&record)
                    .map_err(TimeoutBudgetError::checkpoint_failure)
                    .map_err(AuraError::from)?;
                // The exact proposed close survives Drop during store/readback.
                *closing = Some((bytes, now.ts_ms));
            }
            let (bytes, acknowledged_at_ms) = closing.as_ref().expect("retained original close");
            let key = admitted_location("admitted_enrollment_clock_checkpoint_v2", &self.binding);
            effects
                .secure_store(&key, bytes, &[SecureStorageCapability::Write])
                .await?;
            let stored = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await?;
            if stored != *bytes {
                return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
            }
            let frozen: StoredAdmittedWindow = serde_json::from_slice(bytes)
                .map_err(TimeoutBudgetError::checkpoint_failure)
                .map_err(AuraError::from)?;
            let frozen_budget = serde_json::to_vec(&frozen.budget)
                .map_err(TimeoutBudgetError::checkpoint_failure)
                .map_err(AuraError::from)?;
            Ok(AcknowledgedEnrollmentWindow {
                binding: self.binding.clone(),
                frozen_budget,
                acknowledged_at_ms: *acknowledged_at_ms,
            })
        })
        .await;
        match result {
            Ok(acknowledged) => Ok(acknowledged),
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(source)) => Err(AuraError::from(source)),
        }
    }
}

#[derive(Clone)]
enum EnrollmentExecutionLease {
    Registered(Arc<super::ceremony_tracker::OriginalRegisteredExecutionLease>),
    Exclusive {
        _original: Arc<OwnedSemaphorePermit>,
    },
    Admitted {
        _original: Arc<OwnedSemaphorePermit>,
        transport: Arc<OriginalAdmittedTransportCustody>,
    },
}
impl EnrollmentExecutionLease {
    fn borrow_execution(
        &self,
    ) -> Result<Option<super::ceremony_tracker::OriginalRegisteredExecutionBorrow>, AuraError> {
        match self {
            Self::Registered(original) => original.borrow_execution().map(Some),
            Self::Exclusive { .. } | Self::Admitted { .. } => Ok(None),
        }
    }
    fn require_active(&self) -> Result<(), AuraError> {
        match self {
            Self::Registered(original) => original.require_active(),
            Self::Exclusive { .. } | Self::Admitted { .. } => Ok(()),
        }
    }
}

/// A child retains both the original persisted window and its admission lease.
/// There is no raw-budget extraction or caller-supplied checkpoint constructor.
pub(crate) struct EnrollmentExecutionChild {
    active: TimeoutBudget,
    original: TimeoutBudget,
    checkpoint: Arc<WindowCheckpoint>,
    _lease: EnrollmentExecutionLease,
}

/// One fresh acknowledged allocation owns all attenuated execution and its
/// dedicated child subtree. Serialized clock phases cannot construct this root.
pub(crate) struct EnrollmentExecutionRoot<Origin> {
    child: EnrollmentExecutionChild,
    children: crate::task_registry::TaskGroup,
    origin: Arc<Origin>,
    signing_terminal: RetainedSigningTerminalCustody,
}

impl EnrollmentExecutionRoot<AdmittedEnrollmentManifest> {
    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "original_enrollment_transport_root",
        receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, family = "runtime_helper")]
    pub(crate) fn observe_original_execution_failure(&self) -> Result<(), AuraError> {
        let EnrollmentExecutionLease::Admitted { transport, .. } = &self.child._lease else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        transport.observe_execution_failure();
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated",
        capability = "original_enrollment_transport_root",
        receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, family = "runtime_helper")]
    pub(crate) async fn settle_original_transport(
        &self,
    ) -> Result<Option<OriginalAdmittedAckFailure>, AuraError> {
        let EnrollmentExecutionLease::Admitted { transport, .. } = &self.child._lease else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        Ok(transport.settle().await)
    }
}

impl EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability> {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_registered_enrollment_root",
        receiver_type = EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        family = "runtime_helper"
    )]
    pub(crate) fn retain_issuer_acknowledgment(
        &self,
        acknowledged: &mut Option<AcknowledgedIssuerEnrollmentWindow>,
    ) -> Result<(), AuraError> {
        self.origin.retain_issuer_acknowledgment(acknowledged)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_registered_enrollment_root",
        receiver_type = EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        family = "runtime_helper"
    )]
    pub(crate) fn prepare_running_observation(
        &self,
        effects: &AuraEffectSystem,
        generation: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
        retirement_destination: Arc<
            crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
        >,
    ) -> Result<super::ceremony_tracker::PreparedRunningIssuerObservation, AuraError> {
        self.origin.prepare_running_observation(
            self.original_issuer_observer(effects)?,
            self.children().clone(),
            generation,
            retirement_destination,
        )
    }

    pub(crate) fn original_issuer_observer(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<HeldIssuerCompletionObserver, AuraError> {
        self.origin.require_effects(effects)?;
        let original = self.origin.completion_observation();
        if !self
            .child
            .active
            .shares_observation_owner_with(original.budget())
        {
            return Err(DurableEnrollmentExecutionError::ForeignProvider.refusal());
        }
        Ok(HeldIssuerCompletionObserver {
            active: self.child.active.clone(),
            original,
        })
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_enrollment_terminal_root",
        receiver_type = EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_issuer_terminal(
        &self,
        effects: &AuraEffectSystem,
        destination: &super::issuer_terminal_destination::OriginalIssuerTerminalDestination,
        terminal: &mut Option<crate::handlers::invitation::IssuerEnrollmentTerminalReceipt>,
        acknowledged: &mut Option<AcknowledgedIssuerEnrollmentWindow>,
    ) -> Result<(), AuraError> {
        if acknowledged.is_some() {
            return Err(AuraError::invalid(
                "original issuer acknowledgment is already retained",
            ));
        }
        let _original_execution = self.child._lease.borrow_execution()?;
        let retained_terminal = terminal
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original issuer terminal custody is missing"))?;
        self.child.require_execution_effects(effects)?;
        retained_terminal.require_runtime_owner(effects)?;
        self.origin
            .require_issued_notice_control(retained_terminal.retained())?;
        destination.require_original_protocol_completion(retained_terminal.retained())?;
        let sealed = self.children.seal_admission();
        let result = aura_core::time::timeout::execute_with_timeout_budget(
            effects,
            &self.child.active,
            || async {
                let disposal = sealed.await_disposal().await;
                disposal.require_original_group(&self.children)?;
                if let Some(source) = disposal.failure() {
                    return Err(AuraError::Internal {
                        message: "original issuer child disposal failed".into(),
                        source: Some(Arc::new(source.clone())),
                    });
                }
                match disposal.execution_outcome() {
                    crate::task_registry::TaskGroupExecutionOutcome::Completed
                    | crate::task_registry::TaskGroupExecutionOutcome::NotExecuted => {}
                    crate::task_registry::TaskGroupExecutionOutcome::Interrupted(source) => {
                        return Err(AuraError::Internal {
                            message: "original issuer child execution was interrupted".into(),
                            source: Some(Arc::new(source.clone())),
                        });
                    }
                }
                Ok(())
            },
        )
        .await;
        match result {
            Ok(()) => {}
            Err(TimeoutRunError::Operation(source)) => return Err(source),
            Err(TimeoutRunError::Timeout(source)) => return Err(AuraError::from(source)),
        }
        // Disposal runs before entering the ACK observation gate: descendants
        // may still need the same original gate until their actual completion.
        // Only the shared resource-ACK boundary may expose terminal authority.
        aura_core::time::timeout::acknowledge_with_timeout_budget(
            effects,
            &self.child.active,
            || async {
                self.origin
                    .acknowledge_terminal_checkpoint()
                    .await
                    .map_err(TimeoutBudgetError::checkpoint_failure)
            },
            |()| {
                // Both slots belong to the same retained custodian. This move is
                // synchronous with the original checkpoint acknowledgment gate;
                // cancellation never owns the receipt in an async frame.
                let terminal = terminal.take().ok_or_else(|| {
                    AuraError::invalid("original issuer terminal custody was lost")
                })?;
                *acknowledged = Some(AcknowledgedIssuerEnrollmentWindow {
                    terminal,
                    original: Arc::clone(&self.origin),
                });
                Ok(())
            },
        )
        .await
        .map_err(AuraError::from)?
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentGenerationCapability",
        capability_type = crate::runtime::effects::RegisteredEnrollmentGenerationCapability,
        family = "runtime_helper"
    )]
    pub(crate) fn bind_original_registered_generation(
        &mut self,
        registered: &crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'_>,
    ) -> Result<(), AuraError> {
        registered.require_original_execution_root(self)?;
        self.child.checkpoint = Arc::new(WindowCheckpoint::Registered {
            capability: Arc::clone(&self.origin),
        });
        Ok(())
    }
    /// Consume the actual fresh issuer Pending acknowledgment. The tracker
    /// calls this only after the shared original-endpoint publication boundary.
    pub(super) fn from_issuer_birth(
        birth: super::ceremony_tracker::AcknowledgedIssuerEnrollmentBirth,
        original_tasks: &crate::task_registry::TaskGroup,
    ) -> Self {
        let original = Arc::new(birth.into_original_window());
        Self {
            child: EnrollmentExecutionChild {
                active: original.budget().clone(),
                original: original.budget().clone(),
                _lease: EnrollmentExecutionLease::Registered(original.lease()),
                checkpoint: Arc::new(WindowCheckpoint::HeldIssuer {
                    capability: original.clone(),
                }),
            },
            signing_terminal: RetainedSigningTerminalCustody::fresh(),
            children: original_tasks.group("original-issuer-enrollment-children"),
            origin: original,
        }
    }
}

/// Actual terminal storage acknowledgment retaining the original signed result.
/// A Closed record read on restart cannot recreate this process-local authority.
pub(crate) struct AcknowledgedIssuerEnrollmentWindow {
    original: Arc<RegisteredEnrollmentWindowCapability>,
    terminal: crate::handlers::invitation::IssuerEnrollmentTerminalReceipt,
}

#[derive(Debug, thiserror::Error)]
#[error("acknowledged issuer original root mismatch")]
struct EnrollmentIssuerAcknowledgmentOriginMismatch;

/// Allowed shortcut purpose selected by the enclosing issuance operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnrolledDeviceShortcutPurpose {
    Contact,
    Guardian,
}
/// One-use original positive enrollment handoff. Identity is borrowed from the
/// actual retained signed receipt, never reconstructed from broker metadata.
pub(crate) struct EnrolledDeviceShortcutAuthority {
    acknowledged: AcknowledgedIssuerEnrollmentWindow,
    purpose: EnrolledDeviceShortcutPurpose,
}
impl EnrolledDeviceShortcutAuthority {
    pub(crate) fn subject(&self) -> aura_core::AuthorityId {
        self.acknowledged.terminal.retained().manifest().subject
    }
    pub(crate) fn device(&self) -> aura_core::DeviceId {
        self.acknowledged
            .terminal
            .retained()
            .manifest()
            .invitee_device
    }
    pub(crate) fn original_invitation(&self) -> &aura_core::InvitationId {
        &self.acknowledged.terminal.retained().manifest().invitation
    }
    pub(crate) fn purpose(&self) -> EnrolledDeviceShortcutPurpose {
        self.purpose
    }
}

impl AcknowledgedIssuerEnrollmentWindow {
    pub(super) fn into_shortcut_authority(
        self,
        purpose: EnrolledDeviceShortcutPurpose,
    ) -> EnrolledDeviceShortcutAuthority {
        EnrolledDeviceShortcutAuthority {
            acknowledged: self,
            purpose,
        }
    }

    pub(super) fn require_original(
        &self,
        original: &Arc<RegisteredEnrollmentWindowCapability>,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.original, original) {
            return Err(AuraError::PermissionDenied {
                message: "acknowledged issuer belongs to another original root".into(),
                source: Some(Arc::new(EnrollmentIssuerAcknowledgmentOriginMismatch)),
            });
        }
        original.require_issued_notice_control(self.terminal.retained())
    }
    pub(crate) fn positive(&self) -> bool {
        self.terminal.positive()
    }
}

impl<Origin> Drop for EnrollmentExecutionRoot<Origin> {
    fn drop(&mut self) {
        // Eviction, abandoned handoff and dropped terminal observations retain
        // Pending/Closed refusal; cancellation grants no terminal ACK.
        self.children.request_cancellation();
    }
}

impl<Origin> EnrollmentExecutionRoot<Origin> {
    pub(crate) fn child(&self) -> &EnrollmentExecutionChild {
        &self.child
    }

    pub(crate) fn children(&self) -> &crate::task_registry::TaskGroup {
        &self.children
    }

    pub(crate) fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Share admitted domain evidence without transferring execution-root or
    /// terminal custody. Owned descendants remain in this root's task subtree.
    pub(crate) fn origin_observation(&self) -> Arc<Origin> {
        Arc::clone(&self.origin)
    }

    /// Give an owned descendant the same already-selected absolute endpoint.
    /// This grants no terminal authority and performs no allocation or renewal.
    pub(crate) fn owned_execution_child(&self) -> EnrollmentExecutionChild {
        EnrollmentExecutionChild {
            active: self.child.active.clone(),
            original: self.child.original.clone(),
            checkpoint: Arc::clone(&self.child.checkpoint),
            _lease: self.child._lease.clone(),
        }
    }
}

impl EnrollmentExecutionRoot<AdmittedEnrollmentManifest> {
    fn require_original_domain_invitation(
        &self,
        invitation: &aura_core::invitation::Invitation,
    ) -> Result<(), AuraError> {
        let encode = |value| {
            aura_core::util::serialization::to_vec(value).map_err(|source| AuraError::Internal {
                message: "encode original enrollment domain binding".into(),
                source: Some(Arc::new(source)),
            })
        };
        if encode(invitation)? != encode(self.origin().canonical_invitation())? {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_enrollment_terminal_root",
        receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_confirmation(
        &mut self,
        effects: &AuraEffectSystem,
        proof: &crate::handlers::invitation::VerifiedEnrollmentConfirmation,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_original_domain_invitation(proof.canonical_invitation())?;
        if proof.manifest_digest() != self.origin().manifest_digest() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let digest = proof
            .terminal_frame_digest()
            .map_err(|source| AuraError::Internal {
                message: "bind original committed terminal proof".into(),
                source: Some(Arc::new(source)),
            })?;
        self.close_acknowledged(effects, true, AdmittedTerminalKind::Committed, digest, None)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_enrollment_terminal_root",
        receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_failure(
        &mut self,
        effects: &AuraEffectSystem,
        proof: &crate::handlers::invitation::VerifiedEnrollmentFailureCapability,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_original_domain_invitation(proof.canonical_invitation())?;
        if proof.manifest_digest() != self.origin().manifest_digest() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let digest = proof
            .terminal_frame_digest()
            .map_err(|source| AuraError::Internal {
                message: "bind original negative terminal proof".into(),
                source: Some(Arc::new(source)),
            })?;
        self.close_acknowledged(effects, false, AdmittedTerminalKind::Failed, digest, None)
            .await
    }

    #[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "original_enrollment_terminal_root",
    receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
    family = "runtime_helper"
)]
    pub(crate) async fn acknowledge_original_notice_failure(
        &mut self,
        effects: &AuraEffectSystem,
        proof: &crate::handlers::invitation::VerifiedEnrollmentFailureCapability,
        disposal: &crate::handlers::invitation::AcknowledgedOriginalNoticeDisposal,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_original_domain_invitation(proof.canonical_invitation())?;
        if proof.manifest_digest() != self.origin().manifest_digest() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let digest = proof
            .terminal_frame_digest()
            .map_err(|source| AuraError::Internal {
                message: "bind original reconciled notice failure".into(),
                source: Some(Arc::new(source)),
            })?;
        self.close_acknowledged(
            effects,
            false,
            AdmittedTerminalKind::Failed,
            digest,
            Some(disposal),
        )
        .await
    }

    async fn close_acknowledged(
        &mut self,
        effects: &AuraEffectSystem,
        positive: bool,
        kind: AdmittedTerminalKind,
        digest: [u8; 32],
        notice: Option<&crate::handlers::invitation::AcknowledgedOriginalNoticeDisposal>,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.child.require_execution_effects(effects)?;
        // Native failures remain retained for original task settlement. Their
        // borrows must dispose before a durable Closed receipt can be issued.
        let _transport_disposition = self.settle_original_transport().await?;
        let sealed = self.children.seal_admission();
        let result = aura_core::time::timeout::execute_with_timeout_budget(
            effects,
            &self.child.active,
            || async {
                let disposal = sealed.await_disposal().await;
                disposal
                    .require_original_group(&self.children)
                    .map_err(|source| AuraError::Internal {
                        message: "original enrollment child disposal belongs to another subtree"
                            .into(),
                        source: Some(Arc::new(source)),
                    })?;
                // Only an eligible, actually closed original notice VM with
                // exact required-task ACK can reconcile its own interruption.
                // The registry retains the original exit and native failure.
                let reconciled_notice = if let Some(notice) = notice {
                    if positive || kind != AdmittedTerminalKind::Failed {
                        return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                    }
                    notice.require_original(&digest, &disposal)?;
                    true
                } else {
                    false
                };
                if !reconciled_notice {
                    if let Some(source) = disposal.failure() {
                        return Err(AuraError::Internal {
                            message: "original enrollment child disposal failed".into(),
                            source: Some(Arc::new(source.clone())),
                        });
                    }
                    match disposal.execution_outcome() {
                        crate::task_registry::TaskGroupExecutionOutcome::Interrupted(source) => {
                            return Err(AuraError::Internal {
                                message: "original enrollment child execution was interrupted"
                                    .into(),
                                source: Some(Arc::new(source.clone())),
                            });
                        }
                        crate::task_registry::TaskGroupExecutionOutcome::NotExecuted => {
                            if positive {
                                return Err(
                                    DurableEnrollmentExecutionError::NoCompletedChild.refusal()
                                );
                            }
                        }
                        crate::task_registry::TaskGroupExecutionOutcome::Completed => {}
                    }
                }
                let WindowCheckpoint::Admitted {
                    binding,
                    writes,
                    terminal,
                    ..
                } = self.child.checkpoint.as_ref()
                else {
                    return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                };
                let binding = binding.as_ref();
                let _write = writes.lock().await;
                {
                    let mut claim = terminal.lock().await;
                    match claim.as_ref() {
                        Some(current)
                            if current.kind != kind
                                || current.digest != digest
                                || current.issued =>
                        {
                            return Err(DurableEnrollmentExecutionError::AlreadyBorn.refusal());
                        }
                        Some(_) => {}
                        None => {
                            *claim = Some(AdmittedTerminalClaim {
                                kind,
                                digest,
                                issued: false,
                            });
                        }
                    }
                }
                let anchor =
                    EnrollmentExecutionChild::read_admitted_anchor(effects, binding).await?;
                if anchor.execution != DurableEnrollmentExecutionState::Pending
                    || anchor.budget.started_at_ms() != self.child.original.started_at_ms()
                    || anchor.budget.deadline_at_ms() != self.child.original.deadline_at_ms()
                {
                    return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                }
                let retained = EnrollmentExecutionChild::read_bound_admitted_checkpoint(
                    effects,
                    binding,
                    &self.child.original,
                )
                .await
                .map_err(AuraError::from)?;
                let now = effects.physical_time().await.map_err(AuraError::from)?;
                self.child
                    .active
                    .remaining_at(&now)
                    .map_err(AuraError::from)?;
                if let DurableEnrollmentExecutionState::ClosedV3 {
                    acknowledged_at_ms,
                    kind: retained_kind,
                    digest: retained_digest,
                } = retained.execution
                {
                    if acknowledged_at_ms < binding.admitted_at_ms
                        || acknowledged_at_ms >= self.child.original.deadline_at_ms()
                        || now.ts_ms < acknowledged_at_ms
                        || retained_kind != kind
                        || retained_digest != digest
                    {
                        return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                    }
                    let frozen_budget = serde_json::to_vec(&retained.budget).map_err(|source| {
                        AuraError::Internal {
                            message: "encode recovered original enrollment budget".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                    return Ok(AcknowledgedEnrollmentWindow {
                        binding: binding.clone(),
                        frozen_budget,
                        acknowledged_at_ms,
                    });
                }
                if retained.execution != DurableEnrollmentExecutionState::Pending {
                    // Legacy Closed has no authenticated original timestamp;
                    // it cannot recreate a move-only terminal acknowledgment.
                    return Err(DurableEnrollmentExecutionError::AlreadyBorn.refusal());
                }
                let record = StoredAdmittedWindow {
                    binding: binding.clone(),
                    budget: self.child.original.clone(),
                    execution: DurableEnrollmentExecutionState::ClosedV3 {
                        acknowledged_at_ms: now.ts_ms,
                        kind,
                        digest,
                    },
                };
                let bytes = serde_json::to_vec(&record).map_err(|source| AuraError::Internal {
                    message: "encode original enrollment terminal checkpoint".into(),
                    source: Some(Arc::new(source)),
                })?;
                let frozen: StoredAdmittedWindow =
                    serde_json::from_slice(&bytes).map_err(|source| AuraError::Internal {
                        message: "freeze original enrollment terminal checkpoint".into(),
                        source: Some(Arc::new(source)),
                    })?;
                let key = admitted_location("admitted_enrollment_clock_checkpoint_v2", binding);
                effects
                    .secure_store(&key, &bytes, &[SecureStorageCapability::Write])
                    .await?;
                let stored = effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await?;
                if stored != bytes {
                    return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                }
                let frozen_budget =
                    serde_json::to_vec(&frozen.budget).map_err(|source| AuraError::Internal {
                        message: "encode acknowledged original enrollment budget".into(),
                        source: Some(Arc::new(source)),
                    })?;
                Ok(AcknowledgedEnrollmentWindow {
                    binding: binding.clone(),
                    frozen_budget,
                    acknowledged_at_ms: now.ts_ms,
                })
            },
        )
        .await;
        match result {
            Ok(acknowledged) => {
                let WindowCheckpoint::Admitted { terminal, .. } = self.child.checkpoint.as_ref()
                else {
                    return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                };
                // No await may intervene between marking the original claim
                // issued and returning its single move-owned acknowledgment.
                let mut claim = terminal.try_lock().map_err(|source| AuraError::Internal {
                    message: "original terminal handoff is already owned".into(),
                    source: Some(Arc::new(source)),
                })?;
                let current = claim
                    .as_mut()
                    .ok_or_else(|| DurableEnrollmentExecutionError::ForeignAdmission.refusal())?;
                if current.kind != kind || current.digest != digest || current.issued {
                    return Err(DurableEnrollmentExecutionError::AlreadyBorn.refusal());
                }
                current.issued = true;
                Ok(acknowledged)
            }
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(source)) => Err(AuraError::from(source)),
        }
    }
}

/// Actual original provider acknowledgment of a newly published pending birth.
/// It is neither reconstructible from retained bytes nor transferable to another
/// physical provider. The invitation owner must retain its move-owned custody.
pub(crate) struct AcknowledgedAdmittedEnrollmentBirth<'provider> {
    effects: &'provider AuraEffectSystem,
    binding: AdmittedWindowBinding,
    budget: TimeoutBudget,
}

impl AcknowledgedAdmittedEnrollmentBirth<'_> {
    pub(crate) fn require_original(
        &self,
        effects: &AuraEffectSystem,
        admitted: &AdmittedEnrollmentManifest,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects, effects) {
            return Err(DurableEnrollmentExecutionError::ForeignProvider.refusal());
        }
        if self.binding != EnrollmentExecutionChild::admitted_binding(admitted) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        if self.binding.device != effects.device_id() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        Ok(())
    }

    /// Consume the actual fresh provider acknowledgment while attaching its
    /// original runtime and independently admitted physical invitation owner.
    pub(crate) fn into_root(
        self,
        effects: Arc<AuraEffectSystem>,
        admitted: AdmittedEnrollmentManifest,
        original_tasks: &crate::task_registry::TaskGroup,
    ) -> Result<EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, AuraError> {
        let (lease, transport) =
            effects.acquire_original_admitted_window_owner(&self, &admitted)?;
        lease.require_effects(effects.as_ref())?;
        let lease = lease.into_permit();
        Ok(EnrollmentExecutionRoot {
            child: EnrollmentExecutionChild {
                active: self.budget.clone(),
                original: self.budget,
                checkpoint: Arc::new(WindowCheckpoint::Admitted {
                    effects,
                    binding: Box::new(self.binding),
                    writes: Mutex::new(()),
                    terminal: Mutex::new(None),
                }),
                _lease: EnrollmentExecutionLease::Admitted {
                    _original: Arc::new(lease),
                    transport,
                },
            },
            signing_terminal: RetainedSigningTerminalCustody::fresh(),
            children: original_tasks.group("original-enrollment-children"),
            origin: Arc::new(admitted),
        })
    }
}
/// Move-only acknowledgment of the original consuming terminal closure.
/// Domain evidence and child disposal were checked before the Closed write;
/// frozen bytes do not share mutable state with later observations.
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
enum WindowCheckpoint {
    HeldIssuer {
        capability: Arc<RegisteredEnrollmentWindowCapability>,
    },
    ApprovedSigning {
        capability: Arc<signing_checkpoint::ApprovedSigningCheckpoint>,
    },
    Registered {
        capability: Arc<RegisteredEnrollmentWindowCapability>,
    },
    Admitted {
        effects: Arc<AuraEffectSystem>,
        binding: Box<AdmittedWindowBinding>,
        writes: Mutex<()>,
        terminal: Mutex<Option<AdmittedTerminalClaim>>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum AdmittedTerminalKind {
    Committed,
    Failed,
}
struct AdmittedTerminalClaim {
    kind: AdmittedTerminalKind,
    digest: [u8; 32],
    issued: bool,
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
    execution: DurableEnrollmentExecutionState,
}

/// Persisted observation of original execution custody, never an owner token.
/// Absence is a codec error; obsolete records cannot become fresh allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum DurableEnrollmentExecutionState {
    Allocated,
    Pending,
    Closed,
    /// V3 positive terminal: timestamp is authenticated in the same selected
    /// provider write as Closed, so interrupted readback can recover it.
    ClosedV3 {
        acknowledged_at_ms: u64,
        kind: AdmittedTerminalKind,
        digest: [u8; 32],
    },
    Refused,
}

#[derive(Debug, thiserror::Error)]
enum DurableEnrollmentExecutionError {
    #[error("retained enrollment receipt has no original closed checkpoint")]
    MissingTerminalCheckpoint,
    #[error("original enrollment subtree has no completed child execution")]
    NoCompletedChild,
    #[error("original enrollment execution belongs to another physical provider")]
    ForeignProvider,
    #[error("original enrollment birth belongs to another admitted invitation")]
    ForeignAdmission,
    #[error("original enrollment allocation already has a durable birth")]
    AlreadyBorn,
}

impl DurableEnrollmentExecutionError {
    fn refusal(self) -> AuraError {
        AuraError::PermissionDenied {
            message: "original enrollment execution custody refused".into(),
            source: Some(Arc::new(self)),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAdmittedAnchor {
    version: u16,
    window: StoredAdmittedWindow,
}
// The locator belongs to the original allocation, independently of its codec
// version. Keeping it stable ensures obsolete bytes are refused rather than
// becoming an apparently absent allocation eligible for a second admission.
fn admitted_location(namespace: &str, binding: &AdmittedWindowBinding) -> SecureStorageLocation {
    SecureStorageLocation::new(namespace, binding.ceremony.to_string())
}
impl EnrollmentExecutionChild {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerCancellationTerminal", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    pub(crate) async fn retire_original_cancelled_generation(
        &self,
        effects: &Arc<AuraEffectSystem>,
        terminal: &Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        destination: &Arc<crate::handlers::invitation_service::OriginalIssuerTerminalDestination>,
        runner: &super::ceremony_runner::CeremonyRunner,
    ) -> crate::core::AgentResult<()> {
        self.require_execution_effects(effects.as_ref())?;
        self.require_issued_notice_owner(terminal.issued())?;
        let WindowCheckpoint::Registered { capability } = self.checkpoint.as_ref() else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission
                .refusal()
                .into());
        };
        capability.require_original_cancellation(terminal)?;
        destination.require_original_cancelled_terminal(terminal)?;
        if destination.original_cancelled_retirement_acknowledged(capability, terminal)? {
            return Ok(());
        }
        let _work = self._lease.borrow_execution()?;
        let actual = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            capability.negative_cleanup_budget(),
            terminal,
            || async {
                let writes = runner
                    .retire_failed_enrollment_generation(&terminal.issued().manifest().ceremony)
                    .await;
                destination
                    .retain_original_cancelled_retirement(self, terminal, writes)
                    .map_err(|source| {
                        crate::core::AgentError::from(AuraError::Internal {
                            message: "retain original required retirement".into(),
                            source: Some(Arc::new(source)),
                        })
                    })
            },
        )
        .await;
        let completed = match actual.operation {
            Some(Err(source)) => return Err(source),
            Some(Ok(publication)) => {
                if !destination.original_cancelled_retirement_acknowledged(capability, terminal)? {
                    return Err(crate::core::AgentError::invalid(
                        "accepted original retirement publication lacks its actual acknowledgment",
                    ));
                }
                // The actual destination retains this first cause independently
                // for the failed domain outcome. Physical retirement progress
                // must still allow its signed-terminal and VM cleanup to run.
                let _retained_domain_failure = publication.into_retained_failure();
                true
            }
            None => false,
        };
        actual
            .validation
            .map_err(|source| crate::core::AgentError::from(AuraError::from(source)))?;
        if !completed {
            return Err(crate::core::AgentError::invalid(
                "original cancelled retirement did not execute",
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalRetiredConditionalTransportCustody", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    pub(crate) async fn execute_original_retired_conditional_transport<'owner, F, Fut, T>(
        &self,
        custody: &'owner super::threshold_signing::OriginalRetiredConditionalTransportCustody,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'owner super::threshold_signing::OriginalRetiredConditionalTransportCustody,
            T,
            AuraError,
        >,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, AuraError>>,
    {
        custody.require_original_window(self)?;
        self.require_execution_effects(custody.effects().as_ref())?;
        let cleanup = match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => capability.negative_cleanup_budget(),
            WindowCheckpoint::ApprovedSigning { capability } => {
                capability.negative_cleanup_budget()
            }
            _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        };
        let _work = self._lease.borrow_execution()?;
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                custody.effects().as_ref(),
                cleanup,
                custody,
                operation,
            )
            .await,
        )
    }
    fn require_execution_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self._lease.require_active()?;
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability }
            | WindowCheckpoint::HeldIssuer { capability } => capability.require_effects(effects),
            WindowCheckpoint::ApprovedSigning { capability } => capability.require_effects(effects),
            WindowCheckpoint::Admitted {
                effects: original, ..
            } => {
                if std::ptr::eq(original.as_ref(), effects) {
                    Ok(())
                } else {
                    Err(DurableEnrollmentExecutionError::ForeignProvider.refusal())
                }
            }
        }
    }

    /// Observe the exact owned task group under this sealed original clock.
    /// Raw timeout access is confined to the window implementation.
    pub(crate) async fn wait_owned_group(
        &self,
        effects: &AuraEffectSystem,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<(), crate::task_registry::TaskSupervisionError> {
        use crate::task_registry::TaskSupervisionError;
        let _original_execution =
            self._lease
                .borrow_execution()
                .map_err(|source| TaskSupervisionError::Budget {
                    group: group.name().into(),
                    source: Box::new(TimeoutBudgetError::checkpoint_failure(source)),
                })?;
        self.require_execution_effects(effects)
            .map_err(|source| TaskSupervisionError::Budget {
                group: group.name().into(),
                source: Box::new(TimeoutBudgetError::checkpoint_failure(source)),
            })?;
        let outcome = self
            .execute(effects, || async {
                // active is an original-owned child, so attenuation is retained.
                group.wait_with_original_budget(effects, &self.active).await
            })
            .await;
        match outcome {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(source)) => {
                let failure = match source {
                    source @ TimeoutBudgetError::DeadlineExceeded { .. } => {
                        TaskSupervisionError::Timeout {
                            group: group.name().into(),
                            active_tasks: group.active_tasks(),
                            source: Box::new(source),
                        }
                    }
                    source => TaskSupervisionError::Budget {
                        group: group.name().into(),
                        source: Box::new(source),
                    },
                };
                group.request_cancellation();
                match group.abort_remaining() {
                    Ok(()) => Err(failure),
                    Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        ..
                    }) => Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        cause: Some(Box::new(failure)),
                    }),
                    Err(source) => Err(source),
                }
            }
        }
    }

    pub(crate) async fn shutdown_owned_group(
        &self,
        effects: &AuraEffectSystem,
        group: &crate::task_registry::TaskGroup,
    ) -> Result<(), crate::task_registry::TaskSupervisionError> {
        group.request_cancellation();
        self.wait_owned_group(effects, group).await
    }
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
    ) -> Result<(), AuraError> {
        let full = binding
            .expires_at_ms
            .checked_sub(binding.admitted_at_ms)
            .ok_or_else(|| AuraError::invalid("invalid original admitted interval"))?;
        if record.binding != *binding
            || record.budget.started_at_ms() != binding.admitted_at_ms
            || record.budget.timeout_ms() != full
        {
            return Err(AuraError::from(
                TimeoutBudgetError::CheckpointDiscontinuity {
                    detail: "admitted clock differs from protected original interval".into(),
                },
            ));
        }
        if let DurableEnrollmentExecutionState::ClosedV3 {
            acknowledged_at_ms, ..
        } = record.execution
        {
            if acknowledged_at_ms < binding.admitted_at_ms
                || acknowledged_at_ms >= binding.expires_at_ms
            {
                return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
            }
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
        if anchor.version != 3 {
            return Err(AuraError::invalid("unsupported admitted anchor"));
        }
        Self::validate_admitted_record(&anchor.window, binding)?;
        Ok(anchor.window)
    }
    async fn publish_admitted_anchor(
        effects: &AuraEffectSystem,
        record: &StoredAdmittedWindow,
    ) -> Result<(), AuraError> {
        let anchor = StoredAdmittedAnchor {
            version: 3,
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
        let publication = effects
            .secure_store_immutable(
                &key,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        if publication != aura_core::effects::secure::ImmutableSecureStoreOutcome::Created {
            return Err(AuraError::PermissionDenied {
                message: "original admitted window birth was not freshly acknowledged".into(),
                source: Some(Arc::new(DurableEnrollmentExecutionError::AlreadyBorn)),
            });
        }
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
        let checkpoint_publication = effects
            .secure_create_mutable(
                &admitted_location("admitted_enrollment_clock_checkpoint_v2", &record.binding),
                &checkpoint,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        if checkpoint_publication
            != aura_core::effects::secure::ImmutableSecureStoreOutcome::Created
        {
            return Err(AuraError::PermissionDenied {
                message: "original admitted checkpoint birth was not freshly acknowledged".into(),
                source: Some(Arc::new(DurableEnrollmentExecutionError::AlreadyBorn)),
            });
        }
        let retained_checkpoint = Self::decode_admitted_record(
            effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &record.binding),
        )
        .await?;
        if retained_checkpoint.binding != record.binding
            || retained_checkpoint.execution != record.execution
        {
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

    /// Called only after a new explicit user-transfer admission is committed.
    /// Reimporting an existing admission never allocates a fresh clock owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "retain_new_admitted_window_enrollment_window",
        capability_type = NewEnrollmentAdmissionCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn retain_new_admitted_window<'provider>(
        effects: &'provider AuraEffectSystem,
        admission: NewEnrollmentAdmissionCapability<'_>,
    ) -> Result<AcknowledgedAdmittedEnrollmentBirth<'provider>, AuraError> {
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
        let record = StoredAdmittedWindow {
            binding,
            budget,
            execution: DurableEnrollmentExecutionState::Pending,
        };
        Self::publish_admitted_anchor(effects, &record).await?;
        Ok(AcknowledgedAdmittedEnrollmentBirth {
            effects,
            binding: record.binding,
            budget: record.budget,
        })
    }
    async fn require_retained_admitted_window(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<StoredAdmittedWindow, AuraError> {
        let binding = Self::admitted_binding(witness);
        let anchor = Self::read_admitted_anchor(effects, &binding).await?;
        let checkpoint = Self::decode_admitted_record(
            effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding),
        )
        .await?;
        if anchor.execution != DurableEnrollmentExecutionState::Pending
            || !matches!(
                checkpoint.execution,
                DurableEnrollmentExecutionState::Closed
                    | DurableEnrollmentExecutionState::ClosedV3 { .. }
            )
            || checkpoint.binding != binding
        {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        checkpoint
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        Ok(checkpoint)
    }
    fn require_retained_terminal_purpose(
        checkpoint: &StoredAdmittedWindow,
        expected: AdmittedTerminalKind,
        frame: &crate::handlers::invitation::EnrollmentControlFrame,
        acknowledged_at_ms: u64,
    ) -> Result<(), AuraError> {
        match checkpoint.execution {
            DurableEnrollmentExecutionState::Closed => Ok(()),
            DurableEnrollmentExecutionState::ClosedV3 {
                acknowledged_at_ms: stored_at,
                kind,
                digest,
            } if kind == expected && stored_at == acknowledged_at_ms => {
                let bytes = aura_core::util::serialization::to_vec(frame).map_err(|source| {
                    AuraError::Serialization {
                        message: "encode retained terminal frame binding".into(),
                        source: Some(Arc::new(source)),
                    }
                })?;
                if digest == aura_core::hash::hash(&bytes) {
                    Ok(())
                } else {
                    Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal())
                }
            }
            _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        }
    }
    /// Validate original clock evidence of a separately authenticated retained
    /// receipt. Later live expiry cannot rewrite immutable completed evidence.
    pub(crate) async fn verify_retained_confirmation_clock(
        effects: &AuraEffectSystem,
        retained: &crate::handlers::invitation::enrollment_manifest_admission::RetainedConfirmationAdmission,
    ) -> Result<(), AuraError> {
        let admitted = retained.admitted();
        let checkpoint = Self::require_retained_admitted_window(effects, admitted).await?;
        Self::require_retained_terminal_purpose(
            &checkpoint,
            AdmittedTerminalKind::Committed,
            retained.frame(),
            retained.acknowledged_at_ms(),
        )?;
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
        let checkpoint = Self::require_retained_admitted_window(effects, admitted).await?;
        Self::require_retained_terminal_purpose(
            &checkpoint,
            AdmittedTerminalKind::Failed,
            retained.frame(),
            retained.acknowledged_at_ms(),
        )?;
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

    async fn derive_pending_birth<T>(
        &self,
        effects: &AuraEffectSystem,
        derive: impl FnOnce(&PhysicalTime, Duration) -> Result<T, TimeoutBudgetError>,
    ) -> Result<T, TimeoutBudgetError> {
        let _original_execution = self
            ._lease
            .borrow_execution()
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        self.require_execution_effects(effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let result = aura_core::time::timeout::execute_with_timeout_budget(
            effects,
            &self.active,
            || async {
                self.require_pending_checkpoint(effects).await?;
                // The post-success observation in execute_with_timeout_budget
                // may advance this same clock before it returns. Derive the
                // result while the original observation lease still owns now.
                let _observation = self.active.acquire_observation().await;
                let now = effects
                    .physical_time()
                    .await
                    .map_err(TimeoutBudgetError::time_source_failure)?;
                let remaining = self.active.remaining_at(&now)?;
                derive(&now, remaining)
            },
        )
        .await;
        match result {
            Ok(derived) => Ok(derived),
            Err(TimeoutRunError::Operation(source) | TimeoutRunError::Timeout(source)) => {
                Err(source)
            }
        }
    }

    fn has_pending_birth(&self) -> bool {
        matches!(
            self.checkpoint.as_ref(),
            WindowCheckpoint::Admitted { .. }
                | WindowCheckpoint::HeldIssuer { .. }
                | WindowCheckpoint::Registered { .. }
                | WindowCheckpoint::ApprovedSigning { .. }
        )
    }

    async fn require_pending_checkpoint(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), TimeoutBudgetError> {
        if let WindowCheckpoint::ApprovedSigning { capability } = self.checkpoint.as_ref() {
            capability
                .require_effects(effects)
                .map_err(TimeoutBudgetError::checkpoint_failure)?;
            return capability
                .require_pending(&self.original)
                .await
                .map_err(TimeoutBudgetError::checkpoint_failure);
        }
        if let WindowCheckpoint::HeldIssuer { capability }
        | WindowCheckpoint::Registered { capability } = self.checkpoint.as_ref()
        {
            return capability
                .require_pending()
                .await
                .map_err(TimeoutBudgetError::checkpoint_failure);
        }
        let WindowCheckpoint::Admitted { binding, .. } = self.checkpoint.as_ref() else {
            return Err(TimeoutBudgetError::checkpoint_failure(
                DurableEnrollmentExecutionError::ForeignAdmission.refusal(),
            ));
        };
        let anchor = Self::read_admitted_anchor(effects, binding)
            .await
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let checkpoint =
            Self::read_bound_admitted_checkpoint(effects, binding, &self.original).await?;
        if anchor.execution != DurableEnrollmentExecutionState::Pending
            || checkpoint.execution != DurableEnrollmentExecutionState::Pending
            || anchor.budget.started_at_ms() != self.original.started_at_ms()
            || anchor.budget.deadline_at_ms() != self.original.deadline_at_ms()
        {
            return Err(TimeoutBudgetError::checkpoint_failure(
                DurableEnrollmentExecutionError::AlreadyBorn.refusal(),
            ));
        }
        Ok(())
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
    pub(crate) fn acknowledge_original_retirement(
        &self,
        writes: crate::runtime::effects::AcknowledgedEnrollmentRetirement,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<
        super::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment,
        super::ceremony_tracker::RejectedOriginalEnrollmentRetirement,
    > {
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.acknowledge_original_retirement(writes, issued)
            }
            _ => Err(
                super::ceremony_tracker::RejectedOriginalEnrollmentRetirement {
                    source: AuraError::from(TimeoutBudgetError::CheckpointDiscontinuity {
                        detail: "retirement requires original registered execution owner".into(),
                    }),
                    _writes: Box::new(writes),
                },
            ),
        }
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RetainedEnrollmentVmControl",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(crate) fn original_positive_decision(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<&super::ceremony_tracker::OriginalEnrollmentPositiveDecisionReceipt, AuraError>
    {
        self.require_issued_notice_owner(issued)?;
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.original_positive_decision(issued)
            }
            _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    pub(crate) fn original_negative_decision(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<&super::ceremony_tracker::OriginalEnrollmentNegativeDecisionReceipt, AuraError>
    {
        self.require_issued_notice_owner(issued)?;
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.original_negative_decision(issued)
            }
            _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    pub(crate) async fn await_original_terminal<'issued>(
        &self,
        effects: &AuraEffectSystem,
        issued: &'issued crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'issued crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
            aura_app::runtime_bridge::CeremonyTerminalOutcome,
            AuraError,
        >,
        AuraError,
    > {
        self.require_issued_notice_owner(issued)?;
        self.require_execution_effects(effects)?;
        let WindowCheckpoint::Registered { capability } = self.checkpoint.as_ref() else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        let _work = self._lease.borrow_execution()?;
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects,
                capability.negative_cleanup_budget(),
                issued,
                || capability.await_original_terminal(issued),
            )
            .await,
        )
    }
    pub(crate) fn require_activation_generation(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        activation: &super::ceremony_tracker::EnrollmentActivationCapability<'_>,
    ) -> Result<aura_core::Hash32, AuraError> {
        self.require_execution_effects(effects)?;
        match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.require_activation_generation(effects, issued, activation)
            }
            _ => Err(AuraError::permission_denied(
                "activation requires original registered execution",
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

    pub(crate) async fn remaining_ms(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<u64, TimeoutBudgetError> {
        self.derive_pending_birth(effects, |_, remaining| {
            u64::try_from(remaining.as_millis())
                .map_err(|source| TimeoutBudgetError::invalid_policy(source.to_string()))
        })
        .await
    }
    pub(crate) async fn child(
        &self,
        effects: &AuraEffectSystem,
        requested: Duration,
    ) -> Result<Self, TimeoutBudgetError> {
        let active = self
            .derive_pending_birth(effects, |now, _| self.active.child_budget(now, requested))
            .await?;
        Ok(Self {
            active,
            original: self.original.clone(),
            checkpoint: Arc::clone(&self.checkpoint),
            _lease: self._lease.clone(),
        })
    }
    pub(crate) async fn retry_delay(
        &self,
        effects: &AuraEffectSystem,
        delay_ms: u64,
    ) -> Result<(), TimeoutBudgetError> {
        let remaining = self.remaining_ms(effects).await?;
        if self.has_pending_birth() {
            let result = self
                .execute(effects, || async {
                    effects
                        .sleep_ms(delay_ms.min(remaining))
                        .await
                        .map_err(TimeoutBudgetError::time_source_failure)
                })
                .await;
            return match result {
                Ok(()) => self.remaining_ms(effects).await.map(|_| ()),
                Err(TimeoutRunError::Operation(source) | TimeoutRunError::Timeout(source)) => {
                    Err(source)
                }
            };
        }
        effects
            .sleep_ms(delay_ms.min(remaining))
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        self.remaining_ms(effects).await?;
        Ok(())
    }
    pub(crate) async fn execute<F, Fut, T, E>(
        &self,
        time: &AuraEffectSystem,
        operation: F,
    ) -> Result<T, TimeoutRunError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let _original_execution = self._lease.borrow_execution().map_err(|source| {
            TimeoutRunError::Timeout(TimeoutBudgetError::checkpoint_failure(source))
        })?;
        self.require_execution_effects(time).map_err(|source| {
            TimeoutRunError::Timeout(TimeoutBudgetError::checkpoint_failure(source))
        })?;
        if self.has_pending_birth() {
            // Fresh admitted or issuer birth is already durably Pending. Any crash or
            // dropped negative observation therefore refuses reconstruction;
            // execution needs no unbounded write after the original endpoint.
            let result = aura_core::time::timeout::execute_with_timeout_budget(
                time,
                &self.active,
                || async {
                    self.require_pending_checkpoint(time).await?;
                    Ok(operation().await)
                },
            )
            .await;
            return match result {
                Ok(result) => result.map_err(TimeoutRunError::Operation),
                Err(TimeoutRunError::Operation(source) | TimeoutRunError::Timeout(source)) => {
                    Err(TimeoutRunError::Timeout(source))
                }
            };
        }
        Err(TimeoutRunError::Timeout(
            TimeoutBudgetError::checkpoint_failure(
                DurableEnrollmentExecutionError::ForeignAdmission.refusal(),
            ),
        ))
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
    fn original_execution_custody_cannot_be_cloned_or_decoded() {
        trait AmbiguousIfClone<A> {
            fn absent() {}
        }
        struct Cloned;
        impl<T: ?Sized> AmbiguousIfClone<()> for T {}
        impl<T: Clone> AmbiguousIfClone<Cloned> for T {}
        trait AmbiguousIfDeserialize<A> {
            fn absent() {}
        }
        struct Decoded;
        impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
        impl<T: serde::Deserialize<'static>> AmbiguousIfDeserialize<Decoded> for T {}
        let _ =
            <EnrollmentExecutionRoot<AdmittedEnrollmentManifest> as AmbiguousIfClone<_>>::absent;
        let _ = <EnrollmentExecutionChild as AmbiguousIfClone<_>>::absent;
        let _ = <AcknowledgedAdmittedEnrollmentBirth<'static> as AmbiguousIfClone<_>>::absent;
        let _ = <AcknowledgedEnrollmentWindow as AmbiguousIfClone<_>>::absent;
        let _ = <EnrollmentExecutionRoot<AdmittedEnrollmentManifest> as AmbiguousIfDeserialize<
            _,
        >>::absent;
        let _ = <EnrollmentExecutionChild as AmbiguousIfDeserialize<_>>::absent;
        let _ = <AcknowledgedAdmittedEnrollmentBirth<'static> as AmbiguousIfDeserialize<_>>::absent;
        let _ = <AcknowledgedEnrollmentWindow as AmbiguousIfDeserialize<_>>::absent;
    }
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

    #[tokio::test]
    async fn original_pending_birth_derivatives_keep_monotone_postcheck() {
        #[derive(Debug)]
        enum CleanupFailure {
            OwnerStillShared,
            IncompleteShutdown,
            IncompleteCancellation,
            IncompleteDisposal,
            Cancellation(crate::core::AgentError),
            OriginalDisposal(AuraError),
            MissingFinalizer,
            Shutdown(crate::core::AgentError),
        }
        impl std::fmt::Display for CleanupFailure {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::OwnerStillShared => write!(f, "agent owner remains shared"),
                    Self::IncompleteShutdown => write!(f, "owned shutdown did not finish in bound"),
                    Self::IncompleteCancellation => {
                        write!(f, "original issuer cancellation did not finish in bound")
                    }
                    Self::IncompleteDisposal => {
                        write!(f, "original issuer disposal did not finish in bound")
                    }
                    Self::Cancellation(source) => {
                        write!(f, "typed original cancellation failure: {source}")
                    }
                    Self::OriginalDisposal(source) => {
                        write!(f, "typed original disposal failure: {source}")
                    }
                    Self::MissingFinalizer => write!(f, "original finalizer cause absent"),
                    Self::Shutdown(source) => write!(f, "typed shutdown failure: {source}"),
                }
            }
        }

        Box::pin(async {
        let clock = Arc::new(
            crate::handlers::invitation::tests::ReadAdvancingPhysicalClock::new(5_000),
        );
        let ((issuer, invitee, invitation, _start, _acceptance, _response), original) =
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                "pending-birth-postcheck-order",
                Some(clock.clone()),
                None,
            )
            .await;
        eprintln!("pending-birth diagnostic: signed fixture complete");
        let effects = invitee.runtime().effects();
        let root = invitee
            .invitations()
            .unwrap()
            .take_original_enrollment_execution(&invitation.invitation_id)
            .await
            .unwrap();
        eprintln!("pending-birth diagnostic: original root acquired");
        let original_deadline = root.child().original.deadline_at_ms();

        clock.arm();
        eprintln!("pending-birth diagnostic: first derivative");
        let primary = async {
            let remaining = root.child().remaining_ms(effects.as_ref()).await?;
            eprintln!("pending-birth diagnostic: remaining derived");
            let child = root
                .child()
                .child(effects.as_ref(), Duration::from_millis(25))
                .await?;
            eprintln!("pending-birth diagnostic: child derived");
            let child_deadline = child.active.deadline_at_ms();
            let child_timeout = child.active.timeout_ms();
            drop(child);
            let (first, second) = tokio::join!(
                root.child().remaining_ms(effects.as_ref()),
                root.child().remaining_ms(effects.as_ref())
            );
            first?;
            second?;
            Ok::<_, TimeoutBudgetError>((remaining, child_deadline, child_timeout))
        }
        .await;
        eprintln!("pending-birth diagnostic: primary returned");
        eprintln!("pending-birth diagnostic: primary={primary:?}");
        clock.disarm();
        let reversal = if primary.is_ok() {
            let previous = clock.now_ms();
            clock.set_time(previous - 1);
            let result = root.child().remaining_ms(effects.as_ref()).await;
            Some((previous, result))
        } else {
            None
        };
        drop(root);
        if let Some((previous, _)) = &reversal {
            // Test-controlled time can move forward for independent shutdown,
            // but the original child's latched refusal remains immutable.
            clock.set_time(*previous);
        }
        let original_disposal = original
            .original_disposal_for_test()
            .map_err(CleanupFailure::OriginalDisposal);
        let issuer_cancellation = match issuer.invitations() {
            Ok(service) => match tokio::time::timeout(
                Duration::from_secs(5),
                service.cancel(&invitation.invitation_id),
            )
            .await
            {
                Ok(result) => result.map(|_| ()).map_err(CleanupFailure::Cancellation),
                Err(_) => Err(CleanupFailure::IncompleteCancellation),
            },
            Err(source) => Err(CleanupFailure::Cancellation(source)),
        };
        eprintln!("pending-birth diagnostic: issuer_cancellation={issuer_cancellation:?}");
        let issuer_disposal = match original_disposal {
            Ok(observation) => match tokio::time::timeout(
                Duration::from_secs(5),
                observation.await_disposal(),
            )
            .await
            {
                Ok(disposed) => Ok(disposed.outcome().clone()),
                Err(_) => Err(CleanupFailure::IncompleteDisposal),
            },
            Err(source) => Err(source),
        };
        eprintln!("pending-birth diagnostic: issuer_disposal={issuer_disposal:?}");
        let issuer_finalizer = original
            .original_finalizer_failure_for_test()
            .map_err(CleanupFailure::OriginalDisposal)
            .and_then(|source| source.ok_or(CleanupFailure::MissingFinalizer));
        eprintln!("pending-birth diagnostic: issuer_finalizer={issuer_finalizer:?}");
        drop(original);
        let issuer_context = aura_core::context::EffectContext::new(
            issuer.authority_id(),
            aura_core::ContextId::new_from_entropy([153; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let invitee_context = aura_core::context::EffectContext::new(
            invitee.authority_id(),
            aura_core::ContextId::new_from_entropy([156; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let issuer_shutdown = match Arc::try_unwrap(issuer) {
            Ok(issuer) => match tokio::time::timeout(
                Duration::from_secs(5),
                issuer.shutdown(&issuer_context),
            )
            .await
            {
                Ok(result) => result.map_err(CleanupFailure::Shutdown),
                Err(_) => Err(CleanupFailure::IncompleteShutdown),
            },
            Err(_) => Err(CleanupFailure::OwnerStillShared),
        };
        eprintln!("pending-birth diagnostic: issuer_cleanup={issuer_shutdown:?}");
        let invitee_shutdown = match Arc::try_unwrap(invitee) {
            Ok(invitee) => match tokio::time::timeout(
                Duration::from_secs(5),
                invitee.shutdown(&invitee_context),
            )
            .await
            {
                Ok(result) => result.map_err(CleanupFailure::Shutdown),
                Err(_) => Err(CleanupFailure::IncompleteShutdown),
            },
            Err(_) => Err(CleanupFailure::OwnerStillShared),
        };
        eprintln!("pending-birth diagnostic: invitee_cleanup={invitee_shutdown:?}");

        let (remaining, child_deadline, child_timeout) = primary.unwrap_or_else(|source| {
            panic!(
                "pending-birth derivative failed: {source}; issuer cleanup={issuer_shutdown:?}; invitee cleanup={invitee_shutdown:?}"
            )
        });
        assert!(remaining > 0);
        assert!(child_deadline <= original_deadline);
        assert!(child_timeout <= 25);
        let (previous, rollback) = reversal.expect("positive derivatives reach reversal check");
        assert!(matches!(
            rollback,
            Err(TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms,
                observed_at_ms,
            }) if previous_observed_at_ms == previous && observed_at_ms == previous - 1
        ));
        issuer_cancellation.expect("original issuer cancellation before shutdown");
        let disposed = issuer_disposal.expect("original issuer disposal before shutdown");
        let aura_core::effects::task::RequiredTaskExit::Failed(ref native) = disposed else {
            panic!("original cancelled issuer must retain failed disposition: {disposed:?}");
        };
        let finalizer = issuer_finalizer.expect("original finalizer cause before shutdown");
        if let Err(CleanupFailure::Shutdown(ref source)) = issuer_shutdown {
            fn cause<'a, T: std::error::Error + 'static>(
                mut source: &'a (dyn std::error::Error + 'static),
            ) -> Option<&'a T> {
                loop {
                    if let Some(actual) = source.downcast_ref::<T>() {
                        return Some(actual);
                    }
                    source = source.source()?;
                }
            }
            let terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(native)
                .expect("actual original disposition retains typed terminal failure");
            type Admission = crate::handlers::invitation::EnrollmentVmAdmissionError;
            let cancelled_before_vm = matches!(
                terminal.execution(),
                aura_core::effects::task::RequiredTaskExit::Cancelled
            );
            let cancelled_after_vm = cause::<Admission>(&finalizer).is_some_and(|actual| {
                matches!(
                    actual,
                    Admission::TerminalFailed(
                        aura_app::runtime_bridge::CeremonyFailureReason::Cancelled
                    )
                )
            });
            assert!(
                cancelled_before_vm || cancelled_after_vm,
                "same original issuer retains a cancelled typed cause"
            );
            assert!(matches!(
                cause::<crate::runtime::system::RuntimeShutdownError>(source),
                Some(crate::runtime::system::RuntimeShutdownError::TaskTree(
                    crate::task_registry::TaskSupervisionError::TaskFailed { .. }
                ))
            ));
            let exact_terminal =
                cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(source)
                    .is_some_and(|actual| std::ptr::eq(actual, terminal));
            let finalizer_source = std::error::Error::source(&finalizer).unwrap_or(&finalizer);
            let mut current: Option<&(dyn std::error::Error + 'static)> = Some(source);
            let mut exact_finalizer = false;
            while let Some(actual) = current {
                if std::ptr::eq(actual, finalizer_source) {
                    exact_finalizer = true;
                    break;
                }
                current = actual.source();
            }
            assert!(
                exact_terminal || exact_finalizer,
                "shutdown must retain the exact original finalizer or terminal allocation"
            );
        } else {
            issuer_shutdown.expect("owned issuer shutdown or exact retained negative cause");
        }
        invitee_shutdown.expect("invitee cleanup after original rollback refusal");
        })
        .await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn obsolete_anchor_at_original_locator_cannot_allocate_a_replacement_window() {
        Box::pin(async {
            let (_issuer, invitee, invitation, _start, _acceptance, _response) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "obsolete-original-admitted-anchor",
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
            let binding = EnrollmentExecutionChild::admitted_binding(&witness);
            let key = admitted_location("admitted_enrollment_clock_anchor_v2", &binding);
            let original = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let checkpoint_key =
                admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding);
            let checkpoint = effects
                .secure_retrieve(&checkpoint_key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let mut obsolete: serde_json::Value = serde_json::from_slice(&original).unwrap();
            obsolete["version"] = serde_json::json!(2);
            let obsolete = serde_json::to_vec(&obsolete).unwrap();
            assert!(effects
                .fault_remove_secure_record_for_test(&key)
                .await
                .unwrap());
            effects
                .secure_store_immutable(&key, &obsolete, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            assert!(root.child().remaining_ms(effects.as_ref()).await.is_err());
            assert_eq!(
                effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                obsolete,
                "refusal preserves the original allocation rather than replacing its bytes"
            );
            assert_eq!(
                effects
                    .secure_retrieve(&checkpoint_key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                checkpoint,
                "refusal cannot rewrite the retained observation history"
            );
        })
        .await;
    }

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
            execution: DurableEnrollmentExecutionState::Allocated,
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
            EnrollmentExecutionChild::read_admitted_anchor(effects, binding)
                .await
                .is_err()
        );
        assert!(
            EnrollmentExecutionChild::require_retained_admitted_window(effects, witness)
                .await
                .is_err()
        );
        assert!(!effects.secure_exists(&anchor_key).await.unwrap());
    }
    #[tokio::test]
    async fn actual_admitted_pending_birth_never_repairs_lost_execution_records() {
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
            let binding = EnrollmentExecutionChild::admitted_binding(&witness);
            let anchor_key = admitted_location("admitted_enrollment_clock_anchor_v2", &binding);
            let checkpoint_key =
                admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding);
            let anchor = effects
                .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let root = invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .unwrap();
            let original_deadline = root.child().original.deadline_at_ms();
            root.child().remaining_ms(effects.as_ref()).await.unwrap();
            root.child().remaining_ms(effects.as_ref()).await.unwrap();
            assert_eq!(root.child().original.deadline_at_ms(), original_deadline);
            let retained_checkpoint = effects
                .secure_retrieve(&checkpoint_key, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let retained: StoredAdmittedWindow =
                serde_json::from_slice(&retained_checkpoint).unwrap();
            assert_eq!(retained.execution, DurableEnrollmentExecutionState::Pending);
            // Even the original held root refuses missing durable birth bytes.
            // No observed admission or retained phase can repair or recreate it.
            effects
                .secure_delete(&checkpoint_key, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
            assert!(EnrollmentExecutionChild::require_retained_admitted_window(
                effects.as_ref(),
                &witness
            )
            .await
            .is_err());
            assert!(root.child().remaining_ms(effects.as_ref()).await.is_err());
            assert_eq!(
                effects
                    .secure_retrieve(&anchor_key, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                anchor
            );
            drop(root);
            assert!(invitee
                .invitations()
                .unwrap()
                .take_original_enrollment_execution(&invitation.invitation_id)
                .await
                .is_err());
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
    fn original_signed_interval_rejects_clamped_reconstruction() {
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
            execution: DurableEnrollmentExecutionState::Allocated,
            budget: TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(100),
                Duration::from_millis(900_000),
            )
            .unwrap(),
        };
        let clamped = StoredAdmittedWindow {
            binding: binding.clone(),
            execution: DurableEnrollmentExecutionState::Allocated,
            budget: TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(100),
                Duration::from_millis(240_000),
            )
            .unwrap(),
        };
        EnrollmentExecutionChild::validate_admitted_record(&fresh, &binding).unwrap();
        assert!(EnrollmentExecutionChild::validate_admitted_record(&clamped, &binding).is_err());
    }
}

// Outside implementation slice: private to the existing enrollment-window module.
// No constructor accepts recovered bytes as execution or cleanup authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EnrollmentNegativeCleanupAllowance {
    milliseconds: u64,
}
impl EnrollmentNegativeCleanupAllowance {
    const DEFAULT: Self = Self {
        milliseconds: 30_000,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OriginalEnrollmentWindowPolicy {
    version: u16,
    execution_started_at_ms: u64,
    execution_deadline_at_ms: u64,
    negative_cleanup_allowance: EnrollmentNegativeCleanupAllowance,
    negative_cleanup_deadline_at_ms: u64,
}
impl OriginalEnrollmentWindowPolicy {
    pub(crate) fn require_cleanup(
        &self,
        execution: &TimeoutBudget,
        cleanup: &TimeoutBudget,
    ) -> Result<(), TimeoutBudgetError> {
        self.require_original(execution)?;
        if cleanup.started_at_ms() != self.execution_started_at_ms
            || cleanup.deadline_at_ms() != self.negative_cleanup_deadline_at_ms
        {
            return Err(TimeoutBudgetError::invalid_policy(
                "original negative cleanup interval changed",
            ));
        }
        Ok(())
    }
    // Called only from the private fresh allocation publication operation,
    // alongside the actual generation/profile/provider-bound protected record.
    pub(super) fn at_original_birth(execution: &TimeoutBudget) -> Result<Self, TimeoutBudgetError> {
        let allowance = EnrollmentNegativeCleanupAllowance::DEFAULT;
        let deadline = execution
            .deadline_at_ms()
            .checked_add(allowance.milliseconds)
            .ok_or_else(|| {
                TimeoutBudgetError::invalid_policy("original cleanup endpoint overflow")
            })?;
        Ok(Self {
            version: 1,
            execution_started_at_ms: execution.started_at_ms(),
            execution_deadline_at_ms: execution.deadline_at_ms(),
            negative_cleanup_allowance: allowance,
            negative_cleanup_deadline_at_ms: deadline,
        })
    }
    pub(super) fn require_original(
        &self,
        execution: &TimeoutBudget,
    ) -> Result<(), TimeoutBudgetError> {
        let original = Self::at_original_birth(execution)?;
        if *self != original {
            return Err(TimeoutBudgetError::CheckpointDiscontinuity {
                detail: "original cleanup allocation policy changed".into(),
            });
        }
        Ok(())
    }
}

// Actual negative authority consumes the existing root; see negative_owner.rs.

// Private allocation value before publication, not an execution authority. The
// caller must consume it inside the original protected Created/readback ACK.
pub(super) struct OriginalEnrollmentWindowBirth {
    pub(super) execution: TimeoutBudget,
    pub(super) shortcut: TimeoutBudget,
    pub(super) negative_cleanup: TimeoutBudget,
    pub(super) policy: OriginalEnrollmentWindowPolicy,
}

/// Fixed shortcut extension selected with the original issuer clock. A caller
/// may only request up to one existing enrollment execution interval.
pub(crate) struct OriginalShortcutWindowPolicy {
    extension: std::time::Duration,
}

impl OriginalShortcutWindowPolicy {
    pub(crate) fn bounded_extension(
        execution: std::time::Duration,
        extension: std::time::Duration,
    ) -> Result<Self, TimeoutBudgetError> {
        if extension.is_zero() || extension > execution {
            return Err(TimeoutBudgetError::invalid_policy(
                "shortcut extension must fit the enrollment execution policy",
            ));
        }
        Ok(Self { extension })
    }
}

/// One local selection before durable publication. This value is not a Pending
/// allocation acknowledgment and cannot mint an execution root. Its sole birth
/// is transferred into the canonical fresh tracker publication, never renewed.
pub(crate) struct OriginalIssuerLaunchWindow {
    identity: Arc<()>,
    effects: Arc<AuraEffectSystem>,
    started_at: PhysicalTime,
    birth: Option<OriginalEnrollmentWindowBirth>,
}

/// Observation of that same local selection. Carries no durable authority,
/// arbitrary executor, provider replacement or duration reconstruction API.
pub(crate) struct OriginalIssuerLaunchObservation {
    effects: Arc<AuraEffectSystem>,
    active: TimeoutBudget,
}

/// Only the original selected cleanup helper can mint this after observing the
/// actual retained retirement receipt and successful endpoint validation.
pub(crate) struct OriginalAllocationCleanupAcknowledgment {
    effects: Arc<AuraEffectSystem>,
    receipt: Arc<()>,
    launch: Arc<()>,
}
impl OriginalAllocationCleanupAcknowledgment {
    pub(crate) fn require_original(
        &self,
        effects: &AuraEffectSystem,
        receipt: &Arc<()>,
        launch: &Arc<()>,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.as_ref(), effects)
            || !Arc::ptr_eq(&self.receipt, receipt)
            || !Arc::ptr_eq(&self.launch, launch)
        {
            return Err(AuraError::permission_denied(
                "allocation acknowledgment belongs to another original retirement",
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct OriginalAllocationCheckpointCustody {
    latest: Option<aura_core::time::timeout::NegativeCheckpointObservation<AuraError, Arc<()>>>,
    first_checkpoint_failure: Option<TimeoutBudgetError>,
    first_validation_failure: Option<TimeoutBudgetError>,
}

#[cfg(test)]
impl OriginalAllocationCheckpointCustody {
    pub(crate) fn first_validation_failure_for_test(&self) -> Option<&TimeoutBudgetError> {
        self.first_validation_failure.as_ref()
    }
}

pub(super) struct SelectedOriginalIssuerBirth {
    pub(super) started_at: PhysicalTime,
    pub(super) birth: OriginalEnrollmentWindowBirth,
}

impl OriginalIssuerLaunchWindow {
    pub(crate) fn observation_identity(&self) -> &Arc<()> {
        &self.identity
    }
    /// Execute the original prepublication allocation attempt. Move-owned
    /// output belongs in the caller's retained slot before the callback returns;
    /// a failed postcheck must not discard the original allocation owner.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_issuer_launch_window", receiver_type = OriginalIssuerLaunchWindow, family = "runtime_helper")]
    pub(crate) async fn execute_original_allocation<F, Fut, T>(
        &self,
        effects: &Arc<AuraEffectSystem>,
        operation: F,
    ) -> Result<T, TimeoutRunError<AuraError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, AuraError>>,
    {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(TimeoutRunError::Operation(AuraError::permission_denied(
                "original allocation belongs to another actual runtime",
            )));
        }
        let birth = self.birth.as_ref().ok_or_else(|| {
            TimeoutRunError::Operation(AuraError::invalid(
                "original launch birth already transferred",
            ))
        })?;
        aura_core::execute_with_timeout_budget(effects.as_ref(), &birth.execution, operation).await
    }

    /// Bound negative-only work while this actual original birth selection is
    /// still retained. The caller keeps actual partial-allocation receipts in
    /// its own retained slots before Ready; this result mints no durable ACK.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_issuer_launch_window", receiver_type = OriginalIssuerLaunchWindow, family = "runtime_helper")]
    pub(crate) async fn execute_original_allocation_cleanup<'a, F, Fut, T>(
        &self,
        effects: &Arc<AuraEffectSystem>,
        primary: &'a AuraError,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<&'a AuraError, T, AuraError>,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, AuraError>>,
    {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "original allocation cleanup belongs to another actual runtime",
            ));
        }
        let birth = self
            .birth
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original launch birth already transferred"))?;
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects.as_ref(),
                &birth.negative_cleanup,
                primary,
                operation,
            )
            .await,
        )
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_issuer_launch_window", receiver_type = OriginalIssuerLaunchWindow, family = "runtime_helper")]
    pub(crate) async fn acknowledge_original_allocation_cleanup<'a, 'receipt, C, CFut>(
        &self,
        effects: &Arc<AuraEffectSystem>,
        primary: &'a AuraError,
        checkpoint: C,
        retained: &mut OriginalAllocationCheckpointCustody,
    ) -> Result<OriginalAllocationCleanupAcknowledgment, AuraError>
    where
        C: FnOnce() -> CFut,
        CFut: Future<
            Output = Result<
                crate::runtime::effects::OriginalAllocationCleanupReceipt<'receipt>,
                TimeoutBudgetError,
            >,
        >,
    {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "original allocation checkpoint belongs to another actual runtime",
            ));
        }
        let birth = self
            .birth
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original launch birth already transferred"))?;
        let observed = aura_core::time::timeout::acknowledge_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            &birth.negative_cleanup,
            primary,
            checkpoint,
        )
        .await;
        let checkpoint = match observed.checkpoint {
            aura_core::time::timeout::CheckpointAttempt::Returned(receipt) => {
                aura_core::time::timeout::CheckpointAttempt::Returned(
                    receipt.observation_identity().clone(),
                )
            }
            aura_core::time::timeout::CheckpointAttempt::Failed { source } => {
                aura_core::time::timeout::CheckpointAttempt::Failed { source }
            }
            aura_core::time::timeout::CheckpointAttempt::NotAttempted => {
                aura_core::time::timeout::CheckpointAttempt::NotAttempted
            }
            aura_core::time::timeout::CheckpointAttempt::Interrupted => {
                aura_core::time::timeout::CheckpointAttempt::Interrupted
            }
        };
        retained.latest = Some(aura_core::time::timeout::NegativeCheckpointObservation {
            primary: observed.primary.clone(),
            checkpoint,
            validation: observed.validation,
        });
        let actual = retained.latest.as_ref().ok_or_else(|| {
            AuraError::invalid("original allocation checkpoint observation missing")
        })?;
        if let aura_core::time::timeout::CheckpointAttempt::Failed { source } = &actual.checkpoint {
            retained
                .first_checkpoint_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Err(source) = &actual.validation {
            retained
                .first_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        let receipt = match &actual.checkpoint {
            aura_core::time::timeout::CheckpointAttempt::Returned(receipt) => receipt.clone(),
            aura_core::time::timeout::CheckpointAttempt::Failed { source } => {
                return Err(AuraError::from(source.clone()));
            }
            _ => {
                return Err(AuraError::invalid(
                    "original allocation checkpoint did not return its receipt",
                ));
            }
        };
        actual.validation.clone().map_err(AuraError::from)?;
        Ok(OriginalAllocationCleanupAcknowledgment {
            effects: self.effects.clone(),
            receipt,
            launch: self.identity.clone(),
        })
    }

    pub(crate) async fn select(
        effects: Arc<AuraEffectSystem>,
    ) -> Result<(Self, OriginalIssuerLaunchObservation), AuraError> {
        // This first selected-provider read precedes the existence of any local
        // interval; failure propagates without choosing a replacement clock.
        let started_at = effects
            .physical_time()
            .await
            .map_err(|source| AuraError::Internal {
                message: "select original issuer launch clock".into(),
                source: Some(Arc::new(source)),
            })?;
        let execution_policy = super::ceremony_tracker::CeremonyTracker::timeout_for_kind(
            aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
        );
        let shortcut_policy =
            OriginalShortcutWindowPolicy::bounded_extension(execution_policy, execution_policy)
                .map_err(AuraError::from)?;
        let birth =
            OriginalEnrollmentWindowBirth::allocate(&started_at, execution_policy, shortcut_policy)
                .map_err(AuraError::from)?;
        let observation = OriginalIssuerLaunchObservation {
            effects: effects.clone(),
            active: birth.execution.clone(),
        };
        Ok((
            Self {
                identity: Arc::new(()),
                effects,
                started_at,
                birth: Some(birth),
            },
            observation,
        ))
    }

    pub(super) fn take_original_birth(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<SelectedOriginalIssuerBirth, AuraError> {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "original launch belongs to another actual runtime",
            ));
        }
        let birth = self
            .birth
            .take()
            .ok_or_else(|| AuraError::invalid("original launch birth already transferred"))?;
        Ok(SelectedOriginalIssuerBirth {
            started_at: self.started_at.clone(),
            birth,
        })
    }
}

impl OriginalIssuerLaunchObservation {
    pub(crate) async fn allocate_task_identity(
        &self,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<uuid::Uuid, AuraError> {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "original launch task identity belongs to another runtime",
            ));
        }
        aura_core::execute_with_timeout_budget(effects.as_ref(), &self.active, || async {
            Ok::<_, AuraError>(
                aura_core::effects::RandomExtendedEffects::random_uuid(effects.as_ref()).await,
            )
        })
        .await
        .map_err(|source| AuraError::Internal {
            message: "allocate original issuer task identity".into(),
            source: Some(Arc::new(source)),
        })
    }

    pub(crate) async fn receive_initial_ready(
        &self,
        effects: &Arc<AuraEffectSystem>,
        receiver: &mut Option<
            tokio::sync::oneshot::Receiver<
                Result<
                    crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady,
                    AuraError,
                >,
            >,
        >,
        retained: &mut Option<
            Result<
                crate::runtime_bridge::enrollment_quorum::OriginalPreparedIssuerReady,
                AuraError,
            >,
        >,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, effects) {
            return Err(AuraError::permission_denied(
                "original launch readiness belongs to another runtime",
            ));
        }
        if let Some(Err(source)) = retained.as_ref() {
            return Err(source.clone());
        }
        aura_core::execute_with_timeout_budget(effects.as_ref(), &self.active, || async {
            if retained.is_none() {
                let original = receiver.as_mut().ok_or_else(|| {
                    AuraError::invalid("original launch readiness receiver missing")
                })?;
                let result = original.await.map_err(|source| AuraError::Internal {
                    message: "original launch readiness channel stopped".into(),
                    source: Some(Arc::new(source)),
                })?;
                // Deposit before Ready: the observer owns neither this slot nor
                // the actual completed issuance tuple kept by the entry.
                *retained = Some(result);
                *receiver = None;
            }
            if let Some(Err(source)) = retained.as_ref() {
                return Err(source.clone());
            }
            Ok::<(), AuraError>(())
        })
        .await
        .map_err(|source| AuraError::Internal {
            message: "observe original issuer launch readiness".into(),
            source: Some(Arc::new(source)),
        })
    }
}

impl OriginalEnrollmentWindowBirth {
    pub(super) fn allocate(
        started_at: &aura_core::time::PhysicalTime,
        positive: std::time::Duration,
        shortcut_policy: OriginalShortcutWindowPolicy,
    ) -> Result<Self, TimeoutBudgetError> {
        let positive_ms = u64::try_from(positive.as_millis()).map_err(|_| {
            TimeoutBudgetError::invalid_policy("original execution extent overflow")
        })?;
        let execution_end = started_at.ts_ms.checked_add(positive_ms).ok_or_else(|| {
            TimeoutBudgetError::invalid_policy("original execution endpoint overflow")
        })?;
        let full_ms = positive_ms
            .checked_add(EnrollmentNegativeCleanupAllowance::DEFAULT.milliseconds)
            .ok_or_else(|| {
                TimeoutBudgetError::invalid_policy("original cleanup extent overflow")
            })?;
        let negative_cleanup = TimeoutBudget::from_start_and_timeout(
            started_at,
            std::time::Duration::from_millis(full_ms),
        )?;
        // Two fixed endpoints share the ORIGINAL observation owner. Positive
        // expiration is independently latched and cannot extend the child.
        let execution = negative_cleanup
            .attenuate_deadline_at(aura_core::types::window::WindowPosition::new(execution_end))?;
        let policy = OriginalEnrollmentWindowPolicy::at_original_birth(&execution)?;
        // Select a distinct shortcut endpoint at the same original provider
        // observation; completion has no clock-selection or renewal authority.
        let extension_ms = u64::try_from(shortcut_policy.extension.as_millis())
            .map_err(|_| TimeoutBudgetError::invalid_policy("original shortcut extent overflow"))?;
        let shortcut_ms = positive_ms.checked_add(extension_ms).ok_or_else(|| {
            TimeoutBudgetError::invalid_policy("original shortcut extent overflow")
        })?;
        let shortcut = TimeoutBudget::from_start_and_timeout(
            started_at,
            std::time::Duration::from_millis(shortcut_ms),
        )?;
        Ok(Self {
            execution,
            shortcut,
            negative_cleanup,
            policy,
        })
    }
}

#[cfg(test)]
mod original_shortcut_window_policy_tests {
    use super::*;

    #[test]
    fn shortcut_endpoint_is_selected_at_original_birth_and_expires_without_renewal() {
        let started = PhysicalTime::exact(100);
        let execution = std::time::Duration::from_millis(1_000);
        assert!(OriginalShortcutWindowPolicy::bounded_extension(
            execution,
            std::time::Duration::from_millis(1_001),
        )
        .is_err());
        let policy = OriginalShortcutWindowPolicy::bounded_extension(execution, execution)
            .expect("bounded typed shortcut policy");
        let birth = OriginalEnrollmentWindowBirth::allocate(&started, execution, policy)
            .expect("one original clock selection");
        assert_eq!(birth.execution.deadline_at_ms(), 1_100);
        assert_eq!(birth.shortcut.started_at_ms(), 100);
        assert_eq!(birth.shortcut.deadline_at_ms(), 2_100);
        assert!(birth
            .shortcut
            .remaining_at(&PhysicalTime::exact(2_099))
            .is_ok());
        assert!(birth
            .shortcut
            .remaining_at(&PhysicalTime::exact(2_100))
            .is_err());
    }
}

/// Consumes the original issuer root synchronously. All later futures borrow
/// this owner; neither root nor actual receipts are moved into those futures.
pub(crate) struct OriginalIssuerNegativeCleanupOwner {
    root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
    terminal: Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
    destination: Arc<crate::handlers::invitation_service::OriginalIssuerTerminalDestination>,
    sealed: Option<crate::task_registry::SealedTaskGroup>,
    disposal: Option<Arc<crate::task_registry::TaskGroupDisposalAcknowledgment>>,
    checkpoint: Option<super::ceremony_tracker::OriginalIssuerNegativeCheckpointReceipt>,
    settled: bool,
    conditional_signing: Option<
        Arc<tokio::sync::Mutex<super::threshold_signing::OriginalIssuerTranscriptSigningCustody>>,
    >,
    retirement_failure: Option<AuraError>,
    work_failure: Option<AuraError>,
    checkpoint_failure: Option<TimeoutBudgetError>,
    validation_failure: Option<TimeoutBudgetError>,
}

/// Pre-registration cleanup retains the actual allocation and born root. This
/// evidence cannot sign a terminal frame or authorize a cancellation notice.
pub(crate) struct OriginalHeldIssuerNegativeCleanupOwner {
    root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
    reservation: Option<crate::runtime::effects::EnrollmentGenerationReservation<'static>>,
    primary: AuraError,
    retirement: Option<crate::runtime::effects::AcknowledgedEnrollmentRetirement>,
    sealed: Option<crate::task_registry::SealedTaskGroup>,
    disposal: Option<crate::task_registry::TaskGroupDisposalAcknowledgment>,
    checkpoint: Option<super::ceremony_tracker::OriginalIssuerNegativeCheckpointReceipt>,
    work_failure: Option<AuraError>,
    checkpoint_failure: Option<TimeoutBudgetError>,
    validation_failure: Option<TimeoutBudgetError>,
    acknowledged: bool,
}
impl OriginalHeldIssuerNegativeCleanupOwner {
    pub(crate) fn from_original(
        root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        reservation: crate::runtime::effects::EnrollmentGenerationReservation<'static>,
        primary: AuraError,
    ) -> Self {
        Self {
            root,
            reservation: Some(reservation),
            primary,
            retirement: None,
            sealed: None,
            disposal: None,
            checkpoint: None,
            work_failure: None,
            checkpoint_failure: None,
            validation_failure: None,
            acknowledged: false,
        }
    }

    pub(crate) async fn settle(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<(), AuraError> {
        use aura_core::time::timeout::{
            acknowledge_negative_cleanup_with_timeout_budget,
            execute_negative_cleanup_with_timeout_budget, CheckpointAttempt,
        };
        self.root.origin.require_effects(effects.as_ref())?;
        if self.acknowledged {
            return Ok(());
        }
        let reservation = self
            .reservation
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original held reservation is missing"))?;
        self.root
            .origin
            .require_original_held_reservation(reservation)?;
        reservation.require_effects(effects.as_ref())?;
        let tracker = reservation.decision().tracker();
        if self.sealed.is_none() {
            self.root.children.request_cancellation();
            self.sealed = Some(self.root.children.seal_admission());
        }
        let budget = self.root.origin.negative_cleanup_budget();
        let retirement = &mut self.retirement;
        let disposal = &mut self.disposal;
        let work_failure = &mut self.work_failure;
        let children = &self.root.children;
        let sealed = self
            .sealed
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original held subtree seal is missing"))?;
        let work = execute_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.primary,
            || async {
                let result = async {
                    if disposal.is_none() {
                        *disposal = Some(sealed.await_disposal().await);
                    }
                    disposal
                        .as_ref()
                        .ok_or_else(|| AuraError::invalid("held disposal unresolved"))?
                        .require_original_group(children)?;
                    if retirement.is_none() {
                        *retirement = Some(
                            tracker
                                .retire_original_held_enrollment_generation(reservation)
                                .await?,
                        );
                    }
                    retirement
                        .as_ref()
                        .ok_or_else(|| AuraError::invalid("held retirement unresolved"))?
                        .require_original_held(reservation, effects.as_ref())
                }
                .await;
                if let Err(source) = &result {
                    work_failure.get_or_insert_with(|| source.clone());
                }
                result
            },
        )
        .await;
        if let Err(source) = work.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AuraError::from(source));
        }
        match work.operation {
            Some(Ok(())) => {}
            Some(Err(source)) => return Err(source),
            None => return Err(AuraError::invalid("original held cleanup is unresolved")),
        }
        let original = &self.root.origin;
        let checkpoint = &mut self.checkpoint;
        let checkpoint_failure = &mut self.checkpoint_failure;
        let observation = acknowledge_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.primary,
            || async {
                if checkpoint.is_none() {
                    match original.acknowledge_negative_checkpoint().await {
                        Ok(actual) => *checkpoint = Some(actual),
                        Err(source) => {
                            let source = TimeoutBudgetError::checkpoint_failure(source);
                            checkpoint_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    }
                }
                checkpoint
                    .as_ref()
                    .ok_or_else(|| TimeoutBudgetError::invalid_policy("held checkpoint unresolved"))
            },
        )
        .await;
        if let Err(source) = observation.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AuraError::from(source));
        }
        match observation.checkpoint {
            CheckpointAttempt::Returned(actual) => {
                let retirement = self
                    .retirement
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("held retirement missing"))?;
                let disposal = self
                    .disposal
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("held disposal missing"))?;
                original.release_held_negative_execution_lease(
                    reservation,
                    retirement,
                    actual,
                    disposal,
                    children,
                )?;
                self.acknowledged = true;
                // Only the actual receipts permit releasing native exclusivity.
                self.reservation = None;
                Ok(())
            }
            CheckpointAttempt::Failed { source } => Err(AuraError::from(source)),
            _ => Err(AuraError::invalid("original held checkpoint unresolved")),
        }
    }
    #[cfg(test)]
    pub(crate) fn configured_timeout_ms_for_test(&self) -> u64 {
        u64::try_from(
            super::ceremony_tracker::CeremonyTracker::timeout_for_kind(
                aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
            )
            .as_millis(),
        )
        .expect("enrollment timeout fits milliseconds")
    }
    #[cfg(test)]
    pub(crate) fn original_interval_for_test(&self) -> (u64, u64) {
        (
            self.root.child.original.started_at_ms(),
            self.root.child.original.deadline_at_ms(),
        )
    }
    #[cfg(test)]
    pub(crate) fn assert_retained_cleanup_for_test(&self, original_primary: &AuraError) {
        assert!(
            self.acknowledged,
            "actual original required cleanup acknowledged"
        );
        assert!(
            self.reservation.is_none(),
            "native guards released only after actual acknowledgment"
        );
        assert!(
            self.retirement.is_some(),
            "actual original native retirement receipt retained"
        );
        self.disposal
            .as_ref()
            .expect("actual disposed subtree receipt")
            .require_original_group(&self.root.children)
            .unwrap();
        self.checkpoint
            .as_ref()
            .expect("actual protected negative checkpoint receipt")
            .require_original(&self.root.origin)
            .unwrap();
        match (&self.primary, original_primary) {
            (
                AuraError::Internal {
                    source: Some(actual),
                    ..
                },
                AuraError::Internal {
                    source: Some(expected),
                    ..
                },
            ) => assert!(
                Arc::ptr_eq(actual, expected),
                "exact original first cause allocation retained"
            ),
            _ => panic!("expected actual original cancellation cause"),
        }
        assert!(matches!(
            self.settlement().unwrap(),
            aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(_)
        ));
    }
    pub(crate) fn settlement(
        &self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, AuraError> {
        if !self.acknowledged {
            return Err(AuraError::invalid(
                "held allocation lacks resource acknowledgment",
            ));
        }
        Ok(
            aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                self.primary.clone(),
            ),
        )
    }
}
impl OriginalIssuerNegativeCleanupOwner {
    pub(crate) fn from_original(
        root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        terminal: Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>,
        destination: Arc<crate::handlers::invitation_service::OriginalIssuerTerminalDestination>,
        conditional_signing: Option<
            Arc<
                tokio::sync::Mutex<
                    super::threshold_signing::OriginalIssuerTranscriptSigningCustody,
                >,
            >,
        >,
    ) -> Self {
        Self {
            root,
            terminal,
            destination,
            sealed: None,
            disposal: None,
            checkpoint: None,
            settled: false,
            conditional_signing,
            retirement_failure: None,
            work_failure: None,
            checkpoint_failure: None,
            validation_failure: None,
        }
    }
    pub(crate) async fn settle(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<(), AuraError> {
        use aura_core::time::timeout::{
            acknowledge_negative_cleanup_with_timeout_budget,
            execute_negative_cleanup_with_timeout_budget, CheckpointAttempt,
        };
        self.root
            .child
            .require_execution_effects(effects.as_ref())?;
        self.terminal
            .require_original_effects(effects)
            .map_err(|source| match source {
                crate::core::AgentError::Aura(source) => source,
                source => AuraError::Internal {
                    message: "original cancelled issuer binding".into(),
                    source: Some(Arc::new(source)),
                },
            })?;
        let first_failure = &mut self.retirement_failure;
        self.destination.with_original_cancelled_retirement(
            &self.root.origin,
            &self.terminal,
            |_, _, failure| {
                if let Some(source) = failure {
                    first_failure.get_or_insert_with(|| source.clone());
                }
                Ok(())
            },
        )?;
        if self.sealed.is_none() {
            self.root.children.request_cancellation();
            self.sealed = Some(self.root.children.seal_admission());
        }
        #[cfg(test)]
        self.destination
            .await_negative_cleanup_first_observation_gate_for_test(
                self.checkpoint.is_some(),
                self.disposal.is_some(),
            )
            .await;
        let budget = self.root.origin.negative_cleanup_budget();
        let disposal = &mut self.disposal;
        let work_failure = &mut self.work_failure;
        let sealed = self
            .sealed
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original negative subtree seal is missing"))?;
        let children = &self.root.children;
        let conditional_signing = &self.conditional_signing;
        let original_child = &self.root.child;
        let issued = self.terminal.issued();
        #[cfg(test)]
        let mut refusal_observer = None;
        let destination = &self.destination;
        let work = execute_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.terminal,
            || async {
                #[cfg(test)]
                {
                    destination
                        .await_negative_cleanup_after_observation_gate_for_test(
                            &mut refusal_observer,
                        )
                        .await;
                }

                if disposal.is_none() {
                    *disposal = Some(Arc::new(sealed.await_disposal().await));
                }
                disposal
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original negative disposal missing"))?
                    .require_original_group(children)?;
                if let Err(source) = destination
                    .dispose_original_vm(effects.as_ref(), issued, original_child)
                    .await
                {
                    work_failure.get_or_insert_with(|| source.clone());
                    return Err(source);
                }
                if let Some(signing) = conditional_signing {
                    if let Err(source) = signing
                        .lock()
                        .await
                        .retire_original_positive_for_cleanup(original_child)
                    {
                        work_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                }
                let result = disposal
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original negative disposal is unresolved"))?
                    .require_original_group(children);
                if let Err(source) = &result {
                    work_failure.get_or_insert_with(|| source.clone());
                }
                result
            },
        )
        .await;
        if let Err(source) = work.validation {
            #[cfg(test)]
            if let Some(observer) = refusal_observer.take() {
                let _ = observer.send(source.clone());
            }

            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AuraError::from(source));
        }
        match work.operation {
            Some(Ok(())) => {}
            Some(Err(source)) => return Err(source),
            None => {
                return Err(AuraError::invalid(
                    "original negative disposal was not acknowledged",
                ));
            }
        }
        let checkpoint = &mut self.checkpoint;
        let checkpoint_failure = &mut self.checkpoint_failure;
        let original = &self.root.origin;
        let observation = acknowledge_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.terminal,
            || async {
                if checkpoint.is_none() {
                    match original.acknowledge_negative_checkpoint().await {
                        Ok(receipt) => *checkpoint = Some(receipt),
                        Err(source) => {
                            let source = TimeoutBudgetError::checkpoint_failure(source);
                            checkpoint_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    }
                }
                checkpoint.as_ref().ok_or_else(|| {
                    TimeoutBudgetError::invalid_policy("original negative checkpoint is unresolved")
                })
            },
        )
        .await;
        if let Err(source) = observation.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AuraError::from(source));
        }
        match observation.checkpoint {
            CheckpointAttempt::Returned(receipt) => {
                receipt.require_original(original)?;
                #[cfg(any(test, feature = "simulation"))]
                self.destination
                    .retain_simulated_original_subtree_disposal(
                        Arc::clone(self.disposal.as_ref().ok_or_else(|| {
                            AuraError::invalid("original subtree disposal missing")
                        })?),
                        &self.root.children,
                    )?;
                self.settled = true;
                Ok(())
            }
            CheckpointAttempt::Failed { source } => Err(AuraError::from(source)),
            _ => Err(AuraError::invalid(
                "original negative checkpoint custody is unresolved",
            )),
        }
    }
}

/// Resource terminal custody containing the actual original root, signed
/// negative decision, protected checkpoint and matching disposed subtree.
/// Its presence never changes the domain cancellation into positive success.
pub(crate) struct AcknowledgedIssuerNegativeTerminal {
    owner: OriginalIssuerNegativeCleanupOwner,
}
impl OriginalIssuerNegativeCleanupOwner {
    pub(crate) fn into_acknowledged(
        mut self,
    ) -> Result<AcknowledgedIssuerNegativeTerminal, Box<Self>> {
        if !self.settled {
            return Err(Box::new(self));
        }
        let Some(checkpoint) = self.checkpoint.as_ref() else {
            return Err(Box::new(self));
        };
        let Some(disposal) = self.disposal.as_ref() else {
            return Err(Box::new(self));
        };
        if let Err(source) = self.root.origin.release_negative_execution_lease(
            &self.terminal,
            checkpoint,
            disposal,
            &self.root.children,
        ) {
            self.work_failure.get_or_insert(source);
            return Err(Box::new(self));
        }
        Ok(AcknowledgedIssuerNegativeTerminal { owner: self })
    }
}
impl AcknowledgedIssuerNegativeTerminal {
    pub(crate) fn domain_failure(&self) -> AuraError {
        let original = self.owner.terminal.original_domain_failure();
        let cleanup = self
            .owner
            .retirement_failure
            .clone()
            .or_else(|| self.owner.work_failure.clone())
            .or_else(|| self.owner.checkpoint_failure.clone().map(AuraError::from))
            .or_else(|| self.owner.validation_failure.clone().map(AuraError::from));
        match cleanup {
            Some(primary) => AuraError::Internal {
                message: "original cancellation and cleanup domains failed".into(),
                source: Some(Arc::new(
                    crate::handlers::invitation_service::EnrollmentInitiatorTaskFailure::new(
                        crate::AgentError::from(primary),
                        Some(original),
                    ),
                )),
            },
            None => original,
        }
    }
}

// Private field of the original move-owned EnrollmentExecutionRoot. These are
// actual proof objects, not serialized phases or identifiers. Empty slots are
// unresolved and cannot authorize terminal consumption.
struct RetainedSigningTerminalCustody {
    negative_seal: Option<crate::task_registry::SealedTaskGroup>,
    disposal: Option<crate::task_registry::TaskGroupDisposalAcknowledgment>,
    closed: Option<signing_checkpoint::OriginalSigningTerminalReceipt>,
    negative: Option<signing_checkpoint::OriginalSigningNegativeCleanupReceipt>,
}
impl RetainedSigningTerminalCustody {
    fn fresh() -> Self {
        Self {
            negative_seal: None,
            disposal: None,
            closed: None,
            negative: None,
        }
    }
    fn retain_disposal(
        &mut self,
        proof: crate::task_registry::TaskGroupDisposalAcknowledgment,
    ) -> Result<(), AuraError> {
        if self.disposal.is_some() {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        self.disposal = Some(proof);
        Ok(())
    }
    fn retain_closed(
        &mut self,
        proof: signing_checkpoint::OriginalSigningTerminalReceipt,
    ) -> Result<&signing_checkpoint::OriginalSigningTerminalReceipt, AuraError> {
        if self.closed.is_some() || self.negative.is_some() || self.disposal.is_none() {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        self.closed = Some(proof);
        match self.closed.as_ref() {
            Some(original) => Ok(original),
            None => Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()),
        }
    }
}

// Negative custody retains the original root and its concrete first failure.
struct EnrollmentNegativeCleanupOwner<Origin> {
    root: EnrollmentExecutionRoot<Origin>,
    primary: AuraError,
    cleanup_operation_failure: Option<AuraError>,
    cleanup_validation_failure: Option<TimeoutBudgetError>,
    cleanup_checkpoint_failure: Option<TimeoutBudgetError>,
}
struct AcknowledgedSigningNegativeCleanup {
    // These actual proofs and first causes remain owned until terminal custody drops.
    _closed: Option<signing_checkpoint::OriginalSigningTerminalReceipt>,
    _cleanup_operation_failure: Option<AuraError>,
    _cleanup_validation_failure: Option<TimeoutBudgetError>,
    _cleanup_checkpoint_failure: Option<TimeoutBudgetError>,
    _completion: Option<Arc<super::threshold_signing::OriginalParticipantCompletionCustody>>,
    _disposal: crate::task_registry::TaskGroupDisposalAcknowledgment,
    primary: AuraError,
    _receipt: signing_checkpoint::OriginalSigningNegativeCleanupReceipt,
}

impl
    EnrollmentNegativeCleanupOwner<
        crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    >
{
    async fn acknowledge_signing_refusal(&mut self) -> Result<(), AuraError> {
        let root = &mut self.root;
        let effects = root.origin.effects();
        root.child.require_execution_effects(effects.as_ref())?;
        let WindowCheckpoint::ApprovedSigning { capability } = root.child.checkpoint.as_ref()
        else {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        };
        let cleanup = capability.negative_cleanup_budget();
        let children = &root.children;
        let original = &root.child.original;
        let retained = &mut root.signing_terminal;
        if retained.negative_seal.is_none() {
            retained.negative_seal = Some(children.seal_admission());
        }
        if retained.disposal.is_none() {
            let cleanup_operation_failure = &mut self.cleanup_operation_failure;
            let observed = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects.as_ref(),
                cleanup,
                &self.primary,
                || async {
                    let Some(seal) = retained.negative_seal.as_ref() else {
                        let source =
                            DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal();
                        cleanup_operation_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    };
                    let disposal = seal.await_disposal().await;
                    // Retain the actual returned proof before validation. Its
                    // presence is not an ACK; every retry validates it below.
                    retained.disposal = Some(disposal);
                    let actual = retained.disposal.as_ref().ok_or_else(|| {
                        DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                    })?;
                    if let Err(source) = actual.require_original_group(children) {
                        cleanup_operation_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                    Ok::<(), AuraError>(())
                },
            )
            .await;
            if let Some(Err(source)) = observed.operation.as_ref() {
                self.cleanup_operation_failure
                    .get_or_insert_with(|| source.clone());
            }
            if let Err(source) = observed.validation.as_ref() {
                self.cleanup_validation_failure
                    .get_or_insert_with(|| source.clone());
            }
            match observed.operation {
                Some(Ok(())) => {}
                Some(Err(source)) => return Err(source),
                None => {
                    return Err(observed
                        .validation
                        .err()
                        .map(AuraError::from)
                        .unwrap_or_else(|| {
                            DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                        }));
                }
            }
            observed.validation.map_err(AuraError::from)?;
        }
        retained
            .disposal
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?
            .require_original_group(children)?;
        let cleanup_checkpoint_failure = &mut self.cleanup_checkpoint_failure;
        let observed = aura_core::time::timeout::acknowledge_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            cleanup,
            &self.primary,
            || async move {
                if retained.negative.is_none() {
                    let receipt = match capability.refuse_original_participant(original).await {
                        Ok(receipt) => receipt,
                        Err(source) => {
                            cleanup_checkpoint_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    };
                    retained.negative = Some(receipt);
                }
                retained.negative.as_ref().ok_or_else(|| {
                    let source = TimeoutBudgetError::checkpoint_failure(
                        DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal(),
                    );
                    cleanup_checkpoint_failure.get_or_insert_with(|| source.clone());
                    source
                })
            },
        )
        .await;
        // Returned borrowed proof is observation only. Actual owner fields stay
        // intact on independent post-validation failure or whole-future Drop.
        if let Err(source) = observed.validation.as_ref() {
            self.cleanup_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        if let aura_core::time::timeout::CheckpointAttempt::Failed { source } = &observed.checkpoint
        {
            self.cleanup_checkpoint_failure
                .get_or_insert_with(|| source.clone());
        }
        observed.validation.map_err(AuraError::from)?;
        match observed.checkpoint {
            aura_core::time::timeout::CheckpointAttempt::Returned(receipt) => {
                receipt.require_original(capability)
            }
            aura_core::time::timeout::CheckpointAttempt::Failed { source } => Err(source.into()),
            aura_core::time::timeout::CheckpointAttempt::NotAttempted
            | aura_core::time::timeout::CheckpointAttempt::Interrupted => {
                Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())
            }
        }
    }
}

impl
    EnrollmentExecutionRoot<
        crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    >
{
    /// Consume actual local consent and its fresh protected publication once.
    /// Descendants cannot own or clear this root's terminal custody.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RuntimeApprovedEnrollmentSigningIntent",
        family = "runtime_helper"
    )]
    pub(crate) async fn approved_signing(
        effects: Arc<AuraEffectSystem>,
        approval: crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        original_tasks: &crate::task_registry::TaskGroup,
    ) -> Result<Self, AuraError> {
        let (capability, budget, lease) =
            signing_checkpoint::ApprovedSigningCheckpoint::allocate(effects, &approval).await?;
        Ok(Self {
            child: EnrollmentExecutionChild {
                active: budget.clone(),
                original: budget,
                _lease: EnrollmentExecutionLease::Exclusive { _original: lease },
                checkpoint: Arc::new(WindowCheckpoint::ApprovedSigning {
                    capability: Arc::new(capability),
                }),
            },
            children: original_tasks.group("original-approved-signing-descendants"),
            signing_terminal: RetainedSigningTerminalCustody::fresh(),
            origin: Arc::new(approval),
        })
    }
}

// This acknowledgment retains both actual domain completion and actual Closed
// storage publication. Recovered phase bytes cannot manufacture this owner.
struct AcknowledgedApprovedSigningWindow {
    _disposal: crate::task_registry::TaskGroupDisposalAcknowledgment,
    _receipt: signing_checkpoint::OriginalSigningTerminalReceipt,
}

impl
    EnrollmentExecutionRoot<
        crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    >
{
    async fn acknowledge_approved_participant(
        &mut self,
        completion: &super::threshold_signing::OriginalParticipantCompletion,
    ) -> Result<AcknowledgedApprovedSigningWindow, AuraError> {
        let result = async {
            let effects = self.origin.effects();
            self.child.require_execution_effects(effects.as_ref())?;
            completion.require_original(self.origin.as_ref())?;
            let WindowCheckpoint::ApprovedSigning { capability } = self.child.checkpoint.as_ref()
            else {
                return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
            };
            if self.signing_terminal.disposal.is_none() {
                let sealed = self.children.seal_admission();
                let original_group = &self.children;
                let retained = &mut self.signing_terminal;
                aura_core::time::timeout::execute_with_timeout_budget(
                    effects.as_ref(),
                    &self.child.active,
                    || async {
                        let disposal = sealed.await_disposal().await;
                        retained.retain_disposal(disposal)?;
                        retained
                            .disposal
                            .as_ref()
                            .ok_or_else(|| {
                                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                            })?
                            .require_original_group(original_group)?;
                        // Empty descendants prove only resource disposal. Actual
                        // domain success comes independently from completion.
                        Ok(())
                    },
                )
                .await
                .map_err(|source| match source {
                    TimeoutRunError::Operation(source) => source,
                    TimeoutRunError::Timeout(source) => AuraError::from(source),
                })?;
            }
            let disposal = self.signing_terminal.disposal.as_ref().ok_or_else(|| {
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
            })?;
            disposal.require_original_group(&self.children)?;
            if let Some(source) = disposal.failure() {
                return Err(AuraError::Internal {
                    message: "original signing descendant disposal failed".into(),
                    source: Some(Arc::new(source.clone())),
                });
            }
            if let crate::task_registry::TaskGroupExecutionOutcome::Interrupted(source) =
                disposal.execution_outcome()
            {
                return Err(AuraError::Internal {
                    message: "original signing descendant was interrupted".into(),
                    source: Some(Arc::new(source.clone())),
                });
            }
            let original = &self.child.original;
            let approval = self.origin.as_ref();
            let retained = &mut self.signing_terminal;
            // Targets the coordinated 282 generic-A shared ACK API, not 310's
            // unit API. Actual receipt is forwarded to the terminal owner.
            aura_core::time::timeout::acknowledge_with_timeout_budget(
                effects.as_ref(),
                &self.child.active,
                || async {
                    if retained.closed.is_some() {
                        return match retained.closed.as_ref() {
                            Some(receipt) => Ok(receipt),
                            None => Err(TimeoutBudgetError::checkpoint_failure(
                                DurableEnrollmentExecutionError::MissingTerminalCheckpoint
                                    .refusal(),
                            )),
                        };
                    }
                    let receipt = capability
                        .close_original_participant(original, approval, completion)
                        .await?;
                    retained
                        .retain_closed(receipt)
                        .map_err(TimeoutBudgetError::checkpoint_failure)
                },
                |_retained_receipt| (),
            )
            .await
            .map_err(AuraError::from)?;
            self.signing_terminal
                .closed
                .as_ref()
                .ok_or_else(|| {
                    DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                })?
                .require_original(capability)?;
            // Both actual proofs survived every post-checkpoint await in this
            // original root. Only successful validation consumes them.
            match (
                self.signing_terminal.closed.take(),
                self.signing_terminal.disposal.take(),
            ) {
                (Some(receipt), Some(disposal)) => Ok(AcknowledgedApprovedSigningWindow {
                    _receipt: receipt,
                    _disposal: disposal,
                }),
                (receipt, disposal) => {
                    self.signing_terminal.closed = receipt;
                    self.signing_terminal.disposal = disposal;
                    Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())
                }
            }
        }
        .await;
        // The original registered holder still owns this borrowed root on
        // every error or cancellation; it transitions to negative synchronously.
        result
    }
}

// Private domain holder for the SAME registered-task custody slot. No Clone,
// serde, raw Root extraction, public callback or arbitrary execute API.
enum SigningTaskDomainState {
    Positive(
        EnrollmentExecutionRoot<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
    ),
    Negative(
        EnrollmentNegativeCleanupOwner<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
    ),
    Closed {
        _acknowledged: AcknowledgedApprovedSigningWindow,
        _completion: Arc<super::threshold_signing::OriginalParticipantCompletionCustody>,
    },
    NegativeAcknowledged(AcknowledgedSigningNegativeCleanup),
}
/// The original admitted task alone creates this cell acquisition capability.
/// Signing still requires the actual positive material inside that same cell.
pub(super) struct OriginalPeerPositiveContinuationOwner {
    approval: Arc<crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent>,
    cell: Arc<Mutex<super::threshold_signing::OriginalEnrollmentPeerContinuation>>,
    identity: OriginalConditionalSigningWindowIdentity,
}
impl OriginalPeerPositiveContinuationOwner {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalPeerPositiveContinuationOwner",
        receiver_type = OriginalPeerPositiveContinuationOwner,
        family = "runtime_helper"
    )]
    pub(super) async fn execute<F, Fut, T>(
        &self,
        child: &EnrollmentExecutionChild,
        approval: &Arc<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        operation: F,
    ) -> Result<aura_core::time::timeout::NegativeCleanupExecution<&Self, T, AuraError>, AuraError>
    where
        F: FnOnce(Arc<Mutex<super::threshold_signing::OriginalEnrollmentPeerContinuation>>) -> Fut,
        Fut: Future<Output = Result<T, AuraError>>,
    {
        self.identity.require_child(child)?;
        if !Arc::ptr_eq(&self.approval, approval) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let effects = approval.effects().as_ref();
        child.require_execution_effects(effects)?;
        let WindowCheckpoint::ApprovedSigning { capability } = child.checkpoint.as_ref() else {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        };
        capability.require_approved_manifest(approval)?;
        let _original_work = child._lease.borrow_execution()?;
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects,
                capability.negative_cleanup_budget(),
                self,
                || operation(Arc::clone(&self.cell)),
            )
            .await,
        )
    }
}

pub(super) struct OriginalSigningTaskCustody {
    positive_owner: OriginalPeerPositiveContinuationOwner,
    peer_continuation: Arc<Mutex<super::threshold_signing::OriginalEnrollmentPeerContinuation>>,
    peer_original_window: Arc<EnrollmentExecutionChild>,
    peer_changed: Arc<tokio::sync::Notify>,
    service: super::threshold_signing::ThresholdSigningService,
    native: Option<crate::runtime::effects::OriginalApprovedNativeAdmission>,
    material: Option<super::threshold_signing::ValidatedLocalEnrollmentSigningMaterial>,
    conditional_material: Option<super::threshold_signing::OriginalNegativeTerminalSigningMaterial>,
    conditional_retirement:
        Option<Arc<super::threshold_signing::OriginalRetiredConditionalTransportCustody>>,
    conditional_retirement_actor_ack: Option<Result<(), AuraError>>,
    conditional_dispatcher: super::threshold_signing::OriginalEnrollmentTranscriptDispatcher,
    positive_dispatcher: super::threshold_signing::OriginalPositiveDispatcher,
    positive_readiness_operation: Option<Result<(), AuraError>>,
    positive_readiness_validation: Option<Result<PhysicalTime, TimeoutBudgetError>>,
    positive_readiness_failure: Option<AuraError>,
    conditional_operation: Option<Result<(), AuraError>>,
    conditional_validation: Option<Result<PhysicalTime, TimeoutBudgetError>>,
    native_disposal: Option<crate::runtime::effects::AcknowledgedApprovedNativeDisposal>,
    material_disposal: Option<super::threshold_signing::RetiredOriginalSigningMaterial>,
    material_acquired: bool,
    initial_transport_completed: bool,
    first_operation_failure: Option<AuraError>,
    native_cleanup_failure: Option<AuraError>,
    native_cleanup_validation_failure: Option<aura_core::time::timeout::TimeoutBudgetError>,
    ready_sender: Option<
        tokio::sync::oneshot::Sender<
            Result<super::threshold_signing::EnrollmentTranscriptParticipantIngress, AuraError>,
        >,
    >,
    ingress: Option<super::threshold_signing::EnrollmentTranscriptParticipantIngress>,
    participant_actor: Option<super::threshold_signing::OriginalEnrollmentTranscriptActor>,
    // Option supports only synchronous move transitions under exclusive borrow;
    // no awaited callback/future ever receives an extracted Root.
    state: Option<SigningTaskDomainState>,
    completion: Option<Arc<super::threshold_signing::OriginalParticipantCompletionCustody>>,
}
impl OriginalSigningTaskCustody {
    pub(super) fn admit(
        root: EnrollmentExecutionRoot<
            crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        >,
        service: super::threshold_signing::ThresholdSigningService,
        ready_sender: tokio::sync::oneshot::Sender<
            Result<super::threshold_signing::EnrollmentTranscriptParticipantIngress, AuraError>,
        >,
    ) -> Self {
        let native =
            crate::runtime::effects::OriginalApprovedNativeAdmission::from_original(root.origin());
        let peer_original_window = Arc::new(root.owned_execution_child());
        let peer_changed = Arc::new(tokio::sync::Notify::new());
        let peer_continuation = Arc::new(Mutex::new(
            super::threshold_signing::OriginalEnrollmentPeerContinuation::from_original_root(
                root.origin_observation(),
                Arc::clone(&peer_original_window),
                Arc::clone(&peer_changed),
            ),
        ));
        let positive_owner = OriginalPeerPositiveContinuationOwner {
            approval: root.origin_observation(),
            cell: Arc::clone(&peer_continuation),
            identity: OriginalConditionalSigningWindowIdentity {
                checkpoint: Arc::clone(&peer_original_window.checkpoint),
                original: peer_original_window.original.clone(),
            },
        };
        Self {
            positive_owner,
            peer_continuation,
            peer_original_window,
            peer_changed,
            service,
            native: Some(native),
            material: None,
            conditional_material: None,
            conditional_retirement: None,
            conditional_retirement_actor_ack: None,
            conditional_dispatcher:
                super::threshold_signing::OriginalEnrollmentTranscriptDispatcher::default(),
            positive_dispatcher: super::threshold_signing::OriginalPositiveDispatcher::default(),
            positive_readiness_operation: None,
            positive_readiness_validation: None,
            positive_readiness_failure: None,
            conditional_operation: None,
            conditional_validation: None,
            native_disposal: None,
            material_disposal: None,
            material_acquired: false,
            initial_transport_completed: false,
            first_operation_failure: None,
            native_cleanup_failure: None,
            native_cleanup_validation_failure: None,
            ready_sender: Some(ready_sender),
            ingress: None,
            participant_actor: None,
            state: Some(SigningTaskDomainState::Positive(root)),
            completion: Some(Arc::new(
                super::threshold_signing::OriginalParticipantCompletionCustody::fresh(),
            )),
        }
    }
    pub(super) fn peer_continuation(
        &self,
    ) -> Arc<Mutex<super::threshold_signing::OriginalEnrollmentPeerContinuation>> {
        Arc::clone(&self.peer_continuation)
    }
    pub(super) fn peer_original_window(&self) -> Arc<EnrollmentExecutionChild> {
        Arc::clone(&self.peer_original_window)
    }
    fn begin_negative(&mut self, primary: AuraError) {
        let primary = self
            .first_operation_failure
            .as_ref()
            .or_else(|| {
                self.completion
                    .as_ref()
                    .and_then(|actual| actual.original_failure())
            })
            .or_else(|| {
                self.native
                    .as_ref()
                    .and_then(|actual| actual.original_failure())
            })
            .or_else(|| {
                self.native_disposal
                    .as_ref()
                    .and_then(|actual| actual.original_failure())
            })
            .cloned()
            .unwrap_or(primary);
        // Cancellation may touch supervisor locks, so perform it while the
        // actual Root is STILL IN the retained holder. A panic cannot drop a
        // root temporarily moved to a local async frame.
        if let Some(SigningTaskDomainState::Positive(root)) = self.state.as_ref() {
            root.children.request_cancellation();
        }
        // This move/struct construction has no await or callback and no
        // fallible work. Future Drop can only occur after this method returns.
        let previous = self.state.take();
        self.state = match previous {
            Some(SigningTaskDomainState::Positive(root)) => Some(SigningTaskDomainState::Negative(
                EnrollmentNegativeCleanupOwner {
                    root,
                    primary,
                    cleanup_operation_failure: None,
                    cleanup_validation_failure: None,
                    cleanup_checkpoint_failure: None,
                },
            )),
            other => other,
        };
    }
    async fn dispose_native_original(&mut self, negative: bool) -> Result<(), AuraError> {
        if negative {
            if let Some(actor) = self.participant_actor.as_mut() {
                actor.dispose_original_positive_nonces();
            }
        }
        let root = match self.state.as_ref() {
            Some(SigningTaskDomainState::Positive(root)) => root,
            Some(SigningTaskDomainState::Negative(owner)) => &owner.root,
            _ => return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()),
        };
        let approval = root.origin();
        let effects = approval.effects();
        if let Some(native) = self.native.as_ref() {
            native.require_original(approval)?;
        }
        let material = &mut self.material;
        let conditional_material = &mut self.conditional_material;
        let native = &mut self.native;
        let material_disposal = &mut self.material_disposal;
        let native_disposal = &mut self.native_disposal;
        let peer_continuation = &self.peer_continuation;
        let first_failure = &mut self.native_cleanup_failure;
        let operation = move || async move {
            if let Some(actual) = material.take() {
                match actual.retire_original(approval) {
                    Ok(actual) => *material_disposal = Some(actual),
                    Err(rejected) => {
                        let (actual, source) = *rejected;
                        *material = Some(actual);
                        first_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                }
            }
            if let Some(actual) = conditional_material.take() {
                match actual.retire_original(approval) {
                    Ok(actual) => *material_disposal = Some(actual),
                    Err(rejected) => {
                        let (actual, source) = *rejected;
                        *conditional_material = Some(actual);
                        first_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                }
            }
            {
                let mut peer = peer_continuation.lock().await;
                if negative {
                    if let Err(source) = peer.dispose_peer_positive_for_cleanup() {
                        first_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                }
                if let Err(source) = peer.dispose_peer_native() {
                    first_failure.get_or_insert_with(|| source.clone());
                    return Err(source);
                }
            }
            if let Some(actual) = native.take() {
                match actual.dispose_original(approval) {
                    Ok(actual) => *native_disposal = Some(actual),
                    Err(rejected) => {
                        let (actual, source) = *rejected;
                        *native = Some(actual);
                        first_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                }
            }
            if let Err(source) = native_disposal
                .as_ref()
                .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())
                .and_then(|actual| actual.require_original(approval))
            {
                first_failure.get_or_insert_with(|| source.clone());
                return Err(source);
            }
            Ok::<(), AuraError>(())
        };
        if negative {
            let WindowCheckpoint::ApprovedSigning { capability } = root.child.checkpoint.as_ref()
            else {
                return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
            };
            let observed = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects.as_ref(),
                capability.negative_cleanup_budget(),
                match self.state.as_ref() {
                    Some(SigningTaskDomainState::Negative(owner)) => &owner.primary,
                    _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
                },
                operation,
            )
            .await;
            if let Some(Err(source)) = observed.operation.as_ref() {
                self.native_cleanup_failure
                    .get_or_insert_with(|| source.clone());
            }
            if let Err(source) = &observed.validation {
                self.native_cleanup_validation_failure
                    .get_or_insert_with(|| source.clone());
            }
            match observed.operation {
                Some(Ok(())) => observed.validation.map(|_| ()).map_err(AuraError::from),
                Some(Err(source)) => Err(source),
                None => Err(observed
                    .validation
                    .err()
                    .map(AuraError::from)
                    .unwrap_or_else(|| {
                        DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                    })),
            }
        } else {
            root.child
                .execute(effects.as_ref(), operation)
                .await
                .map_err(|source| match source {
                    TimeoutRunError::Operation(source) => source,
                    TimeoutRunError::Timeout(source) => AuraError::from(source),
                })
        }
    }
    async fn close_positive(&mut self) -> Result<(), AuraError> {
        self.dispose_native_original(false).await?;
        let Some(SigningTaskDomainState::Positive(root)) = self.state.as_mut() else {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        };
        let completion = self
            .completion
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        let completion_proof = completion.original()?;
        let native = self
            .native_disposal
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        native.require_original(root.origin())?;
        if let Some(source) = native.original_failure() {
            return Err(source.clone());
        }
        let material = self
            .material_disposal
            .as_ref()
            .or_else(|| {
                self.conditional_retirement
                    .as_ref()
                    .map(|actual| actual.material_retirement())
            })
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        material.require_original(root.origin())?;
        let actual = root
            .acknowledge_approved_participant(completion_proof)
            .await?;
        // No await follows actual ACK: dropping the replaced Root is permitted
        // only after its domain completion + subtree disposal + storage proof.
        let completion = self
            .completion
            .take()
            .expect("actual retained completion borrowed through terminal ACK");
        self.state = Some(SigningTaskDomainState::Closed {
            _acknowledged: actual,
            _completion: completion,
        });
        Ok(())
    }
    async fn close_negative(&mut self) -> Result<(), AuraError> {
        self.dispose_native_original(true).await?;
        let Some(SigningTaskDomainState::Negative(owner)) = self.state.as_mut() else {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        };
        let native = self
            .native_disposal
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        native.require_original(owner.root.origin())?;
        if self.material_acquired
            && self.material_disposal.is_none()
            && self.conditional_retirement.is_none()
        {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        if let Some(actual) = self.material_disposal.as_ref() {
            actual.require_original(owner.root.origin())?;
        }
        if let Some(actual) = self.conditional_retirement.as_ref() {
            actual
                .material_retirement()
                .require_original(owner.root.origin())?;
        }
        owner.acknowledge_signing_refusal().await?;
        let actual = owner
            .root
            .signing_terminal
            .negative
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        if let Some(closed) = owner.root.signing_terminal.closed.as_ref() {
            let WindowCheckpoint::ApprovedSigning { capability } =
                owner.root.child.checkpoint.as_ref()
            else {
                return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
            };
            closed.require_original(capability)?;
        }
        if actual.retained_decision() == DurableEnrollmentExecutionState::Closed
            && self
                .completion
                .as_ref()
                .is_none_or(|actual| actual.original().is_err())
        {
            // A fresh original root cannot account for committed Closed while
            // silently losing its independently retained native completion.
            // Restart recovery has a distinct negative-only protected decision
            // variant and does not manufacture this in-process completion.
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        // Actual ACK is available before ownership moves. Struct construction
        // and state replacement are synchronous, with no callback/await gap.
        let previous = self.state.take();
        self.state = match previous {
            Some(SigningTaskDomainState::Negative(mut owner)) => {
                match (
                    owner.root.signing_terminal.negative.take(),
                    owner.root.signing_terminal.disposal.take(),
                ) {
                    (Some(actual), Some(disposal)) => {
                        Some(SigningTaskDomainState::NegativeAcknowledged(
                            AcknowledgedSigningNegativeCleanup {
                                _closed: owner.root.signing_terminal.closed.take(),
                                _cleanup_operation_failure: owner.cleanup_operation_failure,
                                _cleanup_validation_failure: owner.cleanup_validation_failure,
                                _cleanup_checkpoint_failure: owner.cleanup_checkpoint_failure,
                                _completion: self.completion.take(),
                                _disposal: disposal,
                                primary: owner.primary,
                                _receipt: actual,
                            },
                        ))
                    }
                    (receipt, disposal) => {
                        owner.root.signing_terminal.negative = receipt;
                        owner.root.signing_terminal.disposal = disposal;
                        self.state = Some(SigningTaskDomainState::Negative(owner));
                        return Err(
                            DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                        );
                    }
                }
            }
            other => other,
        };
        Ok(())
    }
    // Actual execute/cleanup trait implementation must await BORROWED root or
    // negative owner methods. Consuming-self async methods are forbidden here.
    // Successful Closed/Refused ACK moves the corresponding real receipt into
    // state synchronously, then consumes the old root with no further await.
    // A failed/cancelled future leaves the original state/root in this holder.
}

// The registered holder is the sole native participant owner. The concrete
// work method lives in threshold_signing, alongside the private grant producer;
// it receives no callback and exports no arbitrary signing entry point.
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalSigningTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original_participant())
    }
    fn cleanup<'a>(
        &'a mut self,
        original: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, AuraError>,
    > {
        Box::pin(self.cleanup_original(original))
    }
}

impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalSigningTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original_participant())
    }
    fn cleanup<'a>(
        &'a mut self,
        original: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, AuraError>,
    > {
        Box::pin(self.cleanup_original(original))
    }
}

impl OriginalSigningTaskCustody {
    async fn cleanup_original(
        &mut self,
        original: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, AuraError> {
        use aura_core::effects::task::RequiredTaskExit;
        use aura_core::effects::task::RequiredTaskSettlement;
        if matches!(
            self.state,
            Some(
                SigningTaskDomainState::Closed { .. }
                    | SigningTaskDomainState::NegativeAcknowledged(_)
            )
        ) {
            return self.retained_settlement();
        }
        match original {
            RequiredTaskExit::Completed
                if matches!(self.state, Some(SigningTaskDomainState::Positive(_))) =>
            {
                match self.close_positive().await {
                    Ok(()) => return Ok(RequiredTaskSettlement::Resolved),
                    Err(source) => {
                        self.begin_negative(source);
                    }
                }
            }
            RequiredTaskExit::Failed(source) => {
                self.begin_negative(source.clone());
            }
            RequiredTaskExit::Cancelled | RequiredTaskExit::Panicked => {
                // Actual registered execution supplied this terminal
                // disposition. It is a negative cause, not an approval.
                let source = self.original_execution_termination(original)?;
                self.begin_negative(source);
            }
            RequiredTaskExit::Completed => {}
        }
        self.close_negative().await?;
        self.retained_settlement()
    }

    fn retained_settlement(
        &self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, AuraError> {
        use aura_core::effects::task::RequiredTaskSettlement;
        match self.state.as_ref() {
            Some(SigningTaskDomainState::Closed { .. }) => Ok(RequiredTaskSettlement::Resolved),
            Some(SigningTaskDomainState::NegativeAcknowledged(actual)) => Ok(
                RequiredTaskSettlement::ResolvedWithDomainFailure(actual.primary.clone()),
            ),
            _ => Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum OriginalParticipantTermination {
    #[error("original approved participant execution was cancelled")]
    Cancelled,
    #[error("original approved participant execution panicked")]
    Panicked,
}
impl OriginalSigningTaskCustody {
    fn original_execution_termination(
        &self,
        original: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<AuraError, AuraError> {
        let source = match original {
            aura_core::effects::task::RequiredTaskExit::Cancelled => {
                OriginalParticipantTermination::Cancelled
            }
            aura_core::effects::task::RequiredTaskExit::Panicked => {
                OriginalParticipantTermination::Panicked
            }
            _ => return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()),
        };
        Ok(AuraError::Internal {
            message: "original approved participant execution interrupted".into(),
            source: Some(Arc::new(source)),
        })
    }
}

impl OriginalSigningTaskCustody {
    async fn prepare_original_conditional_participant(&mut self) -> Result<(), AuraError> {
        let Some(SigningTaskDomainState::Positive(root)) = self.state.as_ref() else {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        };
        let approval = root.origin_observation();
        let original = root.child();
        let completion = self
            .completion
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        completion.original()?.require_original(&approval)?;
        if let Some(source) = self.native_cleanup_failure.as_ref() {
            return Err(source.clone());
        }
        if let Some(source) = self.native_cleanup_validation_failure.as_ref() {
            return Err(AuraError::from(source.clone()));
        }
        if let Some(retired) = self.conditional_retirement.as_ref() {
            retired.require_original_window(original)?;
            let disposal = self.native_disposal.as_ref().ok_or_else(|| {
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
            })?;
            disposal.require_original(&approval)?;
            if let Some(source) = disposal.original_failure() {
                return Err(source.clone());
            }
            return Ok(());
        }
        if self.conditional_material.is_none() {
            let material = self.material.take().ok_or_else(|| {
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
            })?;
            match material.transfer_original_negative(&approval, Arc::clone(completion), original) {
                Ok(actual) => self.conditional_material = Some(actual),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.material = Some(actual);
                    return Err(source);
                }
            }
        }
        let material = self
            .conditional_material
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        material.require_original(&approval)?;
        let native = &mut self.native;
        let disposal = &mut self.native_disposal;
        let first_failure = &mut self.native_cleanup_failure;
        let observed = original
            .execute_original_negative_terminal(material, &approval, || async {
                if let Some(actual) = native.take() {
                    match actual.dispose_original(&approval) {
                        Ok(actual) => *disposal = Some(actual),
                        Err(rejected) => {
                            let (actual, source) = *rejected;
                            *native = Some(actual);
                            first_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    }
                }
                let actual_disposal = disposal.as_mut().ok_or_else(|| {
                    DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                })?;
                self.peer_continuation
                    .lock()
                    .await
                    .retain_original_native_reentry(actual_disposal)?;
                let actual = disposal.as_ref().ok_or_else(|| {
                    DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
                })?;
                actual.require_original(&approval)?;
                if let Some(source) = actual.original_failure() {
                    return Err(source.clone());
                }
                Ok(())
            })
            .await?;
        // Actual native disposal is retained by the holder before the executor
        // can observe completion. Keep operation and validation independently.
        self.conditional_operation = observed.operation;
        self.conditional_validation = Some(observed.validation);
        if let Some(Err(source)) = self.conditional_operation.as_ref() {
            self.native_cleanup_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Some(Err(source)) = self.conditional_validation.as_ref() {
            self.native_cleanup_validation_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Some(source) = self.native_cleanup_failure.as_ref() {
            return Err(source.clone());
        }
        if let Some(source) = self.native_cleanup_validation_failure.as_ref() {
            return Err(AuraError::from(source.clone()));
        }
        self.conditional_validation
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?
            .clone()
            .map_err(AuraError::from)?;
        self.conditional_operation
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?
            .clone()
    }

    async fn run_original_conditional_participant(&mut self) -> Result<(), AuraError> {
        let Some(SigningTaskDomainState::Positive(root)) = self.state.as_ref() else {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        };
        let approval = root.origin_observation();
        let original = root.child();
        let disposal = self
            .native_disposal
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        let actor = self
            .participant_actor
            .as_mut()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        let ingress = self
            .ingress
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        // Both futures borrow the same retained actor/material/root. Initial
        // signing completion never substitutes for this task's terminal state.
        if self.conditional_retirement.is_none() {
            let material = self.conditional_material.as_ref().ok_or_else(|| {
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
            })?;
            let (actor_exit, dispatcher_exit) = tokio::try_join!(
                actor.run_negative(material, &approval, original),
                self.conditional_dispatcher
                    .run_negative(material, &approval, original, disposal, ingress),
            )?;
            use super::threshold_signing::{
                OriginalConditionalActorExit, OriginalConditionalDispatcherExit,
            };
            match (actor_exit, dispatcher_exit) {
                (
                    OriginalConditionalActorExit::NegativeSigned,
                    OriginalConditionalDispatcherExit::NegativeSigned,
                ) => return Ok(()),
                (
                    OriginalConditionalActorExit::PositiveRetirement,
                    OriginalConditionalDispatcherExit::RetirementRequested,
                ) => {}
                _ => {
                    return Err(AuraError::permission_denied(
                        "original conditional actor and dispatcher terminal dispositions differ",
                    ));
                }
            }
            // The joined futures have ended. Consume the same original material
            // synchronously and deposit actual retirement before any next await.
            let (request, acknowledgment) = self.conditional_dispatcher.retirement_binding()?;
            let material = self.conditional_material.take().ok_or_else(|| {
                DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal()
            })?;
            match material.retire_original_conditional_transport(
                approval.clone(),
                original,
                request,
                acknowledgment,
            ) {
                Ok(actual) => self.conditional_retirement = Some(Arc::new(actual)),
                Err(rejected) => {
                    let (actual, source) = *rejected;
                    self.conditional_material = Some(actual);
                    return Err(source);
                }
            }
        }
        let retired = self
            .conditional_retirement
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
        original
            .execute(approval.effects().as_ref(), || async {
                retired.require_original_window(original)?;
                retired.material_retirement().require_original(&approval)?;
                let mut peer = self.peer_continuation.lock().await;
                if let Some(actual) = &peer.conditional_retirement {
                    if !Arc::ptr_eq(actual, retired) {
                        return Err(AuraError::permission_denied(
                            "original conditional retirement changed",
                        ));
                    }
                } else {
                    peer.conditional_retirement = Some(Arc::clone(retired));
                }
                Ok(())
            })
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "retain actual peer conditional retirement",
                    Arc::new(source),
                )
            })?;
        if self.conditional_retirement_actor_ack.is_none() {
            let actual = actor.acknowledge_conditional_material_retirement(
                retired.material_retirement(),
                &approval,
            );
            self.conditional_retirement_actor_ack = Some(actual);
        }
        self.conditional_retirement_actor_ack
            .as_ref()
            .ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?
            .clone()?;
        self.conditional_dispatcher
            .resume_retirement_reply(retired, original, ingress)
            .await?;
        self.conditional_dispatcher
            .send_retirement_ack(retired, original)
            .await?;
        if let Some(source) = &self.positive_readiness_failure {
            return Err(source.clone());
        }
        let peer_changed = &self.peer_changed;
        let observed = self
            .positive_owner
            .execute(original, &approval, |cell| async move {
                loop {
                    let changed = peer_changed.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    if cell.lock().await.peer_activation_ready()? {
                        return Ok::<(), AuraError>(());
                    }
                    changed.await;
                }
            })
            .await;
        let observed = match observed {
            Ok(actual) => actual,
            Err(source) => {
                self.positive_readiness_failure
                    .get_or_insert_with(|| source.clone());
                return Err(source);
            }
        };
        if let Some(Err(source)) = &observed.operation {
            self.positive_readiness_failure
                .get_or_insert_with(|| source.clone());
        }
        if let Err(source) = &observed.validation {
            self.positive_readiness_failure
                .get_or_insert_with(|| AuraError::from(source.clone()));
        }
        self.positive_readiness_operation = observed.operation;
        self.positive_readiness_validation = Some(observed.validation);
        if let Some(source) = &self.positive_readiness_failure {
            return Err(source.clone());
        }
        self.positive_readiness_validation
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original peer readiness validation absent"))?
            .clone()
            .map_err(AuraError::from)?;
        self.positive_readiness_operation
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original peer readiness interrupted"))?
            .clone()?;
        super::threshold_signing::OriginalEnrollmentPeerContinuation::finish_original_positive(
            &self.positive_owner,
            original,
            &approval,
            actor,
            &mut self.positive_dispatcher,
            ingress,
        )
        .await?;
        Ok(())
    }
}

// This implementation belongs in the threshold_signing owner module so its
// private native grant producer remains inaccessible outside that owner.
impl OriginalSigningTaskCustody {
    async fn execute_original_participant(&mut self) -> Result<(), AuraError> {
        let outcome = async {
            let Some(SigningTaskDomainState::Positive(root)) = self.state.as_ref() else {
                return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
            };
            let approval = root.origin();
            if self.initial_transport_completed {
                self.completion.as_ref().ok_or_else(||
                    DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?
                    .original()?.require_original(approval)?;
                return Ok(());
            }
            let original = root.child();
            let effects = approval.effects();
            let native = self.native.as_mut().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
            original.execute(effects.as_ref(), || native.acquire_original(approval))
                .await.map_err(|source| AuraError::crypto_with_source(
                    "original approved participant custody acquisition failed", Arc::new(source),
                ))?;
            if self.material.is_none() {
                let retained = &mut self.material;
                let material_acquired = &mut self.material_acquired;
                let first_failure = &mut self.first_operation_failure;
                original.execute(effects.as_ref(), || async {
                    let material = match self.service.admit_original_owned_enrollment_material(approval, native).await {
                        Ok(actual) => actual,
                        Err(source) => { first_failure.get_or_insert_with(|| source.clone()); return Err(source); }
                    };
                    *retained = Some(material);
                    *material_acquired = true;
                    Ok::<(), AuraError>(())
                }).await.map_err(|source| AuraError::crypto_with_source(
                    "original approved participant material admission failed", Arc::new(source),
                ))?;
            }
            let material = self.material.as_ref().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
            original.execute(effects.as_ref(), || async {
                let authenticated = self.native.as_ref().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?.original_authenticated(approval)?;
                self.peer_continuation.lock().await.retain_original_native_inventory(material, authenticated.original_baseline(approval)?)
            }).await.map_err(|source| AuraError::crypto_with_source("retain original peer native inventory", Arc::new(source)))?;
            let completion = self.completion.as_ref().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
            if self.participant_actor.is_none() {
                let (ingress, actor) = material.original_participant_actor();
                self.ingress = Some(ingress);
                self.participant_actor = Some(actor);
            }
            let ingress = self.ingress.as_ref().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?.clone();
            let actor = self.participant_actor.as_mut().ok_or_else(|| DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal())?;
            let participant = actor.run_initial(material.approved_local_transcript(approval, original)?, Arc::clone(completion));
            if let Some(ready) = self.ready_sender.take() {
                ready.send(Ok(ingress.clone())).map_err(|_| AuraError::Crypto {
                    message: "original participant readiness receiver ended".into(),
                    source: Some(Arc::new(super::threshold_signing::EnrollmentTranscriptSigningError::ResponseCancelled)),
                })?;
            }
            let dispatcher = material.dispatch_original_participant(approval, original, &ingress);
            tokio::try_join!(participant, dispatcher)?;
            self.initial_transport_completed = true;
            Ok(())
        }.await;
        let outcome = match outcome {
            Ok(()) => match self.prepare_original_conditional_participant().await {
                Ok(()) => self.run_original_conditional_participant().await,
                Err(source) => Err(source),
            },
            Err(source) => Err(source),
        };
        if let Err(source) = &outcome {
            self.first_operation_failure
                .get_or_insert_with(|| source.clone());
            if let Some(ready) = self.ready_sender.take() {
                let _ = ready.send(Err(source.clone()));
            }
        }
        outcome
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) async fn assert_original_native_failure_for_test(
    issuer: Arc<crate::AuraAgent>,
    approval: crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
) {
    use aura_core::effects::secure::{SecureStorageLocation, SecureStorageRecordMissing};
    let effects = issuer.runtime().effects();
    let epoch = approval.manifest().final_epoch;
    let root = EnrollmentExecutionRoot::approved_signing(
        effects.clone(),
        approval,
        &issuer.runtime().tasks().group("actual-native-failure"),
    )
    .await
    .expect("actual original approved signing root");
    let (ready, _receiver) = tokio::sync::oneshot::channel();
    let mut holder = OriginalSigningTaskCustody::admit(
        root,
        issuer.runtime().threshold_signing().clone(),
        ready,
    );
    let location = SecureStorageLocation::with_sub_key(
        "threshold_config",
        issuer.authority_id().to_string(),
        epoch.to_string(),
    );
    assert!(effects
        .fault_remove_secure_record_for_test(&location)
        .await
        .expect("actual native policy record removed"));
    let root = match holder
        .state
        .as_ref()
        .expect("same positive task domain retained")
    {
        SigningTaskDomainState::Positive(root) => root,
        _ => panic!("original positive root"),
    };
    let failure = holder
        .native
        .as_mut()
        .expect("actual native admission owner retained")
        .acquire_original(root.origin())
        .await
        .expect_err("actual required policy missing");
    fn missing(error: &AuraError) -> &SecureStorageRecordMissing {
        let mut current: &(dyn std::error::Error + 'static) = error;
        loop {
            if let Some(actual) = current.downcast_ref::<SecureStorageRecordMissing>() {
                return actual;
            }
            current = current.source().expect("concrete missing policy source");
        }
    }
    assert_eq!(missing(&failure).location(), &location);
    let generation = effects.enrollment_retirement_generation_guard();
    futures::pin_mut!(generation);
    assert!(
        futures::poll!(generation.as_mut()).is_pending(),
        "actual failed acquisition retains its original generation lock"
    );
    holder
        .dispose_native_original(false)
        .await
        .expect("actual native guard disposal succeeds");
    drop(generation.await);
    assert!(holder.native.is_none());
    assert!(std::ptr::eq(
        missing(
            holder
                .native_disposal
                .as_ref()
                .expect("actual retained native disposal receipt")
                .original_failure()
                .expect("original missing-policy failure retained")
        ),
        missing(&failure),
    ));
    let positive = holder
        .close_positive()
        .await
        .expect_err("no completed participant can positively close");
    assert!(matches!(
        std::error::Error::source(&positive)
            .expect("positive refusal has concrete source")
            .downcast_ref::<super::threshold_signing::EnrollmentTranscriptSigningError>(),
        Some(super::threshold_signing::EnrollmentTranscriptSigningError::ResponseCancelled)
    ));
    holder.begin_negative(AuraError::invalid("later terminal refusal"));
    let Some(SigningTaskDomainState::Negative(owner)) = holder.state.as_ref() else {
        panic!("same root negative")
    };
    let primary = &owner.primary;
    assert!(std::ptr::eq(missing(primary), missing(&failure)));
    holder
        .close_negative()
        .await
        .expect("actual original Refused checkpoint and descendant disposal");
    let Some(SigningTaskDomainState::NegativeAcknowledged(actual)) = holder.state.as_ref() else {
        panic!("actual negative acknowledgment")
    };
    assert_eq!(
        actual._receipt.retained_decision(),
        DurableEnrollmentExecutionState::Refused
    );
    let primary = &actual.primary;
    assert!(std::ptr::eq(missing(primary), missing(&failure)));
    let aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(actual) =
        holder
            .retained_settlement()
            .expect("actual negative settlement retained")
    else {
        panic!("cleanup must not turn failed acquisition into success")
    };
    assert!(std::ptr::eq(missing(&actual), missing(&failure)));
}

/// Identity only: retains the actual original checkpoint and observation owner.
/// It exposes neither an executor nor a timeout budget.
pub(super) struct OriginalConditionalSigningWindowIdentity {
    checkpoint: Arc<WindowCheckpoint>,
    original: TimeoutBudget,
}
impl OriginalConditionalSigningWindowIdentity {
    pub(super) fn require_child(&self, child: &EnrollmentExecutionChild) -> Result<(), AuraError> {
        let same_checkpoint = match (self.checkpoint.as_ref(), child.checkpoint.as_ref()) {
            (
                WindowCheckpoint::HeldIssuer {
                    capability: original,
                }
                | WindowCheckpoint::Registered {
                    capability: original,
                },
                WindowCheckpoint::HeldIssuer {
                    capability: supplied,
                }
                | WindowCheckpoint::Registered {
                    capability: supplied,
                },
            ) => Arc::ptr_eq(original, supplied),
            (
                WindowCheckpoint::ApprovedSigning {
                    capability: original,
                },
                WindowCheckpoint::ApprovedSigning {
                    capability: supplied,
                },
            ) => Arc::ptr_eq(original, supplied),
            _ => false,
        };
        if !same_checkpoint || !self.original.shares_observation_owner_with(&child.original) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        Ok(())
    }
}
impl EnrollmentExecutionChild {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "approved_original_conditional_signing_window",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(super) fn retain_original_conditional_identity(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<OriginalConditionalSigningWindowIdentity, AuraError> {
        self.require_execution_effects(approval.effects().as_ref())?;
        match self.checkpoint.as_ref() {
            WindowCheckpoint::HeldIssuer { capability }
            | WindowCheckpoint::Registered { capability } => {
                capability.require_approved_manifest(approval)?;
            }
            WindowCheckpoint::ApprovedSigning { capability } => {
                capability.require_approved_manifest(approval)?;
            }
            _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        }
        Ok(OriginalConditionalSigningWindowIdentity {
            checkpoint: Arc::clone(&self.checkpoint),
            original: self.original.clone(),
        })
    }
}
impl EnrollmentExecutionChild {
    /// Observe only the already-transferred conditional signing owner. Its
    /// custody is the primary here; this does not assert a negative decision.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalNegativeTerminalSigningMaterial",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(super) async fn execute_original_negative_terminal<'owner, T, F, Fut>(
        &self,
        material: &'owner super::threshold_signing::OriginalNegativeTerminalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'owner super::threshold_signing::OriginalNegativeTerminalSigningMaterial,
            T,
            AuraError,
        >,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        material.require_original(approval)?;
        material.require_original_window(self)?;
        let effects = approval.effects().as_ref();
        let _original_work = self._lease.borrow_execution()?;
        self.require_execution_effects(effects)?;
        let original_cleanup = match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => capability.negative_cleanup_budget(),
            WindowCheckpoint::ApprovedSigning { capability } => {
                capability.negative_cleanup_budget()
            }
            _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        };
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects,
                original_cleanup,
                material,
                operation,
            )
            .await,
        )
    }

    /// Final-control work retains its actual original positive signing owner.
    /// The original cleanup endpoint remains usable after the protected commit;
    /// this full outcome does not acknowledge completion or share retirement.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalPositiveTerminalSigningMaterial",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(super) async fn execute_original_positive_terminal<'owner, T, F, Fut>(
        &self,
        material: &'owner super::threshold_signing::OriginalPositiveTerminalSigningMaterial,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        operation: F,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'owner super::threshold_signing::OriginalPositiveTerminalSigningMaterial,
            T,
            AuraError,
        >,
        AuraError,
    >
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, AuraError>>,
    {
        material.require_original(approval, self)?;
        let effects = approval.effects().as_ref();
        let _original_work = self._lease.borrow_execution()?;
        self.require_execution_effects(effects)?;
        let original_cleanup = match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => capability.negative_cleanup_budget(),
            WindowCheckpoint::ApprovedSigning { capability } => {
                capability.negative_cleanup_budget()
            }
            _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        };
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects,
                original_cleanup,
                material,
                operation,
            )
            .await,
        )
    }
}

impl EnrollmentExecutionChild {
    /// Acquire only the signing state already bound to the original issued
    /// control, under the same registered cleanup endpoint. The full outcome
    /// keeps lock acquisition separate from deadline validation.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RetainedEnrollmentVmControl",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(crate) async fn acquire_original_conditional_signing<'owner>(
        &self,
        effects: &AuraEffectSystem,
        issued: &'owner crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<
        aura_core::time::timeout::NegativeCleanupExecution<
            &'owner crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
            tokio::sync::OwnedMutexGuard<
                super::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
            AuraError,
        >,
        AuraError,
    > {
        self.require_execution_effects(effects)?;
        self.require_issued_notice_owner(issued)?;
        let original_state = issued.original_conditional_signing_state(effects, self)?;
        let original_cleanup = match self.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => capability.negative_cleanup_budget(),
            _ => return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        };
        let _original_work = self._lease.borrow_execution()?;
        Ok(
            aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
                effects,
                original_cleanup,
                issued,
                || async move {
                    let acquired = original_state.lock_owned().await;
                    issued.retain_original_conditional_lock_ready()?;
                    Ok(acquired)
                },
            )
            .await,
        )
    }
}

/// Classification is revalidated against the original private receipt cells;
/// this enum alone cannot admit cleanup or construct a terminal acknowledgment.
#[derive(Clone, Copy)]
pub(crate) enum OriginalIssuerResourceDecision {
    Committed,
    VerifiedRejected,
    Interrupted,
}
impl OriginalIssuerResourceDecision {
    pub(super) fn require_original(
        self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        match self {
            Self::Interrupted => match child.checkpoint.as_ref() {
                WindowCheckpoint::Registered { capability } => {
                    capability.require_original_interrupted_resources(issued)
                }
                _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
            },
            Self::Committed => child
                .original_positive_decision(issued)?
                .require_original_control(issued),
            Self::VerifiedRejected => {
                let receipt = child.original_negative_decision(issued)?;
                let original = receipt
                    .original_owner_for_control(issued)?
                    .upgrade()
                    .ok_or_else(|| AuraError::permission_denied("original rejection owner lost"))?;
                original.require_original_verified_rejection(issued)
            }
        }
    }
    fn original_owner(
        self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &EnrollmentExecutionChild,
    ) -> Result<std::sync::Weak<RegisteredEnrollmentWindowCapability>, AuraError> {
        self.require_original(issued, child)?;
        match self {
            Self::Interrupted => match child.checkpoint.as_ref() {
                WindowCheckpoint::Registered { capability } => Ok(Arc::downgrade(capability)),
                _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
            },
            Self::Committed => child
                .original_positive_decision(issued)?
                .original_owner_for_control(issued),
            Self::VerifiedRejected => child
                .original_negative_decision(issued)?
                .original_owner_for_control(issued),
        }
    }
    pub(crate) fn from_original(
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &EnrollmentExecutionChild,
    ) -> Result<Self, AuraError> {
        child.require_issued_notice_owner(issued)?;
        match child.checkpoint.as_ref() {
            WindowCheckpoint::Registered { capability } => {
                capability.original_resource_decision(issued)
            }
            _ => Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal()),
        }
    }
}
/// Retains original terminal execution until its original resources close.
/// No signed terminal/domain acknowledgment can be extracted from this owner.
pub(crate) struct OriginalIssuerResourceCleanupOwner {
    root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
    issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
    signing: Option<
        Arc<tokio::sync::Mutex<super::threshold_signing::OriginalIssuerTranscriptSigningCustody>>,
    >,
    primary: AuraError,
    decision: Option<OriginalIssuerResourceDecision>,
    destination: Arc<super::issuer_terminal_destination::OriginalIssuerTerminalDestination>,
    interruption_guard: Option<tokio::sync::OwnedMutexGuard<()>>,
    generation: Option<super::ceremony_tracker::OriginalEnrollmentRetirementAcknowledgment>,
    refused_generation: Option<super::ceremony_tracker::RejectedOriginalEnrollmentRetirement>,
    sealed: Option<crate::task_registry::SealedTaskGroup>,
    disposal: Option<crate::task_registry::TaskGroupDisposalAcknowledgment>,
    retirement: Option<OriginalIssuerSigningResourceSettlement>,
    closure: Option<super::ceremony_tracker::OriginalIssuerResourceClosure>,
    first_failure: Option<AuraError>,
    work_completed: bool,
    settled_failure: Option<AuraError>,
}
impl OriginalIssuerResourceCleanupOwner {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_terminal_issuer_root",
        capability_type = EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        family = "runtime_helper"
    )]
    pub(crate) fn from_original(
        root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        signing: Option<
            Arc<
                tokio::sync::Mutex<
                    super::threshold_signing::OriginalIssuerTranscriptSigningCustody,
                >,
            >,
        >,
        primary: AuraError,
        destination: Arc<super::issuer_terminal_destination::OriginalIssuerTerminalDestination>,
    ) -> Result<
        Self,
        Box<(
            EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
            AuraError,
        )>,
    > {
        let decision = match OriginalIssuerResourceDecision::from_original(&issued, &root.child) {
            Ok(decision) => decision,
            Err(source) => return Err(Box::new((root, source))),
        };
        Ok(Self {
            root,
            issued,
            signing,
            primary,
            decision: Some(decision),
            destination,
            interruption_guard: None,
            generation: None,
            refused_generation: None,
            sealed: None,
            disposal: None,
            retirement: None,
            closure: None,
            first_failure: None,
            work_completed: false,
            settled_failure: None,
        })
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "original_interrupted_issuer_root", capability_type = EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>, family = "runtime_helper")]
    pub(crate) fn from_interrupted_original(
        root: EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability>,
        issued: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        signing: Option<
            Arc<
                tokio::sync::Mutex<
                    super::threshold_signing::OriginalIssuerTranscriptSigningCustody,
                >,
            >,
        >,
        primary: AuraError,
        destination: Arc<super::issuer_terminal_destination::OriginalIssuerTerminalDestination>,
    ) -> Self {
        Self {
            root,
            issued,
            signing,
            primary,
            destination,
            decision: None,
            interruption_guard: None,
            generation: None,
            refused_generation: None,
            sealed: None,
            disposal: None,
            retirement: None,
            closure: None,
            first_failure: None,
            work_completed: false,
            settled_failure: None,
        }
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerResourceCleanupOwner", receiver_type = OriginalIssuerResourceCleanupOwner, family = "runtime_helper")]
    pub(crate) async fn settle(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, AuraError> {
        use aura_core::time::timeout::{
            acknowledge_negative_cleanup_with_timeout_budget,
            execute_negative_cleanup_with_timeout_budget, CheckpointAttempt,
        };
        self.root.origin.require_effects(effects.as_ref())?;
        if let Some(failure) = self.settled_failure.as_ref() {
            return Ok(
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    failure.clone(),
                ),
            );
        }
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }

        if self.sealed.is_none() {
            self.root.children.request_cancellation();
            self.sealed = Some(self.root.children.seal_admission());
        }
        let budget = self.root.origin.negative_cleanup_budget();
        // The VM was opened by this required task. Cleanup borrows its retained
        // slot under that same registered binding, after disposing its children.
        let root = &self.root;
        let disposal = &mut self.disposal;
        let sealed = &self.sealed;
        let destination = &self.destination;
        let issued = &self.issued;
        let decision = &mut self.decision;
        let interruption_guard = &mut self.interruption_guard;
        #[cfg(test)]
        eprintln!("original issuer resources: await descendants and original VM");
        let prepared = execute_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.primary,
            || async {
                if disposal.is_none() {
                    *disposal = Some(
                        sealed
                            .as_ref()
                            .ok_or_else(|| AuraError::invalid("original subtree seal missing"))?
                            .await_disposal()
                            .await,
                    );
                }
                let disposal = disposal
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original subtree disposal missing"))?;
                disposal.require_original_group(&root.children)?;
                destination
                    .dispose_original_vm(effects.as_ref(), issued, &root.child)
                    .await?;
                #[cfg(test)]
                eprintln!("original issuer resources: descendants and original VM disposed");
                if decision.is_none() {
                    *interruption_guard = Some(
                        root.origin
                            .fence_original_interrupted_resources(
                                issued,
                                destination,
                                disposal,
                                &root.children,
                            )
                            .await?,
                    );
                    *decision = Some(OriginalIssuerResourceDecision::Interrupted);
                }
                decision
                    .ok_or_else(|| AuraError::invalid("original resource decision missing"))?
                    .require_original(issued, &root.child)
            },
        )
        .await;
        match prepared.operation {
            Some(Ok(())) => {}
            Some(Err(source)) => return Err(source),
            None => {
                return Err(AuraError::invalid(
                    "original VM and subtree preparation incomplete",
                ))
            }
        }
        prepared.validation.map_err(AuraError::from)?;
        if self.decision.is_none() {
            return Err(AuraError::invalid(
                "original resource preparation incomplete",
            ));
        }
        let decision = self
            .decision
            .ok_or_else(|| AuraError::invalid("original resource decision missing"))?;
        let child = &self.root.child;
        let issued = &self.issued;
        let children = &self.root.children;
        if !self.work_completed {
            let original_work = self.root.child._lease.borrow_execution()?;
            let disposal = &mut self.disposal;
            let retirement = &mut self.retirement;
            let generation = &mut self.generation;
            let refused_generation = &mut self.refused_generation;
            let original = &self.root.origin;
            let failure = &mut self.first_failure;
            let sealed = self
                .sealed
                .as_ref()
                .ok_or_else(|| AuraError::invalid("committed resource subtree seal missing"))?;
            let child = &self.root.child;
            let issued = &self.issued;
            let signing = &self.signing;
            let children = &self.root.children;
            let work = execute_negative_cleanup_with_timeout_budget(
                effects,
                budget,
                &self.primary,
                || async {
                    let result = async {
                        if disposal.is_none() {
                            *disposal = Some(sealed.await_disposal().await);
                        }
                        disposal
                            .as_ref()
                            .ok_or_else(|| {
                                AuraError::invalid("committed resource subtree disposal missing")
                            })?
                            .require_original_group(children)?;
                        if retirement.is_none() {
                            *retirement = Some(
                                if issued.original_conditional_signing_required(effects.as_ref(), child)? {
                                    let signing = signing.as_ref().ok_or_else(|| {
                                        AuraError::invalid("committed original quorum signing resource owner missing")
                                    })?;
                                    OriginalIssuerSigningResourceSettlement::QuorumRetired(
                                        signing.lock().await.retire_original_issuer_resources(issued, child, decision)?,
                                    )
                                } else {
                                    if signing.is_some() {
                                        return Err(AuraError::permission_denied(
                                            "solo original unexpectedly retains quorum resource custody",
                                        ));
                                    }
                                    child.single_signer_resource_obligation(issued, effects, decision)?
                                },
                            );
                        }
                        if matches!(decision, OriginalIssuerResourceDecision::VerifiedRejected) && generation.is_none() {
                            if let Some(rejected) = refused_generation.as_ref() {
                                return Err(rejected.source.clone());
                            }
                            #[cfg(test)]
                            eprintln!("original issuer resources: begin rejected generation retirement");
                            let writes = original.retire_original_rejected_generation(issued).await?;
                            match original.acknowledge_original_retirement(writes, issued) {
                                Ok(actual) => {
                                    *generation = Some(actual);
                                    #[cfg(test)]
                                    eprintln!("original issuer resources: rejected generation retired");
                                },
                                Err(rejected) => {
                                    let source = rejected.source.clone();
                                    *refused_generation = Some(rejected);
                                    return Err(source);
                                }
                            }
                        }
                        retirement
                            .as_ref()
                            .ok_or_else(|| {
                                AuraError::invalid("committed signing retirement missing")
                            })?
                            .require_original(issued, child)
                    }
                    .await;
                    if let Err(source) = &result {
                        failure.get_or_insert_with(|| source.clone());
                    }
                    result
                },
            )
            .await;
            if let Err(source) = work.validation {
                let source = AuraError::from(source);
                self.first_failure.get_or_insert_with(|| source.clone());
                return Err(source);
            }
            match work.operation {
                Some(Ok(())) => {}
                Some(Err(source)) => return Err(source),
                None => return Err(AuraError::invalid("committed resource work unresolved")),
            }
            drop(original_work);
            self.work_completed = true;
        }
        let generation = self.generation.as_ref();
        let closure = &mut self.closure;
        let original = &self.root.origin;
        let retirement = self
            .retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("committed signing retirement missing"))?;
        let disposal = self
            .disposal
            .as_ref()
            .ok_or_else(|| AuraError::invalid("committed subtree disposal missing"))?;
        let checkpoint_failure = &mut self.first_failure;
        #[cfg(test)]
        eprintln!("original issuer resources: begin resource closure");
        let observation = acknowledge_negative_cleanup_with_timeout_budget(
            effects,
            budget,
            &self.primary,
            || async {
                if closure.is_none() {
                    match original
                        .close_original_issuer_resources(
                            issued, child, retirement, decision, generation, disposal, children,
                        )
                        .await
                    {
                        Ok(receipt) => *closure = Some(receipt),
                        Err(source) => {
                            checkpoint_failure.get_or_insert_with(|| source.clone());
                            return Err(TimeoutBudgetError::checkpoint_failure(source));
                        }
                    }
                }
                closure.as_ref().ok_or_else(|| {
                    TimeoutBudgetError::invalid_policy("committed resource closure missing")
                })
            },
        )
        .await;
        if let Err(source) = observation.validation {
            let source = AuraError::from(source);
            self.first_failure.get_or_insert_with(|| source.clone());
            return Err(source);
        }
        match observation.checkpoint {
            CheckpointAttempt::Returned(receipt) => {
                original.release_issuer_resource_execution(receipt)?;
            }
            CheckpointAttempt::Failed { source } => {
                let source = AuraError::from(source);
                self.first_failure.get_or_insert_with(|| source.clone());
                return Err(source);
            }
            _ => {
                return Err(AuraError::invalid(
                    "committed resource checkpoint unresolved",
                ));
            }
        }
        // All original descendants are disposed before retaining the final
        // failure graph, including a finalizer that failed after observation.
        let failure = self.destination.with_retained_finalizer_failure(
            self.destination
                .with_retained_vm_failures(self.primary.clone()),
        );
        self.settled_failure = Some(failure.clone());
        self.interruption_guard = None;
        Ok(aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(failure))
    }
}

/// Finite resource obligations preserve the actual original signing mode.
/// Solo absence is checked through the sealed issued owner, never inferred from
/// a missing quorum state. Neither variant represents a terminal signature.
pub(super) enum OriginalIssuerSigningResourceSettlement {
    QuorumRetired(super::threshold_signing::OriginalIssuerSigningResourceRetirement),
    SingleSignerNoConditionalSession {
        issued: std::sync::Weak<
            crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        >,
        original: std::sync::Weak<RegisteredEnrollmentWindowCapability>,
        effects: std::sync::Weak<AuraEffectSystem>,
    },
}
impl OriginalIssuerSigningResourceSettlement {
    pub(super) fn require_original(
        &self,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        child: &EnrollmentExecutionChild,
    ) -> Result<(), AuraError> {
        match self {
            Self::QuorumRetired(actual) => actual.require_original(issued, child),
            Self::SingleSignerNoConditionalSession {
                issued: retained,
                original,
                effects,
            } => {
                let retained = retained.upgrade().ok_or_else(|| {
                    AuraError::permission_denied("solo resource control owner was lost")
                })?;
                if !std::ptr::eq(retained.as_ref(), issued) {
                    return Err(AuraError::permission_denied(
                        "solo resource control owner differs",
                    ));
                }
                let effects = effects.upgrade().ok_or_else(|| {
                    AuraError::permission_denied("solo resource runtime owner was lost")
                })?;
                if issued.original_conditional_signing_required(effects.as_ref(), child)? {
                    return Err(AuraError::permission_denied(
                        "solo resource proof cannot discharge quorum custody",
                    ));
                }
                let decision = OriginalIssuerResourceDecision::from_original(issued, child)?;
                let expected = decision
                    .original_owner(issued, child)?
                    .upgrade()
                    .ok_or_else(|| {
                        AuraError::permission_denied("solo original receipt owner was lost")
                    })?;
                if !original
                    .upgrade()
                    .is_some_and(|actual| Arc::ptr_eq(&actual, &expected))
                {
                    return Err(AuraError::permission_denied(
                        "solo original resource window differs",
                    ));
                }
                Ok(())
            }
        }
    }
}
impl EnrollmentExecutionChild {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "RetainedEnrollmentVmControl", receiver_type = EnrollmentExecutionChild, family = "runtime_helper")]
    fn single_signer_resource_obligation(
        &self,
        issued: &Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        effects: &Arc<AuraEffectSystem>,
        decision: OriginalIssuerResourceDecision,
    ) -> Result<OriginalIssuerSigningResourceSettlement, AuraError> {
        self.require_execution_effects(effects.as_ref())?;
        if issued.original_conditional_signing_required(effects.as_ref(), self)? {
            return Err(AuraError::permission_denied(
                "original quorum resource retirement is required",
            ));
        }
        decision.require_original(issued, self)?;
        Ok(
            OriginalIssuerSigningResourceSettlement::SingleSignerNoConditionalSession {
                issued: Arc::downgrade(issued),
                original: decision.original_owner(issued, self)?,
                effects: Arc::downgrade(effects),
            },
        )
    }
}

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

mod signing_checkpoint;

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
        let _ = <crate::handlers::invitation::IssuerEnrollmentTerminalReceipt as AmbiguousIfClone<_>>::marker;
        let _ = <super::EnrollmentExecutionRoot<super::RegisteredEnrollmentWindowCapability> as AmbiguousIfClone<_>>::marker;
        let _ = <super::AcknowledgedIssuerEnrollmentWindow as AmbiguousIfClone<_>>::marker;

        struct DeserializeImplemented;
        trait AmbiguousIfDeserialize<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
        impl<T: serde::de::DeserializeOwned> AmbiguousIfDeserialize<DeserializeImplemented> for T {}
        let _ = <HeldIssuerCompletionObserver as AmbiguousIfDeserialize<_>>::marker;
        let _ = <crate::handlers::invitation::IssuerEnrollmentTerminalReceipt as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::EnrollmentExecutionRoot<super::RegisteredEnrollmentWindowCapability> as AmbiguousIfDeserialize<_>>::marker;
        let _ = <super::AcknowledgedIssuerEnrollmentWindow as AmbiguousIfDeserialize<_>>::marker;
    }
}

/// Restricted original issuer completion observation. No execution permit,
/// Clone/Deserialize, arbitrary executor, child, nonce or session API is exposed.
pub(crate) struct HeldIssuerCompletionObserver {
    active: TimeoutBudget,
    original: super::ceremony_tracker::HeldIssuerClockObservationCapability,
}
impl HeldIssuerCompletionObserver {
    pub(super) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.original.require_effects(effects)
    }
    #[cfg(test)]
    pub(super) async fn finalize_original_issuer(
        &self,
        effects: &AuraEffectSystem,
        retained: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
        service: &crate::handlers::device_epoch_rotation::DeviceEpochRotationService,
    ) -> crate::core::AgentResult<()> {
        self.original.require_effects(effects)?;
        retained.require_runtime_owner(effects)?;
        aura_core::time::timeout::execute_with_timeout_budget(effects, &self.active, || async {
            self.original.require_pending().await?;
            service
                .finalize_sole_device_enrollment(&retained.manifest().ceremony)
                .await
        })
        .await
        .map_err(|source| {
            map_enrollment_run_error("finalize original issuer", &self.active, source)
        })
    }
    #[cfg(test)]
    pub(super) async fn observe_issuer_terminal_receipt(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>,
        runner: &super::ceremony_runner::CeremonyRunner,
    ) -> crate::core::AgentResult<crate::handlers::invitation::IssuerEnrollmentTerminalReceipt>
    {
        self.original.require_effects(effects.as_ref())?;
        retained.require_runtime_owner(effects.as_ref())?;
        aura_core::time::timeout::execute_with_timeout_budget(
            effects.as_ref(),
            &self.active,
            || async {
                self.original.require_pending().await?;
                crate::handlers::invitation::issue_original_terminal_receipt(
                    effects.clone(),
                    retained,
                    runner,
                )
                .await
            },
        )
        .await
        .map_err(|source| {
            map_enrollment_run_error(
                "observe original issuer terminal signing",
                &self.active,
                source,
            )
        })
    }
    pub(crate) async fn receive_issuer_result(
        &self,
        effects: &AuraEffectSystem,
        receiver: tokio::sync::oneshot::Receiver<
            Result<
                aura_app::runtime_bridge::DeviceEnrollmentStart,
                aura_invitation::enrollment_setup::EnrollmentIssuanceError,
            >,
        >,
    ) -> Result<aura_app::runtime_bridge::DeviceEnrollmentStart, AuraError> {
        self.original.require_effects(effects)?;
        aura_core::time::timeout::execute_with_timeout_budget(effects, &self.active, || async {
            self.original.require_pending().await?;
            receiver
                .await
                .map_err(|source| AuraError::Internal {
                    message: "original prepared issuer result channel stopped".into(),
                    source: Some(Arc::new(source)),
                })?
                .map_err(|source| AuraError::Internal {
                    message: "original prepared issuer rejected completion".into(),
                    source: Some(Arc::new(source)),
                })
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
pub(crate) struct EnrollmentExecutionChild {
    active: TimeoutBudget,
    original: TimeoutBudget,
    checkpoint: Arc<WindowCheckpoint>,
    _lease: Arc<OwnedSemaphorePermit>,
}

/// One fresh acknowledged allocation owns all attenuated execution and its
/// dedicated child subtree. Serialized clock phases cannot construct this root.
pub(crate) struct EnrollmentExecutionRoot<Origin> {
    child: EnrollmentExecutionChild,
    children: crate::task_registry::TaskGroup,
    origin: Arc<Origin>,
}

impl EnrollmentExecutionRoot<RegisteredEnrollmentWindowCapability> {
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
    ) -> Result<super::ceremony_tracker::PreparedRunningIssuerObservation, AuraError> {
        self.origin.prepare_running_observation(
            self.running_observer(effects)?,
            self.children().clone(),
            generation,
        )
    }
    fn running_observer(
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
        self,
        effects: &AuraEffectSystem,
        terminal: crate::handlers::invitation::IssuerEnrollmentTerminalReceipt,
    ) -> Result<AcknowledgedIssuerEnrollmentWindow, AuraError> {
        self.child.require_execution_effects(effects)?;
        terminal.require_runtime_owner(effects)?;
        self.origin
            .require_issued_notice_control(terminal.retained())?;
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
                    crate::task_registry::TaskGroupExecutionOutcome::Completed => {}
                    crate::task_registry::TaskGroupExecutionOutcome::NotExecuted => {
                        return Err(DurableEnrollmentExecutionError::NoCompletedChild.refusal());
                    }
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
            || AcknowledgedIssuerEnrollmentWindow { terminal },
        )
        .await
        .map_err(AuraError::from)
    }

    /// Transfer the same allocation from issuance into its registered protocol.
    /// This changes only the permitted checkpoint accessor, never its provider,
    /// absolute endpoint, lease, subtree or terminal authority.
    pub(super) fn into_registered(mut self) -> Self {
        self.child.checkpoint = Arc::new(WindowCheckpoint::Registered {
            capability: Arc::clone(&self.origin),
        });
        self
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
                _lease: original.lease(),
                checkpoint: Arc::new(WindowCheckpoint::HeldIssuer {
                    capability: original.clone(),
                }),
            },
            children: original_tasks.group("original-issuer-enrollment-children"),
            origin: original,
        }
    }
}

/// Actual terminal storage acknowledgment retaining the original signed result.
/// A Closed record read on restart cannot recreate this process-local authority.
pub(crate) struct AcknowledgedIssuerEnrollmentWindow {
    terminal: crate::handlers::invitation::IssuerEnrollmentTerminalReceipt,
}

impl AcknowledgedIssuerEnrollmentWindow {
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
            _lease: Arc::clone(&self.child._lease),
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
        self,
        effects: &AuraEffectSystem,
        proof: &crate::handlers::invitation::VerifiedEnrollmentConfirmation,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_original_domain_invitation(proof.canonical_invitation())?;
        if proof.manifest_digest() != self.origin().manifest_digest() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        self.close_acknowledged(effects, true).await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_enrollment_terminal_root",
        receiver_type = EnrollmentExecutionRoot<AdmittedEnrollmentManifest>,
        family = "runtime_helper"
    )]
    pub(crate) async fn acknowledge_failure(
        self,
        effects: &AuraEffectSystem,
        proof: &crate::handlers::invitation::VerifiedEnrollmentFailureCapability,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.require_original_domain_invitation(proof.canonical_invitation())?;
        if proof.manifest_digest() != self.origin().manifest_digest() {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        self.close_acknowledged(effects, false).await
    }

    async fn close_acknowledged(
        self,
        effects: &AuraEffectSystem,
        positive: bool,
    ) -> Result<AcknowledgedEnrollmentWindow, AuraError> {
        self.child.require_execution_effects(effects)?;
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
                if let Some(source) = disposal.failure() {
                    return Err(AuraError::Internal {
                        message: "original enrollment child disposal failed".into(),
                        source: Some(Arc::new(source.clone())),
                    });
                }
                match disposal.execution_outcome() {
                    crate::task_registry::TaskGroupExecutionOutcome::Interrupted(source) => {
                        return Err(AuraError::Internal {
                            message: "original enrollment child execution was interrupted".into(),
                            source: Some(Arc::new(source.clone())),
                        });
                    }
                    crate::task_registry::TaskGroupExecutionOutcome::NotExecuted => {
                        if positive {
                            return Err(DurableEnrollmentExecutionError::NoCompletedChild.refusal());
                        }
                    }
                    crate::task_registry::TaskGroupExecutionOutcome::Completed => {}
                }
                let WindowCheckpoint::Admitted {
                    binding, writes, ..
                } = self.child.checkpoint.as_ref()
                else {
                    return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
                };
                let _write = writes.lock().await;
                self.child
                    .require_pending_checkpoint(effects)
                    .await
                    .map_err(AuraError::from)?;
                let retained = EnrollmentExecutionChild::read_bound_admitted_checkpoint(
                    effects,
                    binding,
                    &self.child.original,
                )
                .await
                .map_err(AuraError::from)?;
                if retained.execution != DurableEnrollmentExecutionState::Pending {
                    return Err(DurableEnrollmentExecutionError::AlreadyBorn.refusal());
                }
                let now = effects.physical_time().await.map_err(AuraError::from)?;
                self.child
                    .active
                    .remaining_at(&now)
                    .map_err(AuraError::from)?;
                let record = StoredAdmittedWindow {
                    binding: binding.clone(),
                    budget: self.child.original.clone(),
                    execution: DurableEnrollmentExecutionState::Closed,
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
            Ok(acknowledged) => Ok(acknowledged),
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
    /// Consume the actual fresh provider acknowledgment while attaching its
    /// original runtime and independently admitted physical invitation owner.
    pub(crate) fn into_root(
        self,
        effects: Arc<AuraEffectSystem>,
        admitted: AdmittedEnrollmentManifest,
        original_tasks: &crate::task_registry::TaskGroup,
    ) -> Result<EnrollmentExecutionRoot<AdmittedEnrollmentManifest>, AuraError> {
        if !std::ptr::eq(self.effects, effects.as_ref()) {
            return Err(DurableEnrollmentExecutionError::ForeignProvider.refusal());
        }
        if self.binding != EnrollmentExecutionChild::admitted_binding(&admitted) {
            return Err(DurableEnrollmentExecutionError::ForeignAdmission.refusal());
        }
        let lease = effects.acquire_admitted_enrollment_window_owner(&admitted)?;
        lease.require_effects(effects.as_ref())?;
        let lease = lease.into_permit();
        Ok(EnrollmentExecutionRoot {
            child: EnrollmentExecutionChild {
                active: self.budget.clone(),
                original: self.budget,
                checkpoint: Arc::new(WindowCheckpoint::Admitted {
                    effects,
                    binding: self.binding,
                    writes: Mutex::new(()),
                }),
                _lease: Arc::new(lease),
            },
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
    execution: DurableEnrollmentExecutionState,
}

/// Persisted observation of original execution custody, never an owner token.
/// Absence is a codec error; obsolete records cannot become fresh allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum DurableEnrollmentExecutionState {
    Allocated,
    Pending,
    Closed,
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
    #[error("original enrollment execution has an unacknowledged pending observation")]
    Pending,
    #[error("original enrollment execution custody was already closed")]
    Closed,
}

impl DurableEnrollmentExecutionError {
    fn refusal(self) -> AuraError {
        AuraError::PermissionDenied {
            message: "original enrollment execution custody refused".into(),
            source: Some(Arc::new(self)),
        }
    }
}

impl DurableEnrollmentExecutionState {
    pub(super) fn require_unstarted_recovery(self) -> Result<(), AuraError> {
        let source = match self {
            Self::Allocated => return Ok(()),
            Self::Pending => DurableEnrollmentExecutionError::Pending,
            Self::Closed => DurableEnrollmentExecutionError::Closed,
        };
        Err(AuraError::PermissionDenied {
            message: "original enrollment execution cannot be reconstructed".into(),
            source: Some(Arc::new(source)),
        })
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
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_held_issuer_window",
        receiver_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(crate) async fn held_issuer_completion_observer(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<HeldIssuerCompletionObserver, AuraError> {
        let WindowCheckpoint::HeldIssuer { capability } = self.checkpoint.as_ref() else {
            return Err(AuraError::permission_denied(
                "completion requires actual held issuer preparation window",
            ));
        };
        capability.require_effects(effects)?;
        self.remaining_ms(effects).await.map_err(AuraError::from)?;
        let original = capability.completion_observation();
        if !self.active.shares_observation_owner_with(original.budget()) {
            return Err(AuraError::permission_denied(
                "issuer completion original clock changed",
            ));
        }
        Ok(HeldIssuerCompletionObserver {
            active: self.active.clone(),
            original,
        })
    }
    fn require_execution_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
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
    /// New sibling consent creates its local allocation once. No peer clock is used.
    /// Any previous allocation (including restart or consumed approval) refuses
    /// a second owner; only children of the retained original may continue.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RuntimeApprovedEnrollmentSigningIntent",
        family = "runtime_helper"
    )]
    pub(crate) async fn approved_signing(
        effects: Arc<AuraEffectSystem>,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<Self, AuraError> {
        let (capability, budget, lease) =
            signing_checkpoint::ApprovedSigningCheckpoint::allocate(effects, approval).await?;
        Ok(Self {
            active: budget.clone(),
            original: budget,
            _lease: lease,
            checkpoint: Arc::new(WindowCheckpoint::ApprovedSigning {
                capability: Arc::new(capability),
            }),
        })
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
    pub(crate) async fn require_retained_admitted_window(
        effects: &AuraEffectSystem,
        witness: &AdmittedEnrollmentManifest,
    ) -> Result<(), AuraError> {
        let binding = Self::admitted_binding(witness);
        let anchor = Self::read_admitted_anchor(effects, &binding).await?;
        let checkpoint = Self::decode_admitted_record(
            effects,
            &admitted_location("admitted_enrollment_clock_checkpoint_v2", &binding),
        )
        .await?;
        if anchor.execution != DurableEnrollmentExecutionState::Pending
            || checkpoint.execution != DurableEnrollmentExecutionState::Closed
            || checkpoint.binding != binding
        {
            return Err(DurableEnrollmentExecutionError::MissingTerminalCheckpoint.refusal());
        }
        checkpoint
            .budget
            .validate_checkpoint_continuation_from(&anchor.budget)
            .map_err(AuraError::from)?;
        Ok(())
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
        match self.checkpoint.as_ref() {
            WindowCheckpoint::HeldIssuer { capability }
            | WindowCheckpoint::Registered { capability } => capability
                .require_pending()
                .await
                .map_err(TimeoutBudgetError::checkpoint_failure),
            WindowCheckpoint::ApprovedSigning { capability } => {
                capability.checkpoint(&self.original).await
            }
            // Admitted birth is already durably Pending. Only the consuming
            // root may write terminal closure; children grant no write ACK.
            WindowCheckpoint::Admitted { .. } => Ok(()),
        }
    }
    async fn observe(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<
        (
            PhysicalTime,
            aura_core::time::timeout::TimeoutObservationLease,
        ),
        TimeoutBudgetError,
    > {
        self.require_execution_effects(effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let lease = self.original.acquire_observation().await;
        let now = effects
            .physical_time()
            .await
            .map_err(TimeoutBudgetError::time_source_failure)?;
        Ok((now, lease))
    }

    async fn observe_pending_birth(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<PhysicalTime, TimeoutBudgetError> {
        self.require_execution_effects(effects)
            .map_err(TimeoutBudgetError::checkpoint_failure)?;
        let result = aura_core::time::timeout::execute_with_timeout_budget(
            effects,
            &self.active,
            || async {
                self.require_pending_checkpoint(effects).await?;
                let now = effects
                    .physical_time()
                    .await
                    .map_err(TimeoutBudgetError::time_source_failure)?;
                self.active.remaining_at(&now)?;
                Ok(now)
            },
        )
        .await;
        match result {
            Ok(now) => Ok(now),
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
        )
    }

    async fn require_pending_checkpoint(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), TimeoutBudgetError> {
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
        if self.has_pending_birth() {
            let now = self.observe_pending_birth(effects).await?;
            return u64::try_from(self.active.remaining_at(&now)?.as_millis())
                .map_err(|source| TimeoutBudgetError::invalid_policy(source.to_string()));
        }
        let (now, _observation) = self.observe(effects).await?;
        let remaining = self.active.remaining_at(&now);
        self.checkpoint().await?;
        u64::try_from(remaining?.as_millis())
            .map_err(|error| TimeoutBudgetError::invalid_policy(error.to_string()))
    }
    pub(crate) async fn child(
        &self,
        effects: &AuraEffectSystem,
        requested: Duration,
    ) -> Result<Self, TimeoutBudgetError> {
        if self.has_pending_birth() {
            let now = self.observe_pending_birth(effects).await?;
            return Ok(Self {
                active: self.active.child_budget(&now, requested)?,
                original: self.original.clone(),
                checkpoint: Arc::clone(&self.checkpoint),
                _lease: Arc::clone(&self._lease),
            });
        }
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
        capability_type = EnrollmentExecutionChild,
        family = "runtime_helper"
    )]
    pub(crate) async fn issued_notice_validity_child(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<EnrollmentExecutionChild, AuraError> {
        self.require_issued_notice_owner(issued)?;
        let now = self
            .observe_pending_birth(effects)
            .await
            .map_err(AuraError::from)?;
        self.active.remaining_at(&now).map_err(AuraError::from)?;
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

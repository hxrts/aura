//! Original issuer and notice required-task ownership and cleanup.

use crate::core::AgentError;
use crate::handlers::invitation::{
    enrollment_stage_error, RetainedNoticeResult, RetainedOriginalCleanupFailure,
};
use crate::handlers::invitation::{Invitation, InvitationHandler, InvitationType};
#[cfg(test)]
use crate::handlers::invitation_service::take_original_issuer_before_vm_gate_for_test;
use crate::handlers::invitation_service::{
    capture_original_issuer_terminal, settle_required_enrollment_initiator_failure,
    EnrollmentInitiatorTaskFailure,
};
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::issuer_terminal_destination::OriginalIssuerTerminalDestination;
use crate::runtime::AuraEffectSystem;
use std::sync::Arc;

/// Runtime-owned storage shared only by the original notice custodian and its
/// protocol borrow. Cloning the handle grants no disposal acknowledgment.
#[derive(Clone, Default)]
pub(crate) struct RetainedNoticeTaskState {
    inner: Arc<tokio::sync::Mutex<RetainedNoticeTaskData>>,
}
#[derive(Default)]
pub(crate) struct RetainedNoticeTaskData {
    pub(crate) vm: crate::handlers::invitation::RetainedOriginalNoticeVm,
    pub(crate) failure: Option<Arc<AgentError>>,
}
impl RetainedNoticeTaskState {
    pub(crate) async fn lock(&self) -> tokio::sync::MutexGuard<'_, RetainedNoticeTaskData> {
        self.inner.lock().await
    }
}

cfg_if::cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        pub(crate) type OriginalNoticeExecution = futures::future::LocalBoxFuture<'static, Result<(), aura_core::AuraError>>;
    } else {
        pub(crate) type OriginalNoticeExecution = futures::future::BoxFuture<'static, Result<(), aura_core::AuraError>>;
    }
}

/// The registry retains this exact child's VM when its borrowed execution is
/// cancelled. Cleanup runs under the original registered task binding.
pub(crate) struct OriginalNoticeTaskCustody {
    // The Send owner retains a factory for borrowed executor-local work, never
    // an in-flight non-Send browser future. Reentry uses the same retained VM
    // cursor, candidate, stop receiver and original budget.
    pub(crate) execution: Option<Box<dyn FnMut() -> OriginalNoticeExecution + Send>>,
    pub(crate) state: RetainedNoticeTaskState,
    pub(crate) candidate_observed: Arc<std::sync::OnceLock<()>>,
    pub(crate) completion: Option<futures::channel::oneshot::Sender<RetainedNoticeResult>>,
}

impl OriginalNoticeTaskCustody {
    async fn execute_original(&mut self) -> Result<(), aura_core::AuraError> {
        let result = match self.execution.as_mut() {
            Some(execution) => execution().await,
            None => Ok(()),
        };
        self.execution.take();
        result
    }

    async fn cleanup_original(
        &mut self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        let resume_postchecks = self.candidate_observed.get().is_some()
            && self.execution.is_some()
            && !self.state.lock().await.vm.is_eligible();
        if resume_postchecks {
            // Resume the captured original cursor and postchecks after registry
            // cancellation drops their borrow. No new VM or receive is admitted.
            if let Err(source) = self.execute_original().await {
                self.state
                    .lock()
                    .await
                    .failure
                    .get_or_insert_with(|| Arc::new(AgentError::from(source)));
            }
        }
        // An already eligible VM retains its original postcheck result and
        // proceeds directly to close, including after a failed close retry.
        // Drop the interrupted borrow before reacquiring the same retained VM.
        // Neither dropping the future nor a parent task can acknowledge close.
        self.execution.take();
        let mut retained = self.state.lock().await;
        if let Err(source) = retained.vm.close_for_required_cleanup().await {
            let close = Arc::new(enrollment_stage_error(
                "close original required terminal notice VM",
                source,
            ));
            let failure = match retained.failure.take() {
                Some(primary) => Arc::new(AgentError::Aura(aura_core::AuraError::Internal {
                    message: "original notice execution and required close failed".into(),
                    source: Some(Arc::new(RetainedOriginalCleanupFailure { primary, close })),
                })),
                None => close,
            };
            retained.failure = Some(Arc::clone(&failure));
            return Err(aura_core::AuraError::Internal {
                message: "original terminal notice cleanup refused".into(),
                source: Some(failure),
            });
        }
        let failure = retained.failure.as_ref().cloned();
        let observed = if retained.vm.is_eligible() {
            Ok(true)
        } else {
            match &failure {
                Some(source) => Err(Arc::clone(source)),
                None => Ok(false),
            }
        };
        drop(retained);
        if let Some(completion) = self.completion.take() {
            let _ = completion.send(observed);
        }
        Ok(match failure {
            Some(source) => {
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    aura_core::AuraError::Internal {
                        message: "original terminal notice retained its domain failure".into(),
                        source: Some(source),
                    },
                )
            }
            None => aura_core::effects::task::RequiredTaskSettlement::Resolved,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalNoticeTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}

#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalNoticeTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}

/// The original registered owner retains protocol observation and signed result
/// through every borrowed execution/cleanup future. No receipt lives only in a
/// disposable async frame after it is delivered by the protocol child.
#[derive(Default)]
pub(crate) struct OriginalIssuerTerminalState {
    pub(crate) terminal: Option<crate::handlers::invitation::IssuerEnrollmentTerminalReceipt>,
    pub(crate) cancelled:
        Option<Arc<crate::handlers::invitation::OriginalIssuerCancellationTerminal>>,
    pub(crate) execution_failure: Option<aura_core::AuraError>,
    pub(crate) observation_failure: Option<aura_core::AuraError>,
    pub(crate) protocol_failure: Option<aura_core::AuraError>,
    pub(crate) protocol_failure_combined: bool,
    pub(crate) progress_failure_captured: bool,
}
impl OriginalIssuerTerminalState {
    fn capture_published_terminal_and_failures(
        &mut self,
        destination: &OriginalIssuerTerminalDestination,
    ) -> Result<(), aura_core::AuraError> {
        let captured = capture_original_issuer_terminal(destination, self);
        let finalizer = destination.retained_finalizer_failure();
        if let Some(protocol) = self.protocol_failure.clone() {
            // Combine once when the protocol is first observed, or when a
            // finalizer failure arrives after a protocol-only observation.
            // Repeated observation retains the same concrete failure carrier.
            if !self.protocol_failure_combined {
                if let Some(primary) = self.execution_failure.take().or_else(|| finalizer.clone()) {
                    self.execution_failure = Some(aura_core::AuraError::Internal {
                        message: "original issuer execution and protocol failed".into(),
                        source: Some(Arc::new(EnrollmentInitiatorTaskFailure::new(
                            AgentError::from(primary),
                            Some(protocol),
                        ))),
                    });
                    self.protocol_failure_combined = true;
                }
            }
        }
        if let Some(finalizer) = finalizer {
            self.execution_failure.get_or_insert(finalizer);
        }
        match self.execution_failure.as_ref() {
            Some(source) => Err(source.clone()),
            None => captured,
        }
    }
}
pub(crate) struct PreparedOriginalIssuerHandoff {
    pub(crate) running:
        crate::runtime::services::ceremony_tracker::PreparedRunningIssuerObservation,
    pub(crate) publication_sender: tokio::sync::oneshot::Sender<()>,
    pub(crate) initialized_receiver:
        tokio::sync::oneshot::Receiver<Result<(), aura_core::AuraError>>,
    pub(crate) admission_observer:
        crate::runtime::services::enrollment_window::HeldIssuerCompletionObserver,
}

pub(crate) struct OriginalIssuerTaskCustody {
    pub(crate) issuer_resource_cleanup:
        Option<crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner>,
    pub(crate) conditional_signing: Option<
        Arc<
            tokio::sync::Mutex<
                crate::runtime::services::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
        >,
    >,
    pub(crate) registration_custody:
        Option<crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'static>>,
    pub(crate) held_negative:
        Option<crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner>,
    pub(crate) finalizer:
        Option<crate::handlers::device_epoch_rotation::DeviceEpochRotationService>,
    pub(crate) issued:
        Option<Arc<crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl>>,
    pub(crate) original: Option<
        crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
    >,
    pub(crate) negative:
        Option<crate::runtime::services::enrollment_window::OriginalIssuerNegativeCleanupOwner>,
    pub(crate) negative_acknowledged:
        Option<crate::runtime::services::enrollment_window::AcknowledgedIssuerNegativeTerminal>,
    pub(crate) handler: InvitationHandler,
    pub(crate) effects: Arc<AuraEffectSystem>,
    pub(crate) ceremony_runner: Option<CeremonyRunner>,
    pub(crate) invitation: Invitation,
    pub(crate) terminal_destination: Arc<OriginalIssuerTerminalDestination>,
    pub(crate) terminal_state: OriginalIssuerTerminalState,
    pub(crate) acknowledged:
        Option<crate::runtime::services::enrollment_window::AcknowledgedIssuerEnrollmentWindow>,
    pub(crate) admission: Option<tokio::sync::oneshot::Receiver<()>>,
    pub(crate) initialized: Option<tokio::sync::oneshot::Sender<Result<(), aura_core::AuraError>>>,
    pub(crate) entered: bool,
    pub(crate) acknowledgment_retained: bool,
}
impl OriginalIssuerTaskCustody {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OriginalIssuerTaskCustody",
        receiver_type = OriginalIssuerTaskCustody,
        family = "runtime_helper"
    )]
    pub(crate) fn original_enrollment_rotation_origin(
        &self,
    ) -> Result<
        crate::handlers::device_epoch_rotation::DeviceEpochRotationOrigin,
        aura_core::AuraError,
    > {
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            return Err(source.clone());
        }
        if !self.entered || self.initialized.is_some() || self.registration_custody.is_some() {
            return Err(aura_core::AuraError::permission_denied(
                "original enrollment rotation requires initialized issuer custody",
            ));
        }
        let root = self.original.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original issuer root is consumed")
        })?;
        let issued = self.issued.as_ref().ok_or_else(|| {
            aura_core::AuraError::permission_denied("original issuer control is missing")
        })?;
        issued.require_runtime_owner(self.effects.as_ref())?;
        root.child().require_issued_notice_owner(issued)?;
        issued.original_conditional_signing_required(self.effects.as_ref(), root.child())?;
        Ok(
            crate::handlers::device_epoch_rotation::DeviceEpochRotationOrigin::Enrollment(
                crate::handlers::device_epoch_rotation::OriginalEnrollmentRotationContext::from_original(
                    Arc::clone(issued),
                    root.owned_execution_child(),
                    root.children().clone(),
                ),
            ),
        )
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalIssuerTaskCustody", receiver_type = OriginalIssuerTaskCustody, family = "runtime_helper")]
    pub(crate) fn retain_conditional_signing(
        &mut self,
        original: Arc<
            tokio::sync::Mutex<
                crate::runtime::services::threshold_signing::OriginalIssuerTranscriptSigningCustody,
            >,
        >,
    ) -> Result<(), aura_core::AuraError> {
        if self.conditional_signing.is_some() || self.entered {
            return Err(aura_core::AuraError::permission_denied(
                "original conditional signing handoff already entered",
            ));
        }
        original
            .try_lock()
            .map_err(|source| aura_core::AuraError::Internal {
                message: "original conditional signing state is borrowed".into(),
                source: Some(Arc::new(source)),
            })?
            .require_initial_transfer()?;
        self.conditional_signing = Some(original);
        Ok(())
    }
    pub(crate) fn retain_registration_custody(
        &mut self,
        original: &mut Option<
            crate::runtime::effects::RegisteredEnrollmentGenerationCapability<'static>,
        >,
    ) -> Result<(), aura_core::AuraError> {
        if self.registration_custody.is_some() || original.is_none() {
            return Err(aura_core::AuraError::invalid(
                "original registration custody transfer refused",
            ));
        }
        self.registration_custody = original.take();
        Ok(())
    }

    async fn settle_original_negative(
        &mut self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        if self.negative_acknowledged.is_none() {
            if self.negative.is_none() {
                let terminal = self
                    .terminal_state
                    .cancelled
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("actual original cancellation is missing")
                    })?
                    .clone();
                let root = self.original.take().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer root is already consumed")
                })?;
                // Synchronous transfer: no await or fallible operation occurs
                // between removing the root and retaining its negative owner.
                self.negative = Some(crate::runtime::services::enrollment_window::OriginalIssuerNegativeCleanupOwner::from_original(
                    root, terminal, Arc::clone(&self.terminal_destination), self.conditional_signing.as_ref().map(Arc::clone),
                ));
            }
            self.negative
                .as_mut()
                .ok_or_else(|| aura_core::AuraError::invalid("negative original owner is missing"))?
                .settle(&self.effects)
                .await?;
            let owner = self
                .negative
                .take()
                .ok_or_else(|| aura_core::AuraError::invalid("negative original owner was lost"))?;
            match owner.into_acknowledged() {
                Ok(acknowledged) => self.negative_acknowledged = Some(acknowledged),
                Err(owner) => {
                    self.negative = Some(*owner);
                    return Err(aura_core::AuraError::invalid(
                        "negative original terminal is unresolved",
                    ));
                }
            }
        }
        let acknowledged = self.negative_acknowledged.as_ref().ok_or_else(|| {
            aura_core::AuraError::invalid("actual negative terminal acknowledgment is missing")
        })?;
        self.ceremony_runner = None;
        self.finalizer = None;
        let domain = acknowledged.domain_failure();
        let original = self
            .terminal_state
            .execution_failure
            .clone()
            .or_else(|| self.terminal_state.protocol_failure.clone())
            .or_else(|| self.terminal_state.observation_failure.clone());
        let failure = match original {
            Some(primary) => aura_core::AuraError::Internal {
                message: "original issuer execution and signed cancellation failed".into(),
                source: Some(Arc::new(EnrollmentInitiatorTaskFailure::new(
                    AgentError::from(primary),
                    Some(domain),
                ))),
            },
            None => domain,
        };
        Ok(
            aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                self.terminal_destination.with_retained_finalizer_failure(
                    self.terminal_destination.with_retained_vm_failures(failure),
                ),
            ),
        )
    }
    fn fail_initialization(&mut self, source: aura_core::AuraError) -> aura_core::AuraError {
        let original = self
            .terminal_state
            .execution_failure
            .get_or_insert(source)
            .clone();
        if let Some(initialized) = self.initialized.take() {
            let _ = initialized.send(Err(original.clone()));
        }
        original
    }
    async fn observe_original_terminal(&mut self) -> Result<(), aura_core::AuraError> {
        let captured = if self.acknowledged.is_none()
            && !self.acknowledgment_retained
            && self.terminal_destination.result.get().is_some()
        {
            Some(
                self.terminal_state
                    .capture_published_terminal_and_failures(&self.terminal_destination),
            )
        } else {
            None
        };
        if let Some(source) = self.terminal_destination.retained_finalizer_failure() {
            self.terminal_state.execution_failure.get_or_insert(source);
        }
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            return Err(source.clone());
        }
        if self.terminal_state.terminal.is_some()
            || self.terminal_state.cancelled.is_some()
            || self.acknowledged.is_some()
            || self.acknowledgment_retained
        {
            return Ok(());
        }
        if let Some(captured) = captured {
            return captured;
        }
        if let Some(source) = self.terminal_state.observation_failure.as_ref() {
            return Err(source.clone());
        }
        Err(aura_core::AuraError::invalid(
            "original issuer execution ended without a retained terminal result",
        ))
    }
    async fn retain_original_ack(&mut self) -> Result<(), aura_core::AuraError> {
        if self.acknowledgment_retained {
            return Ok(());
        }
        if self.acknowledged.is_none() {
            self.original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .acknowledge_issuer_terminal(
                    self.effects.as_ref(),
                    &self.terminal_destination,
                    &mut self.terminal_state.terminal,
                    &mut self.acknowledged,
                )
                .await?;
        }
        self.original
            .as_ref()
            .ok_or_else(|| {
                aura_core::AuraError::invalid("positive original issuer root is consumed")
            })?
            .retain_issuer_acknowledgment(&mut self.acknowledged)?;
        self.acknowledgment_retained = true;
        Ok(())
    }
    pub(crate) async fn initialize_original(&mut self) -> Result<(), aura_core::AuraError> {
        if !self.entered {
            let Some(admission) = self.admission.as_mut() else {
                return Err(self.fail_initialization(aura_core::AuraError::invalid(
                    "original issuer publication observation is missing",
                )));
            };
            let publication = self
                .original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .child()
                .execute(self.effects.as_ref(), || async {
                    admission
                        .await
                        .map_err(|source| aura_core::AuraError::Internal {
                            message: "observe original issuer entry publication".into(),
                            source: Some(Arc::new(source)),
                        })
                })
                .await
                .map_err(|source| match source {
                    aura_core::TimeoutRunError::Timeout(source) => {
                        aura_core::AuraError::from(source)
                    }
                    aura_core::TimeoutRunError::Operation(source) => source,
                });
            if let Err(source) = publication {
                return Err(self.fail_initialization(source));
            }
            self.admission = None;
            self.entered = true;
            if self.issued.is_none() {
                let original_child = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .child();
                let loaded = original_child
                    .execute(self.effects.as_ref(), || async {
                        crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl::load(
                            self.effects.clone(),
                            &self.invitation,
                        )
                        .await
                    })
                    .await
                    .map_err(|source| {
                        original_child.map_run_error("retain original finalizer control", source)
                    });
                match loaded {
                    Ok(issued) => self.issued = Some(Arc::new(issued)),
                    Err(source) => {
                        self.terminal_state
                            .execution_failure
                            .get_or_insert_with(|| aura_core::AuraError::Internal {
                                message: "retain original finalizer control failed".into(),
                                source: Some(Arc::new(source)),
                            });
                    }
                }
            }
            if let Some(issued) = self.issued.as_ref() {
                if let Err(source) = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .child()
                    .bind_registered_notice_control(issued)
                    .and_then(|_| {
                        self.original
                            .as_ref()
                            .ok_or_else(|| {
                                aura_core::AuraError::invalid(
                                    "positive original issuer root is consumed",
                                )
                            })?
                            .child()
                            .require_issued_notice_owner(issued)
                    })
                {
                    self.terminal_state.execution_failure.get_or_insert(source);
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let (Some(service), Some(issued)) =
                    (self.finalizer.as_ref(), self.issued.as_ref())
                {
                    match service
                        .requires_original_sole_finalization(
                            issued,
                            self.original
                                .as_ref()
                                .ok_or_else(|| {
                                    aura_core::AuraError::invalid(
                                        "positive original issuer root is consumed",
                                    )
                                })?
                                .child(),
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => {
                            self.finalizer = None;
                        }
                        Err(source) => {
                            self.terminal_state
                                .execution_failure
                                .get_or_insert_with(|| aura_core::AuraError::Internal {
                                    message: "select original finalizer role failed".into(),
                                    source: Some(Arc::new(source)),
                                });
                        }
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let Some(registration) = self.registration_custody.as_mut() {
                    if let (Some(signing), Some(issued), Some(root)) = (
                        self.conditional_signing.as_ref(),
                        self.issued.as_ref(),
                        self.original.as_ref(),
                    ) {
                        if let Err(source) = issued
                            .bind_original_conditional_signing(Arc::clone(signing), root.child())
                        {
                            self.terminal_state.execution_failure.get_or_insert(source);
                        }
                    }
                    let result = if let Some(source) =
                        self.terminal_state.execution_failure.as_ref()
                    {
                        Err(source.clone())
                    } else {
                        match (self.original.as_ref(), self.issued.as_ref()) {
                            (Some(root), Some(issued)) => registration
                                .release_original_registration(self.effects.as_ref(), root, issued),
                            _ => Err(aura_core::AuraError::invalid(
                                "actual original control is missing at registration transfer",
                            )),
                        }
                    };
                    if let Err(source) = result {
                        self.terminal_state.execution_failure.get_or_insert(source);
                    } else {
                        self.registration_custody = None;
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                if let Some(service) = self.finalizer.take() {
                    let original = self
                        .original
                        .as_ref()
                        .ok_or_else(|| {
                            aura_core::AuraError::invalid(
                                "positive original issuer root is consumed",
                            )
                        })?
                        .owned_execution_child();
                    let Some(issued) = self.issued.as_ref().cloned() else {
                        return Err(aura_core::AuraError::invalid(
                            "retained original finalizer control is missing",
                        ));
                    };
                    let runner = self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                        aura_core::AuraError::invalid("original issuer context already settled")
                    })?;
                    let effects = self.effects.clone();
                    let retained_destination = Arc::clone(&self.terminal_destination);
                    let finalize = async move {
                        #[cfg(test)]
                        eprintln!("original sole finalizer: entered required producer");
                        match service
                            .finalize_sole_device_enrollment(&issued, &original)
                            .await
                        {
                            Ok(()) => {
                                #[cfg(test)]
                                eprintln!("original sole finalizer: producer completed");
                                Ok(())
                            }
                            Err(source) => {
                                #[cfg(test)]
                                eprintln!("original sole finalizer: native failure {source:?}");
                                let primary = aura_core::AuraError::Internal {
                                    message: "original issuer finalization failed".into(),
                                    source: Some(Arc::new(source)),
                                };
                                retained_destination.retain_finalizer_failure(primary.clone());
                                let publication = original
                                    .execute(effects.as_ref(), || async {
                                        Ok::<_, AgentError>(
                                            settle_required_enrollment_initiator_failure(
                                                &runner,
                                                &issued.manifest().ceremony,
                                                AgentError::from(primary.clone()),
                                            )
                                            .await,
                                        )
                                    })
                                    .await;
                                match publication {
                                    Ok(source) => Err(source),
                                    Err(source) => Err(aura_core::AuraError::Internal {
                                        message:
                                            "original finalizer failure publication incomplete"
                                                .into(),
                                        source: Some(Arc::new(EnrollmentInitiatorTaskFailure::new(
                                            AgentError::from(primary),
                                            Some(
                                                match original.map_run_error(
                                                    "original finalizer failure publication",
                                                    source,
                                                ) {
                                                    AgentError::Aura(source) => source,
                                                    source => aura_core::AuraError::Internal {
                                                        message:
                                                            "original finalizer failure publication"
                                                                .into(),
                                                        source: Some(Arc::new(source)),
                                                    },
                                                },
                                            ),
                                        ))),
                                    }),
                                }
                            }
                        }
                    };
                    cfg_if::cfg_if! {
                        if #[cfg(target_arch = "wasm32")] {
                            let admitted = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_local_try_named_observed("original_issuer_finalizer", finalize);
                        } else {
                            let admitted = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_try_named_observed("original_issuer_finalizer", finalize);
                        }
                    }
                    if let Err(source) = admitted {
                        self.terminal_state
                            .execution_failure
                            .get_or_insert_with(|| aura_core::AuraError::Internal {
                                message: "original issuer finalizer admission failed".into(),
                                source: Some(Arc::new(source)),
                            });
                    }
                }
            }
            if self.terminal_state.execution_failure.is_none() {
                let issued = self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid(
                        "original issuer control missing at protocol admission",
                    )
                })?;
                self.effects.retain_original_mailbox_issuer(issued)?;
                self.terminal_destination.bind_original_control(
                    self.effects.as_ref(),
                    issued,
                    self.original
                        .as_ref()
                        .ok_or_else(|| {
                            aura_core::AuraError::invalid("original protocol root missing")
                        })?
                        .child(),
                )?;
            }
            if let Some(initialized) = self.initialized.take() {
                let actual_admission = self
                    .terminal_state
                    .execution_failure
                    .as_ref()
                    .map_or(Ok(()), |source| Err(source.clone()));
                if initialized.send(actual_admission).is_err() {
                    self.terminal_state
                        .observation_failure
                        .get_or_insert_with(|| {
                            aura_core::AuraError::invalid(
                                "original issuer initialization observer was lost",
                            )
                        });
                }
            }
        }
        Ok(())
    }
    pub(crate) async fn execute_original(&mut self) -> Result<(), aura_core::AuraError> {
        self.initialize_original().await?;
        if self.terminal_state.execution_failure.is_none() {
            #[cfg(test)]
            if let Some((entered, release)) =
                take_original_issuer_before_vm_gate_for_test(self.effects.runtime_authority_id())
            {
                let _ = entered.send(());
                #[cfg(target_arch = "wasm32")]
                let _ = release.await;
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(Some(clock)) = release.await {
                    clock
                        .fail_next_current_task_observation(
                            aura_core::effects::TimeError::OperationFailed {
                                reason: "original issuer progress read before cancellation".into(),
                            },
                        )
                        .await
                        .expect("only this original task owns the selected read fault");
                }
            }
            let root = self
                .original
                .as_ref()
                .ok_or_else(|| aura_core::AuraError::invalid("original issuer root is consumed"))?;
            let issued =
                Arc::clone(self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer control missing")
                })?);
            let runner =
                self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer runner missing")
                })?;
            // This future borrows the required owner's retained VM slot. Both
            // execution and cleanup retain the same registered task binding.
            let (result, negative_delivery_failure) = {
                let mut protocol =
                    Box::pin(self.handler.execute_device_enrollment_initiator_owned(
                        Arc::clone(&self.effects),
                        Arc::clone(&issued),
                        runner,
                        root.owned_execution_child(),
                        Arc::clone(&self.terminal_destination),
                    ));
                tokio::select! {
                    biased;
                    source = self.terminal_destination.await_finalizer_failure() => {
                        #[cfg(test)]
                        eprintln!("original issuer: observed original finalizer failure");
                        // An actual original rejection or cancellation still
                        // owes its signed terminal. Continue this same
                        // admitted protocol; the destination retains the native
                        // finalizer failure for the failed domain disposition.
                        let verified_rejection = matches!(super::enrollment_window::OriginalIssuerResourceDecision::from_original(
                            &issued, root.child(),
                        ), Ok(super::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected));
                        let verified_cancellation = root.child().original_negative_decision(&issued)
                            .is_ok_and(|receipt| receipt.kind() == aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Cancelled);
                        if verified_rejection || verified_cancellation {
                                #[cfg(test)]
                                eprintln!("original issuer: finish original verified negative delivery");
                                match protocol.as_mut().await {
                                    Ok(terminal) => (Ok(terminal), None),
                                    Err(delivery) => {
                                        let failure = aura_core::AuraError::Internal {
                                            message: "original verified negative delivery failed".into(),
                                            source: Some(Arc::new(EnrollmentInitiatorTaskFailure::new(
                                                AgentError::from(source),
                                                Some(aura_core::AuraError::Internal {
                                                    message: "original signed negative protocol failed".into(),
                                                    source: Some(Arc::new(delivery)),
                                                }),
                                            ))),
                                        };
                                        (Err(AgentError::from(failure.clone())), Some(failure))
                                    }
                                }
                        } else {
                            (Err(AgentError::from(source)), None)
                        }
                    }
                    result = protocol.as_mut() => (result, None),
                }
            };
            if let Some(source) = negative_delivery_failure {
                self.terminal_state.execution_failure.get_or_insert(source);
            }
            let completion = if let Ok(terminal) = &result {
                self.terminal_destination
                    .retain_original_protocol_completion(terminal)
            } else {
                Ok(())
            };
            // Retain the actual result even when completion provenance refuses.
            self.terminal_destination.publish(result)?;
            completion?;
        }
        let terminal = match self.terminal_state.execution_failure.as_ref() {
            Some(source) => Err(source.clone()),
            None => self.observe_original_terminal().await,
        };
        if self.terminal_state.cancelled.is_some() {
            return terminal;
        }
        if let Err(source) = terminal {
            if let Some(original) = self.terminal_destination.finalizer_failure.get() {
                self.terminal_state
                    .execution_failure
                    .get_or_insert_with(|| original.clone());
            }
            self.terminal_state
                .execution_failure
                .get_or_insert_with(|| source.clone());
            let ceremony = match &self.invitation.invitation_type {
                InvitationType::DeviceEnrollment { ceremony_id, .. } => ceremony_id,
                _ => return Err(source),
            };
            let execution = AgentError::from(source.clone());
            let published = self
                .original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .child()
                .execute(self.effects.as_ref(), || async {
                    Ok::<_, AgentError>(
                        settle_required_enrollment_initiator_failure(
                            self.ceremony_runner.as_ref().ok_or_else(|| {
                                aura_core::AuraError::invalid(
                                    "original issuer context already settled",
                                )
                            })?,
                            ceremony,
                            execution,
                        )
                        .await,
                    )
                })
                .await;
            #[cfg(test)]
            eprintln!("original issuer: failure publication completed");
            return Err(match published {
                Ok(failure) => failure,
                Err(publication_source) => {
                    let publication = self
                        .original
                        .as_ref()
                        .ok_or_else(|| {
                            aura_core::AuraError::invalid(
                                "positive original issuer root is consumed",
                            )
                        })?
                        .child()
                        .map_run_error("publish original issuer failure", publication_source);
                    let publication = match publication {
                        AgentError::Aura(source) => source,
                        source => aura_core::AuraError::Internal {
                            message: "original issuer terminal publication failed".into(),
                            source: Some(Arc::new(source)),
                        },
                    };
                    aura_core::AuraError::Internal {
                        message: "original issuer failure publication was incomplete".into(),
                        source: Some(Arc::new(EnrollmentInitiatorTaskFailure::new(
                            AgentError::from(source),
                            Some(publication),
                        ))),
                    }
                }
            });
        }
        self.retain_original_ack().await
    }
    pub(crate) async fn cleanup_original(
        &mut self,
        exit: &aura_core::effects::task::RequiredTaskExit,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError> {
        if self.registration_custody.is_some() || self.held_negative.is_some() {
            if self.held_negative.is_none() {
                let primary = self
                    .terminal_state
                    .execution_failure
                    .clone()
                    .unwrap_or_else(|| match exit {
                        aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                            source.clone()
                        }
                        _ => aura_core::AuraError::Internal {
                            message: "original registration handoff was interrupted".into(),
                            source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                                exit: exit.clone(),
                            })),
                        },
                    });
                if self.original.is_none() {
                    return Err(aura_core::AuraError::invalid("original held root missing"));
                }
                self.fail_initialization(primary.clone());
                let mut reservation = None;
                crate::runtime::effects::RegisteredEnrollmentGenerationCapability::take_original_registration(
                    &mut self.registration_custody, self.effects.as_ref(), &mut reservation,
                )?;
                let root = self
                    .original
                    .take()
                    .ok_or_else(|| aura_core::AuraError::invalid("original held root missing"))?;
                self.held_negative = Some(crate::runtime::services::enrollment_window::OriginalHeldIssuerNegativeCleanupOwner::from_original(
                    root, reservation.ok_or_else(|| aura_core::AuraError::invalid("original registration reservation missing"))?, primary,
                ));
            }
            let original = self.held_negative.as_mut().ok_or_else(|| {
                aura_core::AuraError::invalid("original held negative owner missing")
            })?;
            original.settle(&self.effects).await?;
            let settlement = original.settlement()?;
            self.ceremony_runner = None;
            return Ok(settlement);
        }
        // A signed rejected terminal still requires failed-domain resource
        // settlement. Its receipt cannot authorize the positive acknowledgment
        // path, whose descendants include the original failed finalizer.
        // Committed/interrupted cleanup retains its existing receipt boundary.
        if self.acknowledged.is_none()
            && !self.acknowledgment_retained
            && (self.terminal_destination.result.get().is_some()
                || self
                    .terminal_destination
                    .retained_original_progress_failure()
                    .is_some())
        {
            if let Err(source) = self
                .terminal_state
                .capture_published_terminal_and_failures(&self.terminal_destination)
            {
                self.terminal_state
                    .observation_failure
                    .get_or_insert(source);
            }
        }
        // Cancellation can drop the borrowed protocol before it has retained
        // a terminal. The protected original decision still owes its signed
        // notice; resource-only cleanup cannot replace that obligation.
        if self.negative.is_none() && self.negative_acknowledged.is_none() {
            if let Some((root, issued)) = self.original.as_ref().zip(self.issued.as_ref()) {
                let cancelled = root.child().original_negative_decision(issued)
                    .is_ok_and(|receipt| receipt.kind() == aura_invitation::enrollment_signing_intent::EnrollmentNegativeTerminalKind::Cancelled);
                if cancelled
                    && !self
                        .terminal_destination
                        .original_protocol_completed(issued)?
                {
                    let delivered = self
                        .terminal_destination
                        .cancelled
                        .get()
                        .map(|terminal| {
                            self.terminal_destination
                                .original_cancelled_send_completion(terminal)
                                .map(|completed| completed.map(|completed| (terminal, completed)))
                        })
                        .transpose()?
                        .flatten();
                    let terminal = if let Some((terminal, completed)) = delivered {
                        self.terminal_destination
                            .complete_original_cancelled_send(
                                self.effects.as_ref(),
                                root.child(),
                                terminal,
                                completed,
                            )
                            .await?
                    } else {
                        let runner = self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                            aura_core::AuraError::invalid("original cancellation runner missing")
                        })?;
                        let cancellation = runner.await_enrollment_cancellation(issued).await?;
                        let terminal = self
                            .handler
                            .resume_original_enrollment_cancellation(
                                Arc::clone(&self.effects),
                                Arc::clone(issued),
                                runner,
                                root.owned_execution_child(),
                                Arc::clone(&self.terminal_destination),
                                cancellation,
                            )
                            .await
                            .map_err(|source| match source {
                                AgentError::Aura(source) => source,
                                source => aura_core::AuraError::Internal {
                                    message: "original cancellation continuation failed".into(),
                                    source: Some(Arc::new(source)),
                                },
                            })?;
                        // This witness is retained only after actual signed
                        // delivery and same-slot VM close have completed.
                        self.terminal_destination
                            .retain_original_protocol_completion(&terminal)?;
                        terminal
                    };
                    match terminal {
                        crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Cancelled(
                            terminal,
                        ) => {
                            self.terminal_state.cancelled = Some(terminal);
                        }
                        crate::handlers::invitation::IssuerEnrollmentTerminalOutcome::Committed(
                            _,
                        ) => {
                            return Err(aura_core::AuraError::invalid(
                                "cancelled original resumed a positive terminal",
                            ));
                        }
                    }
                }
            }
        }
        if self.issuer_resource_cleanup.is_some()
            || (self.acknowledged.is_none()
                && !self.acknowledgment_retained
                && self
                    .original
                    .as_ref()
                    .zip(self.issued.as_ref())
                    .is_some_and(|(root, issued)| {
                        match crate::runtime::services::enrollment_window::OriginalIssuerResourceDecision::from_original(issued, root.child()) {
                            Ok(crate::runtime::services::enrollment_window::OriginalIssuerResourceDecision::VerifiedRejected) => true,
                            Ok(_) => self.terminal_state.terminal.is_none(),
                            Err(_) => false,
                        }
                    }))
        {
            if self.issuer_resource_cleanup.is_none() {
                let primary = self
                    .terminal_state
                    .execution_failure
                    .clone()
                    .or_else(|| self.terminal_state.protocol_failure.clone())
                    .or_else(|| self.terminal_destination.retained_finalizer_failure())
                    .unwrap_or_else(|| match exit {
                        aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                            source.clone()
                        }
                        _ => aura_core::AuraError::Internal {
                            message: "original committed issuer was interrupted".into(),
                            source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                                exit: exit.clone(),
                            })),
                        },
                    });
                let root = self.original.take().ok_or_else(|| {
                    aura_core::AuraError::invalid("committed original root missing")
                })?;
                let issued = Arc::clone(self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid("committed original control missing")
                })?);
                match crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner::from_original(root, issued, self.conditional_signing.clone(), primary, Arc::clone(&self.terminal_destination)) {
                    Ok(owner) => self.issuer_resource_cleanup = Some(owner),
                    Err(rejected) => { let (root, source) = *rejected; self.original = Some(root); return Err(source); }
                }
            }
            let settlement = self
                .issuer_resource_cleanup
                .as_mut()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("committed resource cleanup owner missing")
                })?
                .settle(&self.effects)
                .await?;
            self.ceremony_runner = None;
            self.finalizer = None;
            return Ok(settlement);
        }
        if let Some(cancelled) = self.terminal_destination.cancelled.get() {
            self.terminal_state
                .cancelled
                .get_or_insert_with(|| Arc::clone(cancelled));
        }
        if self.terminal_state.cancelled.is_some()
            || self.negative.is_some()
            || self.negative_acknowledged.is_some()
        {
            if self.terminal_destination.result.get().is_some() {
                if let Err(source) = capture_original_issuer_terminal(
                    &self.terminal_destination,
                    &mut self.terminal_state,
                ) {
                    self.terminal_state.execution_failure.get_or_insert(source);
                }
            }
            return self.settle_original_negative().await;
        }
        if matches!(
            exit,
            aura_core::effects::task::RequiredTaskExit::Cancelled
                | aura_core::effects::task::RequiredTaskExit::Panicked
        ) && self.terminal_state.terminal.is_none()
            && self.acknowledged.is_none()
            && !self.acknowledgment_retained
        {
            let primary = self
                .terminal_state
                .execution_failure
                .get_or_insert_with(|| aura_core::AuraError::Internal {
                    message: "original issuer execution interrupted before terminal settlement"
                        .into(),
                    source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                        exit: exit.clone(),
                    })),
                })
                .clone();
            let issued = Arc::clone(self.issued.as_ref().ok_or_else(|| primary.clone())?);
            let root = self.original.take().ok_or_else(|| {
                aura_core::AuraError::invalid("interrupted original root missing")
            })?;
            self.issuer_resource_cleanup = Some(
                crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner::from_interrupted_original(
                    root, issued, self.conditional_signing.clone(), primary, Arc::clone(&self.terminal_destination),
                )
            );
            let settlement = self
                .issuer_resource_cleanup
                .as_mut()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("original interrupted cleanup owner missing")
                })?
                .settle(&self.effects)
                .await?;
            self.ceremony_runner = None;
            self.finalizer = None;
            return Ok(settlement);
        }
        if self.initialized.is_some() {
            let source = match exit {
                aura_core::effects::task::RequiredTaskExit::Failed(source) => source.clone(),
                _ => aura_core::AuraError::Internal {
                    message: "original issuer initialization was interrupted".into(),
                    source: Some(Arc::new(OriginalIssuerInitializationIncomplete {
                        exit: exit.clone(),
                    })),
                },
            };
            self.fail_initialization(source);
        }
        if self.terminal_state.terminal.is_none()
            && self.acknowledged.is_none()
            && !self.acknowledgment_retained
            && self.terminal_destination.result.get().is_some()
        {
            capture_original_issuer_terminal(&self.terminal_destination, &mut self.terminal_state)?;
        }
        if self.terminal_state.terminal.is_none()
            && !self.acknowledgment_retained
            && self.acknowledged.is_none()
        {
            if let Some(source) = self.terminal_state.execution_failure.as_ref() {
                return Err(aura_core::AuraError::Internal {
                    message: "original issuer cleanup has no signed terminal authority".into(),
                    source: Some(Arc::new(source.clone())),
                });
            }
        }
        // Complete the same signed terminal if one exists. A missing terminal
        // leaves the original durable Pending refusal; resource supervision is
        // never substituted for a negative domain acknowledgment.
        if !self.acknowledgment_retained
            && self.terminal_state.terminal.is_none()
            && self.acknowledged.is_none()
        {
            self.original
                .as_ref()
                .ok_or_else(|| {
                    aura_core::AuraError::invalid("positive original issuer root is consumed")
                })?
                .children()
                .request_cancellation();
            self.observe_original_terminal().await?;
        }
        self.retain_original_ack().await?;
        self.ceremony_runner = None;
        self.finalizer = None;
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            return Ok(
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    source.clone(),
                ),
            );
        }
        Ok(match exit {
            aura_core::effects::task::RequiredTaskExit::Failed(source) => {
                aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                    source.clone(),
                )
            }
            _ => aura_core::effects::task::RequiredTaskSettlement::Resolved,
        })
    }
}
#[cfg(test)]
pub(crate) fn assert_original_single_signer_cancelled_shutdown_source(
    disposed: &aura_core::effects::task::RequiredTaskExit,
    shutdown: &crate::task_registry::TaskSupervisionError,
) {
    fn cause<'a, T: std::error::Error + 'static>(
        mut error: &'a (dyn std::error::Error + 'static),
    ) -> Option<&'a T> {
        loop {
            if let Some(actual) = error.downcast_ref::<T>() {
                return Some(actual);
            }
            error = error.source()?;
        }
    }
    let aura_core::effects::task::RequiredTaskExit::Failed(source) = disposed else {
        panic!("same cancelled issuer must retain failed domain disposition");
    };
    let terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(source)
        .expect("actual original terminal failure");
    assert!(matches!(
        terminal.execution(),
        aura_core::effects::task::RequiredTaskExit::Cancelled
    ));
    let original = cause::<OriginalIssuerInitializationIncomplete>(terminal.terminal())
        .expect("actual original cancellation initialization source");
    assert!(matches!(
        original.exit,
        aura_core::effects::task::RequiredTaskExit::Cancelled
    ));
    assert!(
        matches!(
            shutdown,
            crate::task_registry::TaskSupervisionError::TaskFailed { .. }
        ),
        "strict supervised issuer domain failure: {shutdown:?}"
    );
    let same_terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(shutdown)
        .is_some_and(|actual| std::ptr::eq(actual, terminal));
    let same_original = cause::<OriginalIssuerInitializationIncomplete>(shutdown)
        .is_some_and(|actual| std::ptr::eq(actual, original));
    assert!(
        same_terminal || same_original,
        "strict shutdown retains exact original terminal/native cause allocation"
    );
}
#[derive(Debug, thiserror::Error)]
#[error("original issuer initialization ended without acknowledgment: {exit:?}")]
struct OriginalIssuerInitializationIncomplete {
    exit: aura_core::effects::task::RequiredTaskExit,
}

#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for OriginalIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original(exit))
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for OriginalIssuerTaskCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, aura_core::AuraError>,
    > {
        Box::pin(self.cleanup_original(exit))
    }
}

#[cfg(test)]
mod terminal_failure_observation_tests {
    use super::*;
    use std::error::Error;

    #[derive(Debug, thiserror::Error)]
    #[error("native original finalizer refusal")]
    struct NativeFinalizerRefusal;
    #[derive(Debug, thiserror::Error)]
    #[error("native original protocol refusal")]
    struct NativeProtocolRefusal;

    fn cause<'a, T: Error + 'static>(mut source: &'a (dyn Error + 'static)) -> &'a T {
        loop {
            if let Some(actual) = source.downcast_ref::<T>() {
                return actual;
            }
            source = source
                .source()
                .expect("original concrete source remains reachable");
        }
    }

    #[test]
    fn original_finalizer_and_protocol_causes_survive_both_publication_orders() {
        for protocol_first in [false, true] {
            let destination = OriginalIssuerTerminalDestination::default();
            let mut state = OriginalIssuerTerminalState::default();
            let native_finalizer = Arc::new(NativeFinalizerRefusal);
            let native_protocol = Arc::new(NativeProtocolRefusal);
            let finalizer = aura_core::AuraError::Internal {
                message: "actual original finalizer failure".into(),
                source: Some(native_finalizer.clone()),
            };
            if !protocol_first {
                destination.retain_finalizer_failure(finalizer.clone());
                // The original observer may have retained the finalizer before
                // the protocol result becomes available.
                state.execution_failure = Some(finalizer.clone());
            }
            destination
                .publish(Err(AgentError::from(aura_core::AuraError::Internal {
                    message: "actual original protocol failure".into(),
                    source: Some(native_protocol.clone()),
                })))
                .expect("one original protocol producer");
            if protocol_first {
                let first = state
                    .capture_published_terminal_and_failures(&destination)
                    .expect_err("original protocol refusal is not success");
                assert!(std::ptr::eq(
                    cause::<NativeProtocolRefusal>(&first),
                    native_protocol.as_ref()
                ));
                destination.retain_finalizer_failure(finalizer);
            }
            let failure = state
                .capture_published_terminal_and_failures(&destination)
                .expect_err("both original failures remain failures");
            let retained = cause::<EnrollmentInitiatorTaskFailure>(&failure);
            assert!(std::ptr::eq(
                cause::<NativeFinalizerRefusal>(retained),
                native_finalizer.as_ref()
            ));
            let protocol = retained
                .terminal_publication_error()
                .expect("independent protocol failure retained");
            assert!(std::ptr::eq(
                cause::<NativeProtocolRefusal>(protocol),
                native_protocol.as_ref()
            ));
            assert!(state.terminal.is_none());
            assert!(state.cancelled.is_none());
            let repeated = state
                .capture_published_terminal_and_failures(&destination)
                .expect_err("observation cannot replace either native failure");
            assert!(std::ptr::eq(
                cause::<EnrollmentInitiatorTaskFailure>(&repeated),
                retained
            ));
            drop(destination);
            drop(state);
            assert!(std::ptr::eq(
                cause::<NativeFinalizerRefusal>(&repeated),
                native_finalizer.as_ref()
            ));
            assert!(std::ptr::eq(
                cause::<NativeProtocolRefusal>(
                    cause::<EnrollmentInitiatorTaskFailure>(&repeated)
                        .terminal_publication_error()
                        .unwrap()
                ),
                native_protocol.as_ref()
            ));
        }
        // A finalizer already reported before the synchronous progress deposit
        // stays first even if no observer had copied it into owner state yet.
        let destination = OriginalIssuerTerminalDestination::default();
        let mut state = OriginalIssuerTerminalState::default();
        let native_finalizer = Arc::new(NativeFinalizerRefusal);
        let native_protocol = Arc::new(NativeProtocolRefusal);
        destination.retain_finalizer_failure(aura_core::AuraError::Internal {
            message: "original finalizer precedes progress deposit".into(),
            source: Some(native_finalizer.clone()),
        });
        let progress = destination
            .retain_original_progress_failure(AgentError::from(aura_core::AuraError::Internal {
                message: "original progress before cancellation arbitration".into(),
                source: Some(native_protocol.clone()),
            }))
            .expect("one original progress error is deposited");
        destination
            .publish(Err(AgentError::from(progress)))
            .expect("original result publication");
        let failure = state
            .capture_published_terminal_and_failures(&destination)
            .expect_err("earlier original finalizer remains primary");
        assert!(std::ptr::eq(
            cause::<NativeFinalizerRefusal>(&failure),
            native_finalizer.as_ref()
        ));
        let combined = cause::<EnrollmentInitiatorTaskFailure>(&failure);
        assert!(std::ptr::eq(
            cause::<NativeProtocolRefusal>(combined.terminal_publication_error().unwrap()),
            native_protocol.as_ref(),
        ));
        let repeated = state
            .capture_published_terminal_and_failures(&destination)
            .expect_err("repeated observation does not replace original source carrier");
        assert!(std::ptr::eq(
            cause::<EnrollmentInitiatorTaskFailure>(&repeated),
            combined
        ));

        // A later delivery failure can reach owner state before the original
        // progress source is observed. Publication order cannot reverse the
        // producer's actual chronology or erase either concrete allocation.
        for finalizer_preceded_progress in [false, true] {
            let destination = OriginalIssuerTerminalDestination::default();
            let mut state = OriginalIssuerTerminalState::default();
            let native_earlier = Arc::new(NativeFinalizerRefusal);
            let native_progress = Arc::new(NativeProtocolRefusal);
            let native_later = Arc::new(NativeFinalizerRefusal);
            if finalizer_preceded_progress {
                destination.retain_finalizer_failure(aura_core::AuraError::Internal {
                    message: "finalizer actually preceded progress".into(),
                    source: Some(native_earlier.clone()),
                });
            }
            destination
                .retain_original_progress_failure(AgentError::from(
                    aura_core::AuraError::Internal {
                        message: "original progress before later delivery".into(),
                        source: Some(native_progress.clone()),
                    },
                ))
                .expect("original producer records chronology before close");
            let later = aura_core::AuraError::Internal {
                message: "later delivery reached the observer first".into(),
                source: Some(native_later.clone()),
            };
            if !finalizer_preceded_progress {
                destination.retain_finalizer_failure(later.clone());
            }
            state.execution_failure = Some(later);
            let failure = state
                .capture_published_terminal_and_failures(&destination)
                .expect_err("later observation cannot supersede earlier production");
            let retained = cause::<EnrollmentInitiatorTaskFailure>(&failure);
            assert!(std::ptr::eq(
                cause::<NativeFinalizerRefusal>(retained.terminal_publication_error().unwrap()),
                native_later.as_ref(),
            ));
            if finalizer_preceded_progress {
                assert!(std::ptr::eq(
                    cause::<NativeFinalizerRefusal>(&failure),
                    native_earlier.as_ref(),
                ));
                let original = cause::<EnrollmentInitiatorTaskFailure>(retained.source().unwrap());
                assert!(std::ptr::eq(
                    cause::<NativeProtocolRefusal>(original.terminal_publication_error().unwrap()),
                    native_progress.as_ref(),
                ));
            } else {
                assert!(std::ptr::eq(
                    cause::<NativeProtocolRefusal>(&failure),
                    native_progress.as_ref(),
                ));
            }
            let repeated = state
                .capture_published_terminal_and_failures(&destination)
                .expect_err("repeated observation retains the same failure carrier");
            assert!(std::ptr::eq(
                cause::<EnrollmentInitiatorTaskFailure>(&repeated),
                retained,
            ));
        }
    }

    #[test]
    fn late_original_finalizer_keeps_protocol_primary_after_observation() {
        let destination = OriginalIssuerTerminalDestination::default();
        let mut state = OriginalIssuerTerminalState::default();
        let native_protocol = Arc::new(NativeProtocolRefusal);
        let progress = destination
            .retain_original_progress_failure(AgentError::from(aura_core::AuraError::Internal {
                message: "actual protocol failure before finalizer completion".into(),
                source: Some(native_protocol.clone()),
            }))
            .expect("actual original progress survives its borrowed future");
        let unpublished = state
            .capture_published_terminal_and_failures(&destination)
            .expect_err("original cause survives before terminal-result publication");
        assert!(std::ptr::eq(
            cause::<NativeProtocolRefusal>(&unpublished),
            native_protocol.as_ref()
        ));
        destination
            .publish(Err(AgentError::from(progress)))
            .expect("one original producer");
        let primary = state
            .capture_published_terminal_and_failures(&destination)
            .expect_err("first original protocol failure");
        let native_finalizer = Arc::new(NativeFinalizerRefusal);
        destination.retain_finalizer_failure(aura_core::AuraError::Internal {
            message: "actual late original finalizer failure".into(),
            source: Some(native_finalizer.clone()),
        });
        let retained = destination.with_retained_finalizer_failure(primary);
        drop(destination);
        drop(state);
        assert!(std::ptr::eq(
            cause::<NativeProtocolRefusal>(&retained),
            native_protocol.as_ref()
        ));
        let finalizer = cause::<EnrollmentInitiatorTaskFailure>(&retained)
            .terminal_publication_error()
            .expect("late finalizer remains independently accessible");
        assert!(std::ptr::eq(
            cause::<NativeFinalizerRefusal>(finalizer),
            native_finalizer.as_ref()
        ));
        // A distinct later protocol result must not mask the deposited cause.
        let destination = OriginalIssuerTerminalDestination::default();
        let first = Arc::new(NativeProtocolRefusal);
        let returned = Arc::new(NativeProtocolRefusal);
        destination
            .retain_original_progress_failure(AgentError::from(aura_core::AuraError::Internal {
                message: "original progress before later protocol failure".into(),
                source: Some(first.clone()),
            }))
            .expect("original progress is retained");
        destination
            .publish(Err(AgentError::from(aura_core::AuraError::Internal {
                message: "independent later original protocol failure".into(),
                source: Some(returned.clone()),
            })))
            .expect("later result remains retained independently");
        let mut state = OriginalIssuerTerminalState::default();
        let retained = state
            .capture_published_terminal_and_failures(&destination)
            .expect_err("both original sources remain failed");
        assert!(std::ptr::eq(
            cause::<NativeProtocolRefusal>(&retained),
            first.as_ref()
        ));
        let later = cause::<EnrollmentInitiatorTaskFailure>(&retained)
            .terminal_publication_error()
            .expect("later protocol source remains owned");
        assert!(std::ptr::eq(
            cause::<NativeProtocolRefusal>(later),
            returned.as_ref()
        ));
    }
}

//! Original issuer required-task ownership and cleanup.

use crate::core::AgentError;
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
    pub(crate) terminal_receiver: Option<tokio::sync::oneshot::Receiver<()>>,
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
        Ok(
            aura_core::effects::task::RequiredTaskSettlement::ResolvedWithDomainFailure(
                self.terminal_state
                    .execution_failure
                    .clone()
                    .or_else(|| self.terminal_state.protocol_failure.clone())
                    .or_else(|| self.terminal_state.observation_failure.clone())
                    .unwrap_or_else(|| acknowledged.domain_failure()),
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
        if let Some(source) = self.terminal_destination.retained_finalizer_failure() {
            self.terminal_state.execution_failure.get_or_insert(source);
        }
        if let Some(source) = self.terminal_state.execution_failure.as_ref() {
            self.terminal_receiver = None;
            return Err(source.clone());
        }
        if self.terminal_state.terminal.is_some()
            || self.terminal_state.cancelled.is_some()
            || self.acknowledged.is_some()
            || self.acknowledgment_retained
        {
            return Ok(());
        }
        if self.terminal_destination.result.get().is_some() {
            return capture_original_issuer_terminal(
                &self.terminal_destination,
                &mut self.terminal_state,
            );
        }
        if let Some(source) = self.terminal_state.observation_failure.as_ref() {
            return Err(source.clone());
        }
        let receiver = self.terminal_receiver.as_mut().ok_or_else(|| {
            aura_core::AuraError::invalid("original issuer protocol has not been admitted")
        })?;
        let destination = &self.terminal_destination;
        let terminal_state = &mut self.terminal_state;
        let observed = self
            .original
            .as_ref()
            .ok_or_else(|| {
                aura_core::AuraError::invalid("positive original issuer root is consumed")
            })?
            .child()
            .execute(self.effects.as_ref(), || async {
                let readiness = tokio::select! {
                    biased;
                    source = destination.await_finalizer_failure() => {
                        terminal_state.execution_failure.get_or_insert_with(|| source.clone());
                        return Err(AgentError::from(source));
                    }
                    result = receiver => result,
                };
                if let Some(source) = destination.retained_finalizer_failure() {
                    terminal_state
                        .execution_failure
                        .get_or_insert_with(|| source.clone());
                    return Err(AgentError::from(source));
                }
                if let Err(source) = readiness {
                    let source = aura_core::AuraError::Internal {
                        message: "original issuer terminal readiness observation failed".into(),
                        source: Some(Arc::new(source)),
                    };
                    terminal_state
                        .observation_failure
                        .get_or_insert_with(|| source.clone());
                    return Err(AgentError::from(source));
                }
                // Unit readiness conveys no receipt authority. Move only the exact
                // result already retained by this custodian's original destination,
                // and store it BEFORE Ready can lose the outer bounded race.
                capture_original_issuer_terminal(destination, terminal_state)
                    .map_err(AgentError::from)
            })
            .await;
        if self.terminal_state.terminal.is_some()
            || self.terminal_state.execution_failure.is_some()
            || self.terminal_state.observation_failure.is_some()
            || self.terminal_state.protocol_failure.is_some()
        {
            self.terminal_receiver = None;
        }
        observed.map_err(|source| match source {
            aura_core::TimeoutRunError::Timeout(source) => aura_core::AuraError::from(source),
            aura_core::TimeoutRunError::Operation(source) => aura_core::AuraError::Internal {
                message: "original issuer terminal result".into(),
                source: Some(Arc::new(source)),
            },
        })
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
                let child = self
                    .original
                    .as_ref()
                    .ok_or_else(|| {
                        aura_core::AuraError::invalid("positive original issuer root is consumed")
                    })?
                    .owned_execution_child();
                let (sender, receiver) = tokio::sync::oneshot::channel();
                self.terminal_receiver = Some(receiver);
                let handler = self.handler.clone();
                let effects = self.effects.clone();
                let runner = self.ceremony_runner.as_ref().cloned().ok_or_else(|| {
                    aura_core::AuraError::invalid("original issuer context already settled")
                })?;
                let issued = Arc::clone(self.issued.as_ref().ok_or_else(|| {
                    aura_core::AuraError::invalid(
                        "original issuer control is missing at child admission",
                    )
                })?);
                self.effects.retain_original_mailbox_issuer(&issued)?;
                let destination = Arc::clone(&self.terminal_destination);
                let execution = async move {
                    #[cfg(test)]
                    if let Some((entered, release)) =
                        take_original_issuer_before_vm_gate_for_test(effects.runtime_authority_id())
                    {
                        let _ = entered.send(());
                        let _ = release.await;
                    }
                    let result = handler
                        .execute_device_enrollment_initiator_owned(
                            effects,
                            issued,
                            runner,
                            child,
                            Arc::clone(&destination),
                        )
                        .await;
                    let (result, failure) = match result {
                        Ok(terminal) => (Ok(terminal), None),
                        Err(source) => {
                            let failure = aura_core::AuraError::Internal {
                                message: "original issuer enrollment child failed".into(),
                                source: Some(Arc::new(source)),
                            };
                            (Err(AgentError::from(failure.clone())), Some(failure))
                        }
                    };
                    // The original destination retains the move-owned result before
                    // signalling. A lost observer cannot destroy signed custody.
                    destination.publish(result)?;
                    sender
                        .send(())
                        .map_err(|_| aura_core::AuraError::PermissionDenied {
                            message: "original issuer terminal observer was lost".into(),
                            source: Some(Arc::new(
                                crate::handlers::invitation::EnrollmentVmAdmissionError::TerminalObserverLost,
                            )),
                        })?;
                    match failure {
                        Some(source) => Err(source),
                        None => Ok(()),
                    }
                };
                cfg_if::cfg_if! {
                    if #[cfg(target_arch = "wasm32")] {
                        let child_admission = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_local_try_named_observed("original_issuer_protocol", execution);
                    } else {
                        let child_admission = self.original.as_ref().ok_or_else(|| aura_core::AuraError::invalid("positive original issuer root is consumed"))?.children().spawn_try_named_observed("original_issuer_protocol", execution);
                    }
                }
                if let Err(source) = child_admission {
                    self.terminal_state
                        .execution_failure
                        .get_or_insert_with(|| aura_core::AuraError::Internal {
                            message: "original issuer protocol admission failed".into(),
                            source: Some(Arc::new(source)),
                        });
                }
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
        // A protected Committed decision cannot be rewritten as cancellation.
        // Interrupted producer cleanup closes only its original resources.
        if self.issuer_resource_cleanup.is_some()
            || (self.terminal_state.terminal.is_none()
                && self.acknowledged.is_none()
                && !self.acknowledgment_retained
                && self
                    .original
                    .as_ref()
                    .zip(self.issued.as_ref())
                    .is_some_and(|(root, issued)| {
                        crate::runtime::services::enrollment_window::OriginalIssuerResourceDecision::from_original(issued, root.child()).is_ok()
                    }))
        {
            if self.issuer_resource_cleanup.is_none() {
                let primary = self
                    .terminal_state
                    .execution_failure
                    .clone()
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
                match crate::runtime::services::enrollment_window::OriginalIssuerResourceCleanupOwner::from_original(root, issued, self.conditional_signing.clone(), primary) {
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
        if self.terminal_receiver.is_none()
            && self.terminal_state.terminal.is_none()
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

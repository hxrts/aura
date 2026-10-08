use super::enrollment_vm_admission::{self, EnrollmentControlFrame};
use super::vm_loop::{handle_invitation_vm_step, handle_invitation_vm_wait_status};
#[cfg(test)]
use super::vm_loop::{invitation_invalid_error, map_invitation_vm_timeout};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use crate::runtime::services::enrollment_window::EnrollmentWindowCapability;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnrollmentInviteeChoice {
    Accept,
    Refuse,
}

/// Primary execution remains the standard cause if teardown also fails.
#[derive(Debug)]
pub(crate) struct EnrollmentVmTeardownFailure {
    execution: AgentError,
    close: Box<dyn std::error::Error + Send + Sync>,
}
impl EnrollmentVmTeardownFailure {
    pub(crate) fn close_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.close.as_ref()
    }
}
impl std::fmt::Display for EnrollmentVmTeardownFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; enrollment VM close failed: {}",
            self.execution,
            self.close_error()
        )
    }
}
impl std::error::Error for EnrollmentVmTeardownFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.execution)
    }
}
fn finish_enrollment_vm_attempt<T, E: std::error::Error + Send + Sync + 'static>(
    result: AgentResult<T>,
    close: Result<(), E>,
) -> AgentResult<T> {
    match (result, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(close)) => Err(AgentError::Aura(aura_core::AuraError::Internal {
            message: "close enrollment VM".into(),
            source: Some(Arc::new(close)),
        })),
        (Err(execution), Err(close)) => {
            let failure = EnrollmentVmTeardownFailure {
                execution,
                close: Box::new(close),
            };
            Err(AgentError::Aura(aura_core::AuraError::Internal {
                message: failure.to_string(),
                source: Some(Arc::new(failure)),
            }))
        }
    }
}

fn enrollment_attempt_failed_to_close(error: &AgentError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if cause.is::<EnrollmentVmTeardownFailure>() {
            return true;
        }
        if cause
            .downcast_ref::<crate::runtime::session_ingress::SessionIngressError>()
            .is_some_and(|cause| {
                matches!(
                    cause,
                    crate::runtime::session_ingress::SessionIngressError::SessionClose { .. }
                )
            })
        {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Whether the original enrollment ended in a durable cancellation.
async fn enrollment_cancelled(
    runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
    ceremony: &aura_core::CeremonyId,
) -> bool {
    matches!(
        runner.terminal_outcome(ceremony).await,
        Ok(Some(
            aura_app::runtime_bridge::CeremonyTerminalOutcome::Failed(
                aura_app::runtime_bridge::CeremonyFailureReason::Cancelled
            )
        ))
    )
}

/// The session slot belongs to the caller outside the timed attempt future.
/// Selecting timeout may drop the borrowed operation, but cannot drop this
/// handle before required asynchronous teardown has been attempted.
pub(super) async fn finish_enrollment_vm_slot<T>(
    result: AgentResult<T>,
    slot: Option<crate::runtime::session_ingress::OwnedVmSession>,
) -> AgentResult<T> {
    match slot {
        Some(session) => finish_enrollment_vm_attempt(result, session.close().await),
        None => result,
    }
}

/// An invitee may not have imported its code when the issuer starts sending.
///
/// Only the initial request's actual unreachable-destination cause is retryable;
/// routing, engine, receive, confirmation, and codec failures remain terminal.
fn enrollment_request_peer_unreachable(error: &AgentError, subject: AuthorityId) -> bool {
    enrollment_send_peer_unreachable(error, subject, "DeviceEnrollmentRequest")
}
pub(super) fn enrollment_notice_peer_unreachable(error: &AgentError, subject: AuthorityId) -> bool {
    enrollment_send_peer_unreachable(error, subject, "DeviceEnrollmentTerminalNotice")
}

fn enrollment_send_peer_unreachable(
    error: &AgentError,
    subject: AuthorityId,
    expected_label: &str,
) -> bool {
    use crate::runtime::session_ingress::SessionIngressError;
    use crate::runtime::vm_host_bridge::AuraVmBridgeRoundError;
    use aura_core::effects::TransportError;
    use aura_protocol::effects::ChoreographyError;
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if cause.is::<EnrollmentVmTeardownFailure>() {
            return false;
        }
        if let Some(SessionIngressError::BridgeRound {
            source:
                AuraVmBridgeRoundError::Send {
                    from_role,
                    to_role,
                    label,
                    source,
                },
            ..
        }) = cause.downcast_ref::<SessionIngressError>()
        {
            let ChoreographyError::Transport { source } = source.as_ref() else {
                return false;
            };
            return from_role == "Initiator"
                && to_role == "Invitee"
                && label == expected_label
                && source
                    .downcast_ref::<TransportError>()
                    .is_some_and(|transport| {
                        matches!(transport, TransportError::DestinationUnreachable { destination }
                        if *destination == subject)
                    });
        }
        source = cause.source();
    }
    false
}

/// Pause between invitee attempts while waiting for the initiator.
const DEVICE_ENROLLMENT_INVITEE_RETRY_DELAY_MS: u64 = 500;

/// Pause between initiator attempts while waiting for the new device.
const DEVICE_ENROLLMENT_RETRY_DELAY_MS: u64 = 5_000;

fn enrollment_stage_error(
    stage: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: stage.into(),
        source: Some(Arc::new(source)),
    })
}

#[cfg(test)]
async fn enrollment_time<E: PhysicalTimeEffects + ?Sized>(
    effects: &E,
) -> AgentResult<PhysicalTime> {
    effects
        .physical_time()
        .await
        .map_err(|source| enrollment_stage_error("read enrollment owner clock", source))
}

#[cfg(test)]
async fn enrollment_remaining_ms<E: PhysicalTimeEffects + ?Sized>(
    effects: &E,
    budget: &TimeoutBudget,
) -> AgentResult<u64> {
    let now = enrollment_time(effects).await?;
    let remaining = budget.remaining_at(&now).map_err(|source| {
        map_invitation_vm_timeout(
            "device enrollment window",
            budget,
            TimeoutRunError::Timeout(source),
        )
    })?;
    u64::try_from(remaining.as_millis())
        .map_err(|source| enrollment_stage_error("convert enrollment remaining budget", source))
}

#[cfg(test)]
async fn enrollment_attempt_budget<E: PhysicalTimeEffects + ?Sized>(
    effects: &E,
    window: &TimeoutBudget,
) -> AgentResult<TimeoutBudget> {
    let now = enrollment_time(effects).await?;
    window
        .child_budget(
            &now,
            std::time::Duration::from_millis(INVITATION_VM_LOOP_TIMEOUT_MS),
        )
        .map_err(|source| {
            map_invitation_vm_timeout(
                "device enrollment window",
                window,
                TimeoutRunError::Timeout(source),
            )
        })
}

#[cfg(test)]
async fn enrollment_retry_delay<E: PhysicalTimeEffects + ?Sized>(
    effects: &E,
    budget: &TimeoutBudget,
    delay_ms: u64,
) -> AgentResult<()> {
    let remaining_ms = enrollment_remaining_ms(effects, budget).await?;
    effects
        .sleep_ms(delay_ms.min(remaining_ms))
        .await
        .map_err(|source| enrollment_stage_error("wait for enrollment retry", source))
}

pub(super) struct InvitationDeviceEnrollmentHandler;

impl InvitationDeviceEnrollmentHandler {
    pub(super) fn new(_handler: &InvitationHandler) -> Self {
        Self
    }

    /// Run the initiator until the new device accepts or the acceptance window
    /// closes. The new device only becomes reachable as a device of this
    /// authority once a person imports the code on it, so each attempt re-opens
    /// the session and re-sends the request.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentWindowCapability",
        family = "runtime_helper"
    )]
    pub(super) async fn execute_device_enrollment_initiator_owned(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> AgentResult<()> {
        let retained =
            super::enrollment_trust::RetainedEnrollmentVmControl::load(effects.clone(), invitation)
                .await?;
        budget
            .bind_registered_notice_control(&retained)
            .map_err(AgentError::from)?;
        let mut slot = None;
        let outcome = {
            let progress = Box::pin(self.run_device_enrollment_initiator_window(
                effects.clone(),
                &retained,
                &ceremony_runner,
                &budget,
                &mut slot,
            ));
            let cancellation = ceremony_runner.await_enrollment_cancellation(&retained);
            futures::pin_mut!(progress, cancellation);
            match futures::future::select(progress, cancellation).await {
                futures::future::Either::Left((Ok(_), _)) => Ok(None),
                // A response that arrives after a committed cancellation is
                // refused by the attempt; the cancellation still owns the
                // terminal path, so the signed notice must be sent.
                futures::future::Either::Left((Err(error), cancellation)) => {
                    let cancelled =
                        enrollment_cancelled(&ceremony_runner, &retained.manifest().ceremony).await;
                    if cancelled {
                        cancellation.await.map(Some).map_err(AgentError::from)
                    } else {
                        Err(error)
                    }
                }
                futures::future::Either::Right((result, _)) => {
                    result.map(Some).map_err(AgentError::from)
                }
            }
        };
        let outcome = match finish_enrollment_vm_slot(outcome, slot.take()).await {
            Ok(outcome) => outcome,
            // A cancel waiting on this owner's settlement must observe the
            // failure rather than wait for a settlement that never comes.
            Err(error) => {
                if enrollment_cancelled(&ceremony_runner, &retained.manifest().ceremony).await {
                    return super::enrollment_terminal_notice::publish_cancelled_settlement(
                        &ceremony_runner,
                        &retained.manifest().ceremony,
                        Err(error),
                    )
                    .await;
                }
                return Err(error);
            }
        };
        let Some(cancelled) = outcome else {
            return Ok(());
        };
        // Sign while the cancelled generation is held, then release it so a
        // new enrollment is not refused while an unanswered notice retries.
        // The cancel call awaits this settlement before it returns.
        let settled = async {
            let notice = super::enrollment_terminal_notice::sign_cancelled_notice(
                &effects,
                &retained,
                &ceremony_runner,
                &cancelled,
                &budget,
            )
            .await?;
            ceremony_runner
                .retire_failed_enrollment_generation(&retained.manifest().ceremony)
                .await
                .map_err(AgentError::from)?;
            Ok::<_, AgentError>(notice)
        }
        .await;
        let notice = super::enrollment_terminal_notice::publish_cancelled_settlement(
            &ceremony_runner,
            &retained.manifest().ceremony,
            settled,
        )
        .await?;
        loop {
            let attempt = budget
                .execute(effects.as_ref(), || {
                    Box::pin(super::enrollment_terminal_notice::send_cancelled_notice(
                        effects.clone(),
                        &retained,
                        &notice,
                        &budget,
                        &mut slot,
                    ))
                })
                .await
                .map_err(|source| {
                    budget.map_run_error("signed enrollment cancellation notice", source)
                });
            match finish_enrollment_vm_slot(attempt, slot.take()).await {
                Ok(()) => return Ok(()),
                Err(error)
                    if enrollment_notice_peer_unreachable(&error, retained.manifest().subject) =>
                {
                    budget
                        .retry_delay(effects.as_ref(), DEVICE_ENROLLMENT_RETRY_DELAY_MS)
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "enrollment cancellation notice retry",
                                TimeoutRunError::Timeout(source),
                            )
                        })?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_device_enrollment_initiator_window(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: &super::enrollment_trust::RetainedEnrollmentVmControl,
        ceremony_runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: &EnrollmentWindowCapability,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
    ) -> AgentResult<()> {
        loop {
            let attempt_budget = budget
                .child(
                    effects.as_ref(),
                    std::time::Duration::from_millis(INVITATION_VM_LOOP_TIMEOUT_MS),
                )
                .await
                .map_err(|source| {
                    budget
                        .map_run_error("device enrollment window", TimeoutRunError::Timeout(source))
                })?;

            let attempt = attempt_budget
                .execute(effects.as_ref(), || {
                    Box::pin(self.run_device_enrollment_initiator_attempt(
                        effects.clone(),
                        retained,
                        ceremony_runner.clone(),
                        &attempt_budget,
                        session_slot,
                    ))
                })
                .await
                .map_err(|source| {
                    attempt_budget.map_run_error("device enrollment initiator attempt", source)
                });
            let attempt = finish_enrollment_vm_slot(attempt, session_slot.take()).await;
            match attempt {
                Ok(()) => return Ok(()),
                Err(error)
                    if !enrollment_attempt_failed_to_close(&error)
                        && (error.is_timeout()
                            || enrollment_request_peer_unreachable(
                                &error,
                                retained.manifest().subject,
                            )) =>
                {
                    tracing::debug!(
                        invitation_id = %retained.canonical_invitation().invitation_id,
                        error = %error,
                        "device enrollment initiator attempt ended; retrying until the new device accepts"
                    );
                    budget
                        .retry_delay(effects.as_ref(), DEVICE_ENROLLMENT_RETRY_DELAY_MS)
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "device enrollment retry",
                                TimeoutRunError::Timeout(source),
                            )
                        })?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_device_enrollment_initiator_attempt(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: &super::enrollment_trust::RetainedEnrollmentVmControl,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: &EnrollmentWindowCapability,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
    ) -> AgentResult<()> {
        let invitation = retained.canonical_invitation();
        let response_verifier =
            super::enrollment_trust::PinnedEnrollmentResponseVerifierCapability::acquire(
                effects.as_ref(),
                retained,
            )
            .await?;
        let subject_authority = retained.manifest().subject;
        let request = enrollment_vm_admission::sign_request(effects.as_ref(), retained).await?;
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let initiator_role = ChoreographicRole::new(
            retained.manifest().initiator_device,
            subject_authority,
            RoleIndex::new(0).expect("role index"),
        );
        let invitee_role = ChoreographicRole::new(
            retained.manifest().invitee_device,
            subject_authority,
            RoleIndex::new(1).expect("role index"),
        );
        let roles = vec![initiator_role, invitee_role];
        let peer_roles = BTreeMap::from([("Invitee".to_string(), invitee_role)]);
        let manifest = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::local_types();

        let result = async {
            let session = session_slot.insert(open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                "Initiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "device enrollment VM stage failed".into(),
                    source: Some(std::sync::Arc::new(error)),
                })
            })?);
            session.queue_send_bytes(to_vec(&request).map_err(|error| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "device enrollment VM stage failed".into(),
                    source: Some(std::sync::Arc::new(error)),
                })
            })?);

            let loop_result = budget
                .execute(effects.as_ref(), || Box::pin(async {
                    loop {
                        budget
                            .remaining_ms(effects.as_ref())
                            .await
                            .map_err(|source| {
                                budget.map_run_error(
                                    "device enrollment VM progression",
                                    TimeoutRunError::Timeout(source),
                                )
                            })?;
                        let round = session
                            .advance_round_until_receive(
                                "Initiator",
                                &peer_roles,
                                InvitationHandler::is_transport_no_message,
                            )
                            .await
                            .map_err(|error| {
                                AgentError::Aura(aura_core::AuraError::Internal {
                                    message: "device enrollment VM stage failed".into(),
                                    source: Some(std::sync::Arc::new(error)),
                                })
                            })?;

                        if let Some(blocked) = round.blocked_receive {
                            // Decode only bounded peer bytes; invalid packets never move the VM.
                            let max_response = aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_PUBLIC_KEY_PACKAGE_BYTES + 4096;
                            if blocked.payload.len() > max_response { continue; }
                            let Ok(response) = from_slice::<DeviceEnrollmentResponseWrapper>(&blocked.payload) else { continue; };
                            let Some(verified) = response_verifier.verify_received_response(effects.as_ref(), &response.0).await? else { continue; };
                            budget.remaining_ms(effects.as_ref()).await.map_err(|source| budget.map_run_error("verified response publication", TimeoutRunError::Timeout(source)))?;
                            match verified {
                                super::enrollment_trust::VerifiedEnrollmentResponseDispositionCapability::Accepted(verified) => {
                                    ceremony_runner.record_verified_enrollment_response(*verified).await.map_err(AgentError::from)?;
                                }
                                super::enrollment_trust::VerifiedEnrollmentResponseDispositionCapability::Refused(verified) => {
                                    ceremony_runner.record_verified_enrollment_rejection(verified).await.map_err(AgentError::from)?;
                                }
                            }
                            let confirm = enrollment_vm_admission::sign_terminal_confirmation(
                                effects.as_ref(),
                                retained,
                                &ceremony_runner,
                            )
                            .await?;
                            budget
                                .remaining_ms(effects.as_ref())
                                .await
                                .map_err(|source| {
                                    budget.map_run_error(
                                        "device enrollment acceptance send",
                                        TimeoutRunError::Timeout(source),
                                    )
                                })?;
                            session.queue_send_bytes(
                                to_vec(&confirm).map_err(|error| AgentError::Aura(error.into()))?,
                            );
                            session.inject_blocked_receive(blocked).map_err(|error| {
                                AgentError::Aura(aura_core::AuraError::Internal {
                                    message: "device enrollment VM stage failed".into(),
                                    source: Some(std::sync::Arc::new(error)),
                                })
                            })?;
                            continue;
                        }

                        if handle_invitation_vm_wait_status(
                            round.host_wait_status,
                            // Deferred only means the invitee has not answered yet; it
                            // imports the code on another device, possibly minutes later.
                            false,
                            "device enrollment initiator VM timed out while waiting for receive",
                            "device enrollment initiator VM cancelled while waiting for receive",
                        )?
                        .is_some()
                        {
                            break Ok(());
                        }

                        // A deferred receive leaves the VM blocked on the invitee's
                        // response (a step reports Stuck); keep waiting within the
                        // window rather than failing the attempt.
                        if matches!(round.host_wait_status, AuraVmHostWaitStatus::Deferred)
                            && matches!(round.step, StepResult::Stuck)
                        {
                            continue;
                        }

                        if handle_invitation_vm_step(
                            round.step,
                            "device enrollment initiator VM became stuck without a pending receive",
                        )? {
                            break Ok(());
                        }
                    }
                }))
                .await
                .map_err(|error| budget.map_run_error("device enrollment initiator VM", error));

            loop_result
        }
        .await;
        result
    }

    /// Run the invitee until the initiator's request arrives or the wait window
    /// closes. A single receive waits only a few seconds while the initiator
    /// re-sends on its own cycle; a request that arrives between attempts stays
    /// queued under the invitation's session for the next attempt.
    pub(super) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        tasks: &crate::task_registry::TaskGroup,
    ) -> AgentResult<()> {
        let admitted = super::enrollment_manifest_admission::load_admitted_baseline(
            effects.as_ref(),
            invitation.receiver_id,
            invitation,
        )
        .await
        .map_err(AgentError::EnrollmentManifest)?;
        match self
            .execute_device_enrollment_invitee_response(
                effects,
                Arc::new(admitted),
                tasks,
                EnrollmentInviteeChoice::Accept,
            )
            .await?
        {
            None => Ok(()),
            Some(reason) => Err(enrollment_vm_admission::terminal_failure(reason)),
        }
    }

    pub(super) async fn execute_device_enrollment_invitee_decline(
        &self,
        effects: Arc<AuraEffectSystem>,
        admitted: Arc<super::enrollment_manifest_admission::AdmittedEnrollmentManifest>,
        tasks: &crate::task_registry::TaskGroup,
    ) -> AgentResult<()> {
        match self
            .execute_device_enrollment_invitee_response(
                effects,
                admitted,
                tasks,
                EnrollmentInviteeChoice::Refuse,
            )
            .await?
        {
            Some(aura_app::runtime_bridge::CeremonyFailureReason::Rejected) => Ok(()),
            Some(reason) => Err(enrollment_vm_admission::terminal_failure(reason)),
            None => {
                Err(aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished.into())
            }
        }
    }

    async fn execute_device_enrollment_invitee_response(
        &self,
        effects: Arc<AuraEffectSystem>,
        admitted: Arc<super::enrollment_manifest_admission::AdmittedEnrollmentManifest>,
        tasks: &crate::task_registry::TaskGroup,
        choice: EnrollmentInviteeChoice,
    ) -> AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>> {
        let budget = EnrollmentWindowCapability::admitted(effects.clone(), admitted.as_ref())
            .await
            .map_err(AgentError::from)?;
        let notice_group = tasks.group(format!(
            "enrollment_terminal_notice.{}",
            admitted.manifest().invitation
        ));
        let (stop_tx, stop) = futures::channel::oneshot::channel::<()>();
        let (completed_tx, completed) = futures::channel::oneshot::channel();
        let notice_effects = effects.clone();
        let notice_admitted = admitted.clone();
        let notice_budget = budget.clone();
        let callback = Box::pin(async move {
            let mut slot = None;
            let result = {
                let receive = notice_budget.execute(notice_effects.as_ref(), || {
                    Box::pin(super::enrollment_terminal_notice::receive_cancelled_notice(
                        notice_effects.clone(),
                        notice_admitted.as_ref(),
                        &notice_budget,
                        &mut slot,
                    ))
                });
                futures::pin_mut!(receive, stop);
                match futures::future::select(receive, stop).await {
                    futures::future::Either::Left((result, _)) => {
                        result.map(Some).map_err(|source| {
                            notice_budget.map_run_error("enrollment terminal notice", source)
                        })
                    }
                    // Both an explicit stop and sender drop withdraw this owned
                    // listener. Neither branch manufactures terminal evidence.
                    futures::future::Either::Right((_, _)) => Ok(None),
                }
            };
            let result = finish_enrollment_vm_slot(result, slot.take()).await;
            let result = result.map_err(Arc::new);
            let failure = result.as_ref().err().cloned();
            // An abandoned receiver cannot authorize activation. On failure,
            // the callback still retains the same actual cause in task health.
            if completed_tx.send(result).is_err() {
                tracing::debug!("enrollment terminal notice observer was withdrawn");
            }
            match failure {
                None => Ok(()),
                Some(source) => Err(aura_core::AuraError::Internal {
                    message: "enrollment terminal notice listener failed".into(),
                    source: Some(source),
                }),
            }
        });
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                let _owned_notice = notice_group.spawn_local_try_named("receive_terminal_notice", callback);
            } else {
                let _owned_notice = notice_group.spawn_try_named("receive_terminal_notice", callback);
            }
        }
        let mut slot = None;
        enum Selected {
            Response(AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>>),
            Notice(
                Result<
                    Result<
                        Option<super::enrollment_vm_admission::VerifiedEnrollmentFailureCapability>,
                        Arc<AgentError>,
                    >,
                    futures::channel::oneshot::Canceled,
                >,
            ),
        }
        let selected = {
            let response = Box::pin(self.run_device_enrollment_invitee_window(
                effects.clone(),
                admitted.as_ref(),
                choice,
                &budget,
                &mut slot,
            ));
            futures::pin_mut!(response, completed);
            match futures::future::select(response, completed.as_mut()).await {
                futures::future::Either::Left((result, _)) => {
                    let signalled = stop_tx.send(()).is_ok();
                    tracing::debug!(signalled, "withdraw owned terminal notice listener");
                    let closed = budget
                        .execute(effects.as_ref(), || async {
                            let outcome = completed.await.map_err(|source| {
                                enrollment_stage_error("drain terminal notice owner", source)
                            })?;
                            notice_group
                                .await_owned_task_completion()
                                .await
                                .map_err(|source| {
                                    enrollment_stage_error(
                                        "observe terminal notice task completion",
                                        source,
                                    )
                                })?;
                            Ok(outcome)
                        })
                        .await
                        .map_err(|source| {
                            budget.map_run_error("drain terminal notice owner", source)
                        });
                    match closed {
                        Ok(Ok(None)) => Selected::Response(result),
                        Ok(Ok(Some(failed))) => Selected::Notice(Ok(Ok(Some(failed)))),
                        Ok(Err(source)) => Selected::Notice(Ok(Err(source))),
                        Err(source) => {
                            let abort = notice_group.force_abort_remaining();
                            Selected::Response(finish_enrollment_vm_attempt(
                                finish_enrollment_vm_attempt(result, Err(source)),
                                abort,
                            ))
                        }
                    }
                }
                futures::future::Either::Right((result, _)) => Selected::Notice(result),
            }
        };
        match selected {
            Selected::Response(result) => finish_enrollment_vm_slot(result, slot.take()).await,
            Selected::Notice(result) => {
                let evidence = result
                    .map_err(|source| {
                        enrollment_stage_error("observe terminal notice owner", source)
                    })
                    .and_then(|result| {
                        result.map_err(|source| {
                            AgentError::Aura(aura_core::AuraError::Internal {
                                message: "enrollment terminal notice listener failed".into(),
                                source: Some(source),
                            })
                        })
                    });
                let evidence = finish_enrollment_vm_slot(evidence, slot.take())
                    .await?
                    .ok_or_else(|| {
                        AgentError::internal(
                            "terminal notice owner withdrew before response settlement",
                        )
                    })?;
                let drained = budget
                    .execute(effects.as_ref(), || async {
                        notice_group
                            .await_owned_task_completion()
                            .await
                            .map_err(|source| {
                                enrollment_stage_error(
                                    "observe terminal notice task completion",
                                    source,
                                )
                            })
                    })
                    .await
                    .map_err(|source| budget.map_run_error("drain terminal notice owner", source));
                if let Err(source) = drained {
                    return finish_enrollment_vm_attempt(
                        Err(source),
                        notice_group.force_abort_remaining(),
                    );
                }
                let acknowledged = budget
                    .acknowledge_failure(effects.as_ref(), &evidence)
                    .await
                    .map_err(AgentError::from)?;
                let retained = super::enrollment_manifest_admission::retain_verified_failure(
                    effects.as_ref(),
                    evidence,
                    acknowledged,
                )
                .await
                .map_err(AgentError::EnrollmentManifest)?;
                Ok(Some(retained.evidence().reason()))
            }
        }
    }

    async fn run_device_enrollment_invitee_window(
        &self,
        effects: Arc<AuraEffectSystem>,
        admitted: &super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        choice: EnrollmentInviteeChoice,
        budget: &EnrollmentWindowCapability,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
    ) -> AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>> {
        loop {
            let attempt_budget = budget
                .child(
                    effects.as_ref(),
                    std::time::Duration::from_millis(INVITATION_VM_LOOP_TIMEOUT_MS),
                )
                .await
                .map_err(|source| {
                    budget
                        .map_run_error("device enrollment window", TimeoutRunError::Timeout(source))
                })?;

            let attempt = attempt_budget
                .execute(effects.as_ref(), || {
                    Box::pin(self.run_device_enrollment_invitee_attempt(
                        effects.clone(),
                        admitted,
                        &attempt_budget,
                        choice,
                        session_slot,
                    ))
                })
                .await
                .map_err(|source| {
                    attempt_budget.map_run_error("device enrollment invitee attempt", source)
                });
            match finish_enrollment_vm_slot(attempt, session_slot.take()).await {
                Ok(outcome) => return Ok(outcome),
                Err(error) if error.is_timeout() && !enrollment_attempt_failed_to_close(&error) => {
                    tracing::debug!(
                        invitation_id = %admitted.canonical_invitation().invitation_id,
                        error = %error,
                        "device enrollment invitee attempt ended; waiting for the initiator again"
                    );
                    budget
                        .retry_delay(effects.as_ref(), DEVICE_ENROLLMENT_INVITEE_RETRY_DELAY_MS)
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "device enrollment retry",
                                TimeoutRunError::Timeout(source),
                            )
                        })?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_device_enrollment_invitee_attempt(
        &self,
        effects: Arc<AuraEffectSystem>,
        admitted: &super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        budget: &EnrollmentWindowCapability,
        choice: EnrollmentInviteeChoice,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
    ) -> AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>> {
        let invitation = admitted.canonical_invitation();
        let expected_request = enrollment_vm_admission::expected_request(admitted);
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let admitted_manifest = admitted.manifest();
        let initiator_role = ChoreographicRole::new(
            admitted_manifest.initiator_device,
            admitted_manifest.subject,
            RoleIndex::new(0).expect("role index"),
        );
        let invitee_role = ChoreographicRole::new(
            admitted_manifest.invitee_device,
            admitted_manifest.subject,
            RoleIndex::new(1).expect("role index"),
        );
        let roles = vec![initiator_role, invitee_role];
        let peer_roles = BTreeMap::from([("Initiator".to_string(), initiator_role)]);
        let manifest = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::local_types();

        let session = session_slot.insert(
            open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                "Invitee",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "device enrollment VM stage failed".into(),
                    source: Some(std::sync::Arc::new(error)),
                })
            })?,
        );

        let loop_result = budget
            .execute(effects.as_ref(), || Box::pin(async {
                let mut matched_request = None;
                let mut confirmed = None;
                loop {
                    budget
                        .remaining_ms(effects.as_ref())
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "device enrollment VM progression",
                                TimeoutRunError::Timeout(source),
                            )
                        })?;
                    let round = session
                        .advance_round("Invitee", &peer_roles)
                        .await
                        .map_err(|error| {
                            AgentError::Aura(aura_core::AuraError::Internal {
                                message: "device enrollment VM stage failed".into(),
                                source: Some(std::sync::Arc::new(error)),
                            })
                        })?;

                    if let Some(blocked) = round.blocked_receive {
                        if let Some(request) = &matched_request {
                            let confirmation = EnrollmentControlFrame::decode(&blocked.payload)?;
                            let evidence = confirmation
                                .verify_terminal(effects.as_ref(), admitted, request)
                                .await?;
                            budget
                                .remaining_ms(effects.as_ref())
                                .await
                                .map_err(|source| {
                                    budget.map_run_error(
                                        "device enrollment confirmation publication",
                                        TimeoutRunError::Timeout(source),
                                    )
                                })?;
                            confirmed = Some(evidence);
                        } else {
                            let frame = EnrollmentControlFrame::decode(&blocked.payload)?;
                            let request = frame.verify_request(effects.as_ref(), admitted).await?;
                            request.validate_against(&expected_request)?;
                            let response = DeviceEnrollmentResponseWrapper(match choice {
                                EnrollmentInviteeChoice::Accept => DeviceEnrollmentResponse::Accepted(enrollment_vm_admission::sign_acceptance_for_request(effects.as_ref(), admitted, &request).await?),
                                EnrollmentInviteeChoice::Refuse => DeviceEnrollmentResponse::Refused(enrollment_vm_admission::sign_refusal_for_request(effects.as_ref(), admitted, &request).await?),
                            });
                            session.queue_send_bytes(
                                to_vec(&response).map_err(|error| AgentError::Aura(error.into()))?,
                            );
                            matched_request = Some(request);
                        }
                        session.inject_blocked_receive(blocked).map_err(|error| {
                            AgentError::Aura(aura_core::AuraError::Internal {
                                message: "device enrollment VM stage failed".into(),
                                source: Some(std::sync::Arc::new(error)),
                            })
                        })?;
                        continue;
                    }

                    if handle_invitation_vm_wait_status(
                        round.host_wait_status,
                        false,
                        "device enrollment invitee VM timed out while waiting for receive",
                        "device enrollment invitee VM cancelled while waiting for receive",
                    )?
                    .is_some()
                    {
                        break confirmed.take().ok_or_else(|| aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished.into());
                    }

                    if handle_invitation_vm_step(
                        round.step,
                        "device enrollment invitee VM became stuck without a pending receive",
                    )? {
                        if confirmed.is_none() {
                            return Err(
                            aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished
                                .into(),
                        );
                        }
                        break confirmed.take().ok_or_else(|| aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished.into());
                    }
                }
            }))
            .await
            .map_err(|error| budget.map_run_error("device enrollment invitee VM", error));

        let publication = async {
            match loop_result {
                Ok(enrollment_vm_admission::VerifiedEnrollmentTerminal::Committed(confirmed)) => {
                    if choice == EnrollmentInviteeChoice::Refuse {
                        Err(
                            aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished
                                .into(),
                        )
                    } else {
                        let acknowledged = budget
                            .acknowledge_confirmation(effects.as_ref())
                            .await
                            .map_err(AgentError::from)?;
                        super::enrollment_manifest_admission::retain_verified_confirmation(
                            effects.as_ref(),
                            confirmed,
                            acknowledged,
                        )
                        .await
                        .map_err(AgentError::EnrollmentManifest)?;
                        Ok(None)
                    }
                }
                Ok(enrollment_vm_admission::VerifiedEnrollmentTerminal::Failed(failed)) => {
                    let acknowledged = budget
                        .acknowledge_failure(effects.as_ref(), &failed)
                        .await
                        .map_err(AgentError::from)?;
                    let retained = super::enrollment_manifest_admission::retain_verified_failure(
                        effects.as_ref(),
                        failed,
                        acknowledged,
                    )
                    .await
                    .map_err(AgentError::EnrollmentManifest)?;
                    Ok(Some(retained.evidence().reason()))
                }
                Err(source) => Err(source),
            }
        }
        .await;
        publication
    }
}

/// Verify an invitee's device-enrollment acceptance before it is counted.
///
/// The acceptance must come from the invited authority, match this
/// invitation, ceremony, and device, and carry a valid signature over the
/// acceptance transcript.
#[cfg(test)]
pub(super) async fn verify_device_enrollment_acceptance(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
    subject_authority: AuthorityId,
    ceremony_id: &CeremonyId,
    device_id: DeviceId,
    accept: &DeviceEnrollmentAccept,
) -> AgentResult<super::VerifiedEnrollmentResponse> {
    if accept.acceptor_id != invitation.receiver_id {
        return Err(invitation_invalid_error(
            "device enrollment acceptance does not match invited authority",
            format_args!("{} != {}", accept.acceptor_id, invitation.receiver_id),
        ));
    }
    if accept.invitation_id != invitation.invitation_id
        || &accept.ceremony_id != ceremony_id
        || accept.device_id != device_id
    {
        return Err(AgentError::invalid(
            "device enrollment acceptance does not match this invitation".to_string(),
        ));
    }
    let InvitationType::DeviceEnrollment {
        pending_epoch,
        initiator_device_id,
        ..
    } = &invitation.invitation_type
    else {
        return Err(AgentError::invalid("expected enrollment invitation"));
    };
    let verifier = super::enrollment_trust::RetainedEnrollmentVerifier::load(
        effects,
        subject_authority,
        ceremony_id,
        *pending_epoch,
        *initiator_device_id,
        invitation.receiver_id,
        device_id,
    )
    .await?;
    verifier
        .verify_acceptance(effects, invitation, accept)
        .await
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use aura_core::effects::time::TimeError;
    use std::error::Error;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn request_send_failure(destination: AuthorityId, label: &str) -> AgentError {
        let session_id =
            crate::runtime::subsystems::choreography::RuntimeChoreographySessionId::from_uuid(
                uuid::Uuid::from_bytes([41; 16]),
            );
        let cause = crate::runtime::session_ingress::SessionIngressError::BridgeRound {
            session_id,
            owner_label: "actual enrollment request owner".into(),
            source: crate::runtime::vm_host_bridge::AuraVmBridgeRoundError::Send {
                from_role: "Initiator".into(),
                to_role: "Invitee".into(),
                label: label.into(),
                source: Box::new(aura_protocol::effects::ChoreographyError::Transport {
                    source: Box::new(aura_core::effects::TransportError::DestinationUnreachable {
                        destination,
                    }),
                }),
            },
        };
        enrollment_stage_error("required initial request send", cause)
    }

    #[test]
    fn enrollment_request_retry_requires_exact_native_send_and_destination() {
        let subject = AuthorityId::from_uuid(uuid::Uuid::from_bytes([42; 16]));
        let other = AuthorityId::from_uuid(uuid::Uuid::from_bytes([43; 16]));
        let error = request_send_failure(subject, "DeviceEnrollmentRequest");
        assert!(enrollment_request_peer_unreachable(&error, subject));
        assert!(!enrollment_request_peer_unreachable(&error, other));
        assert!(!enrollment_request_peer_unreachable(
            &request_send_failure(other, "DeviceEnrollmentRequest"),
            subject
        ));
        assert!(!enrollment_request_peer_unreachable(
            &request_send_failure(subject, "DeviceEnrollmentConfirm"),
            subject
        ));
        let diagnostic = AgentError::internal(error.to_string());
        assert!(!enrollment_request_peer_unreachable(&diagnostic, subject));
        let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
        let mut original_transport = None;
        while let Some(source) = cause {
            if let Some(transport) = source.downcast_ref::<aura_core::effects::TransportError>() {
                original_transport = Some(transport);
                break;
            }
            cause = source.source();
        }
        assert!(matches!(original_transport,
            Some(aura_core::effects::TransportError::DestinationUnreachable { destination })
                if *destination == subject));
    }

    #[test]
    fn required_close_retains_both_causes_and_prevents_request_retry() {
        let subject = AuthorityId::from_uuid(uuid::Uuid::from_bytes([48; 16]));
        let execution = request_send_failure(subject, "DeviceEnrollmentRequest");
        let close = std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "actual close provider failure",
        );
        let result: AgentResult<()> = finish_enrollment_vm_attempt(Err(execution), Err(close));
        let error = result.expect_err("failed close must prevent a successful attempt");
        assert!(enrollment_attempt_failed_to_close(&error));
        assert!(!enrollment_request_peer_unreachable(&error, subject));
        let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
        let mut composite = None;
        while let Some(source) = cause {
            if let Some(failure) = source.downcast_ref::<EnrollmentVmTeardownFailure>() {
                composite = Some(failure);
                break;
            }
            cause = source.source();
        }
        let composite = composite.expect("both native failures must remain accessible");
        assert_eq!(
            composite
                .close_error()
                .downcast_ref::<std::io::Error>()
                .expect("original close provider type survives")
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        assert!(
            enrollment_request_peer_unreachable(&composite.execution, subject),
            "the original transport cause survives without granting composite retry"
        );
    }

    struct Clock {
        now: AtomicU64,
        read_fails: bool,
        sleep_fails: bool,
        slept: AtomicU64,
    }

    impl Clock {
        fn new(now: u64) -> Self {
            Self {
                now: AtomicU64::new(now),
                read_fails: false,
                sleep_fails: false,
                slept: AtomicU64::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for Clock {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            if self.read_fails {
                return Err(TimeError::ServiceUnavailable);
            }
            Ok(PhysicalTime {
                ts_ms: self.now.load(Ordering::SeqCst),
                uncertainty: None,
            })
        }

        async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
            if self.sleep_fails {
                return Err(TimeError::ServiceUnavailable);
            }
            self.slept.fetch_add(ms, Ordering::SeqCst);
            self.now.fetch_add(ms, Ordering::SeqCst);
            Ok(())
        }
    }

    fn window() -> TimeoutBudget {
        TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            },
            std::time::Duration::from_millis(10),
        )
        .unwrap()
    }

    fn contains_time_source(error: &AgentError) -> bool {
        let mut cause: Option<&(dyn Error + 'static)> = Some(error);
        while let Some(current) = cause {
            if current.is::<TimeError>() {
                return true;
            }
            cause = current.source();
        }
        false
    }

    #[tokio::test]
    async fn unavailable_owner_clock_is_required_failure_with_original_source() {
        let mut clock = Clock::new(100);
        clock.read_fails = true;
        let error = enrollment_remaining_ms(&clock, &window())
            .await
            .unwrap_err();
        assert!(
            !error.is_timeout(),
            "clock failure cannot enter timeout retry"
        );
        assert!(contains_time_source(&error));
        assert_eq!(clock.slept.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn child_attempt_and_backoff_cannot_extend_the_owned_window() {
        let clock = Clock::new(107);
        let budget = window();
        let child = enrollment_attempt_budget(&clock, &budget).await.unwrap();
        assert_eq!(child.deadline_at_ms(), budget.deadline_at_ms());
        assert_eq!(child.timeout_ms(), 3);
        enrollment_retry_delay(&clock, &budget, 500).await.unwrap();
        assert_eq!(clock.slept.load(Ordering::SeqCst), 3);
        let error = enrollment_attempt_budget(&clock, &budget)
            .await
            .unwrap_err();
        assert!(error.is_timeout());
        let original = error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<aura_core::TimeoutBudgetError>()
            .unwrap();
        assert!(matches!(
            original,
            aura_core::TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 110,
                observed_at_ms: 110,
            }
        ));
        assert!(enrollment_retry_delay(&clock, &budget, 500)
            .await
            .unwrap_err()
            .is_timeout());
        assert_eq!(clock.slept.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn failed_retry_sleep_retains_cause_and_is_not_a_timeout() {
        let mut clock = Clock::new(100);
        clock.sleep_fails = true;
        let error = enrollment_retry_delay(&clock, &window(), 500)
            .await
            .unwrap_err();
        assert!(!error.is_timeout());
        assert!(contains_time_source(&error));
        assert_eq!(clock.now.load(Ordering::SeqCst), 100);
    }
}

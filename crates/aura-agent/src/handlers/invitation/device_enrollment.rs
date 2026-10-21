use super::enrollment_vm_admission::{self, EnrollmentControlFrame};
use super::vm_loop::{handle_invitation_vm_step, handle_invitation_vm_wait_status};
#[cfg(test)]
use super::vm_loop::{invitation_invalid_error, map_invitation_vm_timeout};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use crate::runtime::services::enrollment_window::EnrollmentExecutionChild;
use crate::runtime::services::issuer_task_custody::{
    OriginalNoticeExecution, OriginalNoticeTaskCustody, RetainedNoticeTaskState,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnrollmentInviteeChoice {
    Accept,
    Refuse,
}

/// Routing only: all signed evidence and disposal proofs remain in custody.
pub(crate) enum OriginalInviteeInterruptionDisposition {
    Disposed,
    ReconcileSignedNotice,
}

/// Move-only terminal state retained by the original invitee task owner.
/// Awaited protected writes borrow these values; cancellation cannot consume
/// the signed proof or the one issued original-window acknowledgment.
pub(crate) struct OriginalInviteeTerminalCustody {
    terminal: Option<enrollment_vm_admission::VerifiedEnrollmentTerminal>,
    vm: RetainedInviteeVm,
    notice: RetainedInviteeNotice,
    acknowledged: Option<crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow>,
    retained: Option<super::enrollment_manifest_admission::OriginalInviteeRetentionWitness>,
    frames_verified: bool,
    cancelled_original_disposed: bool,
}

impl OriginalInviteeTerminalCustody {
    pub(crate) fn new() -> Self {
        Self {
            terminal: None,
            vm: RetainedInviteeVm::default(),
            notice: RetainedInviteeNotice::default(),
            acknowledged: None,
            retained: None,
            frames_verified: false,
            cancelled_original_disposed: false,
        }
    }

    pub(crate) fn completed(&self) -> bool {
        self.frames_verified
            && self.retained_original().is_ok()
            && self.vm.session.is_none()
            && self.notice.drained
    }

    pub(crate) fn failed_response_disposed(&self) -> bool {
        matches!(self.vm.response_outcome.as_ref(), Some(Err(_)))
            && self.vm.session.is_none()
            && self.vm.pending_receive.is_none()
            && self.vm.eligible.is_none()
            && self.notice.drained
            && !self.notice.observed_candidate
            && self.notice.failure.is_none()
            && self.terminal.is_none()
            && self.acknowledged.is_none()
            && self.retained.is_none()
            && !self.frames_verified
            && !self.has_signed_original_candidate()
            && self.vm.refused_candidate.as_ref().is_none_or(|refused| {
                self.vm
                    .postcheck_failure
                    .as_ref()
                    .is_some_and(|failure| Arc::ptr_eq(failure, &refused.failure))
            })
    }

    pub(crate) fn has_retained_original_progress(&self) -> bool {
        self.terminal.is_some() || self.vm.has_original_cursor() || self.notice.group.is_some()
    }

    pub(crate) fn cancelled_original_disposed(&self) -> bool {
        self.cancelled_original_disposed
            && self.vm.session.is_none()
            && self.vm.pending_receive.is_none()
            && self.notice.drained
            && self.terminal.is_none()
            && self.acknowledged.is_none()
            && self.retained.is_none()
            && !self.frames_verified
    }

    pub(crate) fn has_signed_original_candidate(&self) -> bool {
        self.terminal.is_some()
            || self.vm.candidate.is_some()
            || self.vm.eligible.is_some()
            || self.notice.observed_candidate
    }

    pub(crate) fn interrupted_notice_failure(&self) -> Option<&Arc<AgentError>> {
        self.cancelled_original_disposed
            .then_some(self.notice.failure.as_ref())
            .flatten()
    }

    /// Dispose the original, interrupted operation without resuming an
    /// unreachable transport receive or issuing a signed terminal.
    pub(crate) async fn dispose_cancelled_original(
        &mut self,
    ) -> AgentResult<OriginalInviteeInterruptionDisposition> {
        if self.terminal.is_some()
            || self.vm.eligible.is_some()
            || matches!(self.vm.response_outcome.as_ref(), Some(Ok(())))
        {
            return Err(AgentError::internal(
                "original signed invitee progress requires terminal reconciliation",
            ));
        }
        if let Some(stop) = self.notice.stop.take() {
            let _ = stop.send(());
        }
        if self.notice.group.is_some() && !self.notice.drained {
            self.notice.reconcile_original_task().await?;
            if !self.notice.observed_candidate
                && !self.notice.observed_empty
                && self.notice.failure.is_none()
            {
                let completed = self.notice.completed.as_mut().ok_or_else(|| {
                    AgentError::internal("original notice completion receiver was not retained")
                })?;
                let result = completed.await;
                self.notice.completed.take();
                match result {
                    Ok(Ok(true)) => self.notice.observed_candidate = true,
                    Ok(Ok(false)) => self.notice.observed_empty = true,
                    Ok(Err(source)) => {
                        self.notice.failure.get_or_insert(source);
                    }
                    Err(source) => {
                        self.notice.failure.get_or_insert_with(|| {
                            Arc::new(enrollment_stage_error(
                                "observe cancelled original notice",
                                source,
                            ))
                        });
                    }
                }
            }
            // A failed child is still an actual completed child only when its
            // original group has no active task left. Keep its typed failure.
            let joined = self.notice.drain_original_group().await;
            if let Err(source) = joined {
                if self
                    .notice
                    .group
                    .as_ref()
                    .is_some_and(|group| !group.active_tasks().is_empty())
                {
                    return Err(source);
                }
                self.notice.failure.get_or_insert_with(|| Arc::new(source));
                self.notice.drained = true;
            }
        } else if self.notice.group.is_none() {
            self.notice.drained = true;
        }
        if self.notice.observed_candidate {
            return Ok(OriginalInviteeInterruptionDisposition::ReconcileSignedNotice);
        }
        if self.notice.drained {
            let mut notice = self.notice.task_state.lock().await;
            if notice.vm.session.is_some() {
                return Err(AgentError::internal(
                    "original notice task has not acknowledged VM close",
                ));
            }
            notice.vm.retire_cancelled_receive_after_close();
        }
        #[cfg(test)]
        let pending_receive_before_close = self.vm.pending_receive.is_some();
        let close_result = self.vm.close_retained().await;
        #[cfg(test)]
        if close_result.is_err() && pending_receive_before_close {
            assert!(
                self.vm.session.is_some() && self.vm.pending_receive.is_some(),
                "original VM close refusal retains both nonclone session and receive"
            );
        }
        close_result?;
        self.vm.retire_failed_response_receive_after_close(true);
        self.cancelled_original_disposed = true;
        Ok(OriginalInviteeInterruptionDisposition::Disposed)
    }

    pub(crate) fn retained_original(&self) -> Result<(), AgentError> {
        let terminal = self.terminal.as_ref().ok_or_else(|| {
            AgentError::internal("original invitee terminal proof remains pending")
        })?;
        let acknowledged = self.acknowledged.as_ref().ok_or_else(|| {
            AgentError::internal("original invitee acknowledgment remains pending")
        })?;
        self.retained
            .as_ref()
            .ok_or_else(|| {
                AgentError::internal("original invitee receipt readback remains pending")
            })?
            .require_original(terminal, acknowledged)
            .map_err(AgentError::EnrollmentManifest)
    }
}

#[cfg(test)]
mod original_failed_response_disposal_tests {
    use super::*;

    #[test]
    fn failed_response_cannot_settle_before_original_notice_group_drain() {
        let mut original = OriginalInviteeTerminalCustody::new();
        original.vm.response_outcome = Some(Err(Arc::new(AgentError::Config(
            "original selected postcheck failed".into(),
        ))));
        assert!(!original.failed_response_disposed());
        original.notice.drained = true;
        assert!(original.failed_response_disposed());
        assert!(
            !original.completed(),
            "negative disposal is never a signed terminal"
        );
    }

    #[test]
    fn failed_response_retains_blocked_receive_until_original_vm_close_ack() {
        let mut original = OriginalInviteeTerminalCustody::new();
        original.vm.response_outcome = Some(Err(Arc::new(AgentError::Config(
            "original selected postcheck failed".into(),
        ))));
        original.notice.drained = true;
        original.vm.pending_receive = Some(crate::runtime::vm_host_bridge::BlockedVmReceive {
            from_role: "Initiator".into(),
            to_role: "Invitee".into(),
            peer_role: aura_protocol::effects::ChoreographicRole::new(
                DeviceId::new_from_entropy([19; 32]),
                AuthorityId::new_from_entropy([20; 32]),
                RoleIndex::new(0).expect("role index"),
            ),
            payload: Vec::new(),
        });
        original
            .vm
            .retire_failed_response_receive_after_close(false);
        assert!(original.vm.pending_receive.is_some());
        assert!(!original.failed_response_disposed());
        original.vm.retire_failed_response_receive_after_close(true);
        assert!(original.vm.pending_receive.is_none());
        assert!(original.failed_response_disposed());
        assert!(!original.completed());
    }
}

#[derive(Default)]
struct RetainedInviteeVm {
    // This exact child owns the original selected endpoint. Reentry must not
    // create a fresh per-attempt window after the VM has received a frame.
    attempt: Option<Arc<EnrollmentExecutionChild>>,
    session: Option<crate::runtime::session_ingress::OwnedVmSession>,
    request: Option<aura_invitation::protocol::DeviceEnrollmentRequest>,
    pending_receive: Option<crate::runtime::vm_host_bridge::BlockedVmReceive>,
    candidate: Option<enrollment_vm_admission::VerifiedEnrollmentTerminal>,
    // A selected postcheck refusal never grants terminal eligibility. Retain
    // its exact signed evidence and cause only after the original VM closes.
    refused_candidate: Option<RefusedOriginalInviteeCandidate>,
    eligible: Option<enrollment_vm_admission::VerifiedEnrollmentTerminal>,
    response_bytes: Option<Vec<u8>>,
    response_queued: bool,
    receive_injected: bool,
    vm_completed: bool,
    // Retain the original typed failure through an interrupted cleanup retry.
    response_outcome: Option<Result<(), Arc<AgentError>>>,
    injection_failure: Option<Arc<AgentError>>,
    postcheck_failure: Option<Arc<AgentError>>,
}

struct RefusedOriginalInviteeCandidate {
    _candidate: enrollment_vm_admission::VerifiedEnrollmentTerminal,
    failure: Arc<AgentError>,
}

pub(crate) type RetainedNoticeResult = Result<bool, Arc<AgentError>>;

#[derive(Debug)]
pub(crate) struct RetainedOriginalCleanupFailure {
    pub(crate) primary: Arc<AgentError>,
    pub(crate) close: Arc<AgentError>,
}

impl std::fmt::Display for RetainedOriginalCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "original enrollment stage failed and its cleanup also failed ({:?})",
            terminal_notice_failure_categories(self.close.as_ref())
        )
    }
}

impl std::error::Error for RetainedOriginalCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

/// Issued only after original notice eligibility, VM close and exact task ACK.
/// The signed-frame digest binds this resource proof to its original failure.
pub(crate) struct AcknowledgedOriginalNoticeDisposal {
    digest: [u8; 32],
    task: crate::task_registry::OriginalRequiredTaskDisposalObservation,
}
impl AcknowledgedOriginalNoticeDisposal {
    pub(crate) fn require_original(
        &self,
        digest: &[u8; 32],
        disposal: &crate::task_registry::TaskGroupDisposalAcknowledgment,
    ) -> Result<(), aura_core::AuraError> {
        if digest != &self.digest {
            return Err(aura_core::AuraError::permission_denied(
                "foreign signed notice disposal",
            ));
        }
        self.task.require_only_original_task(disposal)
    }
}

#[derive(Default)]
struct RetainedInviteeNotice {
    task_state: RetainedNoticeTaskState,
    group: Option<crate::task_registry::TaskGroup>,
    control: Option<crate::task_registry::OriginalRequiredTaskHandle>,
    stop: Option<futures::channel::oneshot::Sender<()>>,
    completed: Option<futures::channel::oneshot::Receiver<RetainedNoticeResult>>,
    observed_candidate: bool,
    failure: Option<Arc<AgentError>>,
    observed_empty: bool,
    drained: bool,
    disposal: Option<AcknowledgedOriginalNoticeDisposal>,
}

impl RetainedInviteeVm {
    fn retire_failed_response_receive_after_close(&mut self, close_acknowledged: bool) {
        if close_acknowledged {
            debug_assert!(self.session.is_none());
            self.pending_receive.take();
        }
    }

    async fn close_retained(&mut self) -> AgentResult<()> {
        if let Some(session) = self.session.as_mut() {
            session.close_retained().await.map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Internal {
                    message: "close original invitee VM slot".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
            self.session.take();
            if matches!(self.response_outcome.as_ref(), Some(Err(_)))
                && self.eligible.is_none()
                && self.refused_candidate.is_none()
            {
                if let Some(failure) = self.postcheck_failure.as_ref() {
                    if let Some(candidate) = self.candidate.take() {
                        self.refused_candidate = Some(RefusedOriginalInviteeCandidate {
                            _candidate: candidate,
                            failure: Arc::clone(failure),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn has_original_cursor(&self) -> bool {
        self.attempt.is_some()
            || self.session.is_some()
            || self.request.is_some()
            || self.candidate.is_some()
            || self.eligible.is_some()
            || self.pending_receive.is_some()
            || self.response_bytes.is_some()
            || self.response_outcome.is_some()
    }
}

impl RetainedInviteeNotice {
    async fn retain_signed_disposal(&mut self) -> AgentResult<bool> {
        if self.disposal.is_some() {
            return Ok(true);
        }
        let retained = self.task_state.lock().await;
        if !retained.vm.eligible {
            return Ok(false);
        }
        if retained.vm.session.is_some() {
            return Err(AgentError::internal(
                "eligible notice VM close remains unacknowledged",
            ));
        }
        let digest = retained
            .vm
            .candidate
            .as_ref()
            .ok_or_else(|| {
                AgentError::internal("eligible original notice lost its signed candidate")
            })?
            .terminal_frame_digest()?;
        let failure = retained.failure.clone();
        drop(retained);
        let control = self
            .control
            .as_ref()
            .ok_or_else(|| AgentError::internal("original notice task control was not retained"))?;
        let task = control.disposal_observer().await_disposal().await;
        task.require_original(control).map_err(AgentError::from)?;
        let group = self
            .group
            .as_ref()
            .ok_or_else(|| AgentError::internal("original notice group was not retained"))?;
        let disposal = group.seal_admission().await_disposal().await;
        task.require_only_original_task(&disposal)
            .map_err(AgentError::from)?;
        if let Some(failure) = failure {
            self.failure.get_or_insert(failure);
        }
        self.disposal = Some(AcknowledgedOriginalNoticeDisposal { digest, task });
        self.observed_candidate = true;
        self.drained = true;
        Ok(true)
    }

    async fn reconcile_original_task(&mut self) -> AgentResult<()> {
        if let Some(control) = self.control.as_mut() {
            use aura_core::effects::task::RequiredTaskControl;
            use futures::FutureExt;
            control
                .await_original_attempt_disposition()
                .await
                .map_err(AgentError::from)?;
            if control
                .disposal_observer()
                .await_disposal()
                .now_or_never()
                .is_none()
            {
                cfg_if::cfg_if! {
                    if #[cfg(target_arch = "wasm32")] {
                        control.reconcile_local().await.map_err(AgentError::from)?;
                    } else {
                        control.reconcile().await.map_err(AgentError::from)?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn drain_original_group(&mut self) -> AgentResult<()> {
        if self.drained {
            return Ok(());
        }
        self.reconcile_original_task().await?;
        let group = self.group.as_ref().ok_or_else(|| {
            AgentError::internal("original terminal notice group was not retained")
        })?;
        group
            .await_owned_task_completion()
            .await
            .map_err(|source| {
                enrollment_stage_error("observe original terminal notice completion", source)
            })?;
        self.drained = true;
        Ok(())
    }
}

/// Log only bounded error categories. Enrollment errors can contain protocol
/// material in their Display text, so browser diagnostics must not print the
/// source chain itself.
fn terminal_notice_failure_categories(error: &AgentError) -> Vec<&'static str> {
    use std::error::Error;
    let mut categories = Vec::with_capacity(8);
    let mut cause: Option<&(dyn Error + 'static)> = Some(error);
    for _ in 0..8 {
        let Some(current) = cause else { break };
        let category = if let Some(agent) = current.downcast_ref::<AgentError>() {
            match agent {
                AgentError::TimeoutWithSource { .. } | AgentError::Timeout(_) => "agent_timeout",
                AgentError::Aura(_) => "agent_aura",
                AgentError::Config(_) => "agent_config",
                AgentError::Runtime(_) => "agent_runtime",
                AgentError::Context(_) => "agent_context",
                AgentError::Effects(_) => "agent_effects",
                AgentError::Choreography(_) => "agent_choreography",
                AgentError::DeviceEnrollmentMessage(_) => "agent_enrollment_message",
                AgentError::EnrollmentManifest(_) => "agent_enrollment_manifest",
                AgentError::UnresolvedDeviceBinding { .. } => "agent_device_binding",
            }
        } else if let Some(aura) = current.downcast_ref::<aura_core::AuraError>() {
            match aura {
                aura_core::AuraError::Network { .. } => "aura_network",
                aura_core::AuraError::Storage { .. } => "aura_storage",
                aura_core::AuraError::Crypto { .. } => "aura_crypto",
                aura_core::AuraError::Serialization { .. } => "aura_serialization",
                aura_core::AuraError::Internal { .. } => "aura_internal",
                _ => "aura_other",
            }
        } else if let Some(ingress) =
            current.downcast_ref::<crate::runtime::session_ingress::SessionIngressError>()
        {
            match ingress {
                crate::runtime::session_ingress::SessionIngressError::SessionStart { .. } => {
                    "session_start"
                }
                crate::runtime::session_ingress::SessionIngressError::BridgeRound { .. } => {
                    "session_bridge_round"
                }
                crate::runtime::session_ingress::SessionIngressError::SessionClose { .. } => {
                    "session_close"
                }
                _ => "session_other",
            }
        } else if let Some(bridge) =
            current.downcast_ref::<crate::runtime::vm_host_bridge::AuraVmBridgeRoundError>()
        {
            match bridge {
                crate::runtime::vm_host_bridge::AuraVmBridgeRoundError::Send { .. } => {
                    "bridge_send"
                }
                crate::runtime::vm_host_bridge::AuraVmBridgeRoundError::Receive { .. } => {
                    "bridge_receive"
                }
                crate::runtime::vm_host_bridge::AuraVmBridgeRoundError::Step { .. } => {
                    "bridge_step"
                }
                _ => "bridge_other",
            }
        } else if current.is::<aura_core::TimeoutBudgetError>() {
            "timeout_budget"
        } else if current.is::<aura_core::effects::TransportError>() {
            "transport"
        } else if current.is::<EnrollmentVmTeardownFailure>() {
            "vm_teardown"
        } else {
            "other"
        };
        categories.push(category);
        cause = current.source();
    }
    categories
}

#[cfg(test)]
#[test]
fn terminal_notice_diagnostic_retains_typed_cause_without_error_text() {
    assert_eq!(
        terminal_notice_failure_categories(&AgentError::runtime("secret enrollment payload")),
        ["agent_runtime"]
    );
    let error = AgentError::Aura(aura_core::AuraError::Internal {
        message: "secret enrollment payload".into(),
        source: Some(Arc::new(
            aura_core::TimeoutBudgetError::ObservationUnavailable,
        )),
    });
    assert_eq!(
        terminal_notice_failure_categories(&error),
        ["agent_aura", "aura_internal", "timeout_budget"]
    );
    let mut nested = aura_core::AuraError::Internal {
        message: "secret enrollment payload".into(),
        source: None,
    };
    for _ in 0..12 {
        nested = aura_core::AuraError::Internal {
            message: "secret enrollment payload".into(),
            source: Some(Arc::new(nested)),
        };
    }
    let categories = terminal_notice_failure_categories(&AgentError::Aura(nested));
    assert_eq!(categories.len(), 8);
    assert!(categories
        .iter()
        .all(|category| !category.contains("secret")));
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
/// handle before required asynchronous teardown has acknowledged completion.
/// A refused or interrupted close leaves the same capability in the slot.
pub(super) async fn finish_enrollment_vm_slot<T>(
    result: AgentResult<T>,
    slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
) -> AgentResult<T> {
    let Some(session) = slot.as_mut() else {
        return result;
    };
    let closed = session.close_retained().await;
    if closed.is_ok() {
        // No await separates the close acknowledgment from releasing its owner.
        slot.take();
    }
    finish_enrollment_vm_attempt(result, closed)
}

/// Keep a failed original operation outside a cancellable close frame while
/// preserving its exact error category for the caller's normal retry decision.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "EnrollmentExecutionChild",
    family = "runtime_helper"
)]
async fn finish_original_enrollment_vm_slot(
    result: AgentResult<()>,
    slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
    destination: &crate::runtime::services::issuer_terminal_destination::OriginalIssuerTerminalDestination,
    effects: &AuraEffectSystem,
    issued: &super::enrollment_trust::RetainedEnrollmentVmControl,
    original: &EnrollmentExecutionChild,
) -> AgentResult<()> {
    let error = match result {
        Ok(()) => return finish_enrollment_vm_slot(Ok(()), slot).await,
        Err(error) => error,
    };
    let Some(session) = slot.as_mut() else {
        return Err(error);
    };
    let pending = destination
        .hold_original_vm_operation_failure(effects, issued, original, session, error)?;
    let closed = session.close_retained().await;
    if closed.is_ok() {
        slot.take();
    }
    finish_enrollment_vm_attempt(Err(pending.finish()), closed)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn original_enrollment_vm_read_fault_for_test<'a>(
    effects: &'a AuraEffectSystem,
    owner: &crate::runtime::RuntimeSessionOwner,
) -> Option<futures::future::BoxFuture<'a, ()>> {
    let clock = {
        let mut fault = effects.original_enrollment_vm_read_fault.lock();
        if fault
            .as_ref()
            .is_some_and(|(session, _)| *session == owner.session_id.as_uuid())
        {
            fault.take().map(|(_, clock)| clock)
        } else {
            None
        }
    };
    clock.map(|clock| {
        effects
            .assert_owned_choreography_session(owner)
            .expect("actual original VM remains owned before its operation read");
        let selected: Arc<dyn aura_core::effects::PhysicalTimeEffects> = clock.clone();
        assert!(
            effects.time_effects().selected_provider_is(&selected),
            "operation read fault retains the original selected physical provider"
        );
        let fault: futures::future::BoxFuture<'a, ()> = Box::pin(async move {
            clock
                .fail_next_current_task_observation(
                    aura_core::effects::TimeError::OperationFailed {
                        reason: "original issuer progress read before cancellation".into(),
                    },
                )
                .await
                .expect("same original task arms its actual operation read");
        });
        fault
    })
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

pub(crate) fn enrollment_stage_error(
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

pub(super) struct InvitationDeviceEnrollmentHandler {
    #[cfg(test)]
    invitation_cache: Arc<crate::runtime::services::invitation_manager::InvitationManager>,
}

impl InvitationDeviceEnrollmentHandler {
    pub(super) fn new(_handler: &InvitationHandler) -> Self {
        Self {
            #[cfg(test)]
            invitation_cache: Arc::clone(&_handler.invitation_cache),
        }
    }

    /// Run the initiator until the new device accepts or the acceptance window
    /// closes. The new device only becomes reachable as a device of this
    /// authority once a person imports the code on it, so each attempt re-opens
    /// the session and re-sends the request.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentExecutionChild",
        family = "runtime_helper"
    )]
    pub(super) async fn execute_device_enrollment_initiator_owned(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: Arc<super::enrollment_trust::RetainedEnrollmentVmControl>,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        retirement_destination: Arc<
            crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
        >,
    ) -> AgentResult<super::IssuerEnrollmentTerminalOutcome> {
        self.execute_original_enrollment_terminal(
            effects,
            retained,
            ceremony_runner,
            budget,
            retirement_destination,
            None,
        )
        .await
    }

    pub(super) async fn resume_original_enrollment_cancellation(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: Arc<super::enrollment_trust::RetainedEnrollmentVmControl>,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        retirement_destination: Arc<
            crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
        >,
        cancellation: crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    ) -> AgentResult<super::IssuerEnrollmentTerminalOutcome> {
        cancellation.require_runtime_owner(&effects)?;
        if cancellation.ceremony() != &retained.manifest().ceremony
            || cancellation.invitation() != &retained.manifest().invitation
        {
            return Err(AgentError::invalid(
                "cancellation belongs to another original issuance",
            ));
        }
        self.execute_original_enrollment_terminal(
            effects,
            retained,
            ceremony_runner,
            budget,
            retirement_destination,
            Some(cancellation),
        )
        .await
    }

    async fn execute_original_enrollment_terminal(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: Arc<super::enrollment_trust::RetainedEnrollmentVmControl>,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: crate::runtime::services::enrollment_window::EnrollmentExecutionChild,
        retirement_destination: Arc<
            crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
        >,
        cancellation: Option<
            crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
        >,
    ) -> AgentResult<super::IssuerEnrollmentTerminalOutcome> {
        retained.require_runtime_owner(effects.as_ref())?;
        budget.require_issued_notice_owner(&retained)?;
        budget
            .bind_registered_notice_control(&retained)
            .map_err(AgentError::from)?;
        let mut slot = retirement_destination
            .borrow_original_vm(effects.as_ref(), &retained, &budget)
            .await?;
        let resume_cancelled =
            cancellation.is_some() && retirement_destination.cancelled.get().is_some();
        let outcome = if let Some(cancelled) = cancellation {
            // Required cleanup resumes only the original negative protocol.
            // It must never poll the positive request future again.
            Ok((None, Some(cancelled)))
        } else {
            let progress = Box::pin(self.run_device_enrollment_initiator_window(
                effects.clone(),
                &retained,
                &ceremony_runner,
                &budget,
                &mut slot,
                &retirement_destination,
            ));
            let cancellation = ceremony_runner.await_enrollment_cancellation(&retained);
            futures::pin_mut!(progress, cancellation);
            match futures::future::select(progress, cancellation).await {
                futures::future::Either::Left((Ok(()), _)) => retirement_destination
                    .take_original_terminal(&retained)
                    .map(|receipt| (Some(receipt), None))
                    .map_err(AgentError::from),
                // A response that arrives after a committed cancellation is
                // refused by the attempt; the cancellation still owns the
                // terminal path, so the signed notice must be sent.
                futures::future::Either::Left((Err(error), cancellation)) => {
                    let error = AgentError::from(
                        retirement_destination.retain_original_progress_failure(error)?,
                    );
                    #[cfg(all(test, not(target_arch = "wasm32")))]
                    retirement_destination
                        .await_progress_error_gate_for_test(&error)
                        .await;
                    let cancelled =
                        enrollment_cancelled(&ceremony_runner, &retained.manifest().ceremony).await;
                    if cancelled {
                        cancellation
                            .await
                            .map(|cancelled| (None, Some(cancelled)))
                            .map_err(AgentError::from)
                    } else {
                        Err(error)
                    }
                }
                futures::future::Either::Right((result, _)) => result
                    .map(|cancelled| (None, Some(cancelled)))
                    .map_err(AgentError::from),
            }
        };
        let closed = if resume_cancelled {
            // An interrupted negative send retains its original VM cursor.
            // The send helper resumes that slot before its usual close/retry.
            outcome
        } else {
            finish_enrollment_vm_slot(outcome, &mut slot).await
        };
        let outcome = match closed {
            Ok(outcome) => outcome,
            // A cancel waiting on this owner's settlement must observe the
            // failure rather than wait for a settlement that never comes.
            Err(error) => {
                if enrollment_cancelled(&ceremony_runner, &retained.manifest().ceremony).await {
                    let source = match error {
                        AgentError::Aura(source) => source,
                        source => aura_core::AuraError::Internal {
                            message: "original cancelled VM teardown failed".into(),
                            source: Some(Arc::new(source)),
                        },
                    };
                    retirement_destination.retain_original_retirement_failure(source.clone());
                    let settled = Err::<(), _>(source.clone());
                    super::enrollment_terminal_notice::publish_original_cancelled_settlement(
                        &ceremony_runner,
                        &retained.manifest().ceremony,
                        &settled,
                    )
                    .await;
                    return Err(AgentError::from(source));
                }
                return Err(error);
            }
        };
        let (receipt, cancelled) = outcome;
        if let Some(receipt) = receipt {
            return Ok(super::IssuerEnrollmentTerminalOutcome::Committed(receipt));
        }
        let Some(cancelled) = cancelled else {
            return Err(AgentError::invalid(
                "enrollment ended without original terminal custody",
            ));
        };
        retirement_destination.begin_original_terminal();
        let cancelled_terminal = if let Some(terminal) = retirement_destination.cancelled.get() {
            terminal.require_original_effects(&effects)?;
            if !std::ptr::eq(terminal.issued(), retained.as_ref()) {
                return Err(AgentError::invalid(
                    "cancelled terminal has another original control",
                ));
            }
            Arc::clone(terminal)
        } else {
            retirement_destination.retain_cancelled_terminal(
                super::OriginalIssuerCancellationTerminal {
                    cancellation: cancelled,
                    issued: retained.clone(),
                    failure: enrollment_vm_admission::terminal_failure_cause(
                        aura_app::runtime_bridge::CeremonyFailureReason::Cancelled,
                    ),
                },
            )?
        };
        #[cfg(test)]
        retirement_destination
            .await_cancelled_notice_gate_for_test()
            .await;
        // Sign while the cancelled generation is held, then release it so a
        // new enrollment is not refused while an unanswered notice retries.
        // The cancel call awaits this settlement before it returns.
        let settled = async {
            let notice = if let Some(notice) =
                retirement_destination.original_cancelled_notice(&cancelled_terminal)?
            {
                notice
            } else {
                let notice = super::enrollment_terminal_notice::sign_cancelled_notice(
                    &effects,
                    &retained,
                    &ceremony_runner,
                    &cancelled_terminal.cancellation,
                    &budget,
                )
                .await?;
                retirement_destination.retain_original_cancelled_notice(notice)?
            };
            budget
                .retire_original_cancelled_generation(
                    &effects,
                    &cancelled_terminal,
                    &retirement_destination,
                    &ceremony_runner,
                )
                .await?;
            Ok::<_, AgentError>((notice, cancelled_terminal))
        }
        .await
        .map_err(|source| {
            let source = match source {
                AgentError::Aura(source) => source,
                source => aura_core::AuraError::Internal {
                    message: "original cancelled enrollment settlement failed".into(),
                    source: Some(Arc::new(source)),
                },
            };
            retirement_destination.retain_original_retirement_failure(source.clone());
            source
        });
        super::enrollment_terminal_notice::publish_original_cancelled_settlement(
            &ceremony_runner,
            &retained.manifest().ceremony,
            &settled,
        )
        .await;
        let (notice, cancelled_terminal) = settled.map_err(AgentError::from)?;
        let negative_window = budget
            .original_cancelled_notice_window(
                effects.as_ref(),
                &cancelled_terminal,
                &notice,
                retirement_destination.as_ref(),
            )
            .map_err(AgentError::from)?;
        loop {
            let attempt = negative_window
                .execute(effects.as_ref(), || {
                    Box::pin(super::enrollment_terminal_notice::send_cancelled_notice(
                        effects.clone(),
                        &cancelled_terminal,
                        &notice,
                        &budget,
                        &mut slot,
                        &retirement_destination,
                    ))
                })
                .await
                .map_err(|source| {
                    negative_window.map_run_error("signed enrollment cancellation notice", source)
                });
            match finish_original_enrollment_vm_slot(
                attempt,
                &mut slot,
                &retirement_destination,
                effects.as_ref(),
                &retained,
                &budget,
            )
            .await
            {
                Ok(()) => {
                    return Ok(super::IssuerEnrollmentTerminalOutcome::Cancelled(
                        cancelled_terminal,
                    ));
                }
                Err(error)
                    if enrollment_notice_peer_unreachable(&error, retained.manifest().subject) =>
                {
                    negative_window
                        .retry_delay(effects.as_ref(), DEVICE_ENROLLMENT_RETRY_DELAY_MS)
                        .await?;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_device_enrollment_initiator_window(
        &self,
        effects: Arc<AuraEffectSystem>,
        retained: &Arc<super::enrollment_trust::RetainedEnrollmentVmControl>,
        ceremony_runner: &crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: &EnrollmentExecutionChild,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
        destination: &crate::runtime::services::issuer_terminal_destination::OriginalIssuerTerminalDestination,
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
                        destination,
                    ))
                })
                .await
                .map_err(|source| {
                    attempt_budget.map_run_error("device enrollment initiator attempt", source)
                });
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if let Err(error) = &attempt {
                // Observe the actual operation error before close can suspend.
                // This test hook retains no production error or cleanup proof.
                destination.await_progress_error_gate_for_test(error).await;
            }
            let attempt = finish_original_enrollment_vm_slot(
                attempt,
                session_slot,
                destination,
                effects.as_ref(),
                retained,
                budget,
            )
            .await;
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
        retained: &Arc<super::enrollment_trust::RetainedEnrollmentVmControl>,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: &EnrollmentExecutionChild,
        session_slot: &mut Option<crate::runtime::session_ingress::OwnedVmSession>,
        destination: &crate::runtime::services::issuer_terminal_destination::OriginalIssuerTerminalDestination,
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

            #[cfg(all(test, not(target_arch = "wasm32")))]
            if let Some(fault) =
                original_enrollment_vm_read_fault_for_test(effects.as_ref(), session.owner())
            {
                fault.await;
            }
            let loop_result = budget
                .execute(effects.as_ref(), || Box::pin(async {
                    let mut terminal_ready = false;
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
                            destination.begin_original_terminal();
                            match verified {
                                super::enrollment_trust::VerifiedEnrollmentResponseDispositionCapability::Accepted(verified) => {
                                    ceremony_runner.record_verified_enrollment_response(*verified).await.map_err(AgentError::from)?;
                                }
                                super::enrollment_trust::VerifiedEnrollmentResponseDispositionCapability::Refused(verified) => {
                                    #[cfg(test)]
                                    eprintln!("enrollment rejection stage: record verified original refusal");
                                    ceremony_runner.record_verified_enrollment_rejection(verified).await.map_err(AgentError::from)?;
                                    #[cfg(test)]
                                    eprintln!("enrollment rejection stage: durable refusal recorded");
                                }
                            }
                            #[cfg(test)]
                            eprintln!("enrollment rejection stage: begin actual terminal receipt");
                            let confirm = enrollment_vm_admission::sign_terminal_receipt(
                                effects.clone(),
                                retained.clone(),
                                budget,
                            )
                            .await;
                            #[cfg(test)]
                            eprintln!("enrollment rejection stage: terminal receipt result {:?}", confirm.as_ref().map(|_| ()));
                            let confirm = confirm?;
                            let encoded = to_vec(confirm.frame()).map_err(|error| AgentError::Aura(error.into()));
                            destination.stage_original_terminal(confirm, effects.as_ref())?;
                            let encoded = encoded?;
                            budget
                                .remaining_ms(effects.as_ref())
                                .await
                                .map_err(|source| {
                                    budget.map_run_error(
                                        "device enrollment acceptance send",
                                        TimeoutRunError::Timeout(source),
                                    )
                                })?;
                            session.queue_send_bytes(encoded);
                            terminal_ready = true;
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
                            break if terminal_ready { Ok(()) } else { Err(enrollment_vm_admission::failure(enrollment_vm_admission::EnrollmentVmAdmissionError::MissingTerminalReceipt)) };
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
                            break if terminal_ready { Ok(()) } else { Err(enrollment_vm_admission::failure(enrollment_vm_admission::EnrollmentVmAdmissionError::MissingTerminalReceipt)) };
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
    #[cfg(test)]
    pub(super) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        root: &mut crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
    ) -> AgentResult<()> {
        let mut custody = OriginalInviteeTerminalCustody::new();
        self.execute_device_enrollment_invitee_with_custody(effects, root, &mut custody)
            .await
    }

    pub(super) async fn execute_device_enrollment_invitee_with_custody(
        &self,
        effects: Arc<AuraEffectSystem>,
        root: &mut crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
        custody: &mut OriginalInviteeTerminalCustody,
    ) -> AgentResult<()> {
        match self
            .execute_device_enrollment_invitee_response(
                effects,
                root,
                EnrollmentInviteeChoice::Accept,
                custody,
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
        root: &mut crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
    ) -> AgentResult<()> {
        let mut custody = OriginalInviteeTerminalCustody::new();
        match self
            .execute_device_enrollment_invitee_response(
                effects,
                root,
                EnrollmentInviteeChoice::Refuse,
                &mut custody,
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
        root: &mut crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
        choice: EnrollmentInviteeChoice,
        custody: &mut OriginalInviteeTerminalCustody,
    ) -> AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>> {
        if custody.terminal.is_some() {
            return self
                .settle_original_invitee_terminal(effects.as_ref(), root, custody)
                .await;
        }
        let admitted = root.origin_observation();
        let budget = root.child();
        if custody.notice.group.is_none() {
            let notice_group = root.children().group(format!(
                "enrollment_terminal_notice.{}",
                admitted.manifest().invitation
            ));
            let (stop_tx, stop) = futures::channel::oneshot::channel::<()>();
            let (completed_tx, completed) = futures::channel::oneshot::channel();
            let notice_effects = effects.clone();
            let notice_admitted = admitted.clone();
            let notice_budget = Arc::new(root.owned_execution_child());
            let notice_state = custody.notice.task_state.clone();
            let candidate_observed = notice_state.lock().await.vm.candidate_observation();
            #[cfg(test)]
            let notice_gate = Arc::clone(&self.invitation_cache);
            let notice_stop = Arc::new(tokio::sync::Mutex::new(stop));
            let callback = Box::new(move || {
                let notice_budget = Arc::clone(&notice_budget);
                let notice_effects = Arc::clone(&notice_effects);
                let notice_admitted = Arc::clone(&notice_admitted);
                let notice_state = notice_state.clone();
                let notice_stop = Arc::clone(&notice_stop);
                #[cfg(test)]
                let notice_gate = Arc::clone(&notice_gate);
                Box::pin(async move {
                    let mut stop_owner = notice_stop.lock().await;
                    let stop = &mut *stop_owner;
                    let result = {
                        let mut retained = notice_state.lock().await;
                        let candidate_observed = retained.vm.candidate_observation();
                        let result = {
                            let receive = notice_budget.execute(notice_effects.as_ref(), || {
                            Box::pin(
                                super::enrollment_terminal_notice::receive_cancelled_notice_retained(
                                    notice_effects.clone(),
                                    notice_admitted.as_ref(),
                                    &notice_budget,
                                    &mut retained.vm,
                                    #[cfg(test)]
                                    notice_gate.as_ref(),
                                ),
                            )
                        });
                            futures::pin_mut!(receive, stop);
                            if candidate_observed.get().is_some() {
                                receive.await.map(Some).map_err(|source| {
                                    Arc::new(
                                        notice_budget
                                            .map_run_error("enrollment terminal notice", source),
                                    )
                                })
                            } else {
                                match futures::future::select(receive, stop).await {
                                    futures::future::Either::Left((result, _)) => {
                                        result.map(Some).map_err(|source| {
                                            Arc::new(notice_budget.map_run_error(
                                                "enrollment terminal notice",
                                                source,
                                            ))
                                        })
                                    }
                                    futures::future::Either::Right((_, receive)) => {
                                        if candidate_observed.get().is_some() {
                                            // Preserve this SAME pinned receive, selected window
                                            // and VM through postchecks after signed capture.
                                            receive.await.map(Some).map_err(|source| {
                                                Arc::new(notice_budget.map_run_error(
                                                    "enrollment terminal notice",
                                                    source,
                                                ))
                                            })
                                        } else {
                                            Ok(None)
                                        }
                                    }
                                }
                            }
                        };
                        match &result {
                            Ok(Some(())) => {
                                retained.vm.eligible = true;
                                #[cfg(test)]
                                {
                                    use crate::runtime::services::invitation_manager::OriginalInviteeStage;
                                    let invitation =
                                        &notice_admitted.canonical_invitation().invitation_id;
                                    if notice_gate.original_invitee_stage_matches_for_test(
                                        invitation,
                                        OriginalInviteeStage::EligibleNoticeBeforeCloseWithCloseFault,
                                    ) {
                                        notice_gate.await_original_invitee_stage_gate_for_test(
                                            invitation,
                                            OriginalInviteeStage::EligibleNoticeBeforeCloseWithCloseFault,
                                        ).await;
                                        let original = retained.vm.session.as_ref()
                                            .expect("eligible signed notice retains its original VM");
                                        notice_effects.fail_original_owned_session_end_once_for_test(original.owner());
                                    } else {
                                        notice_gate.await_original_invitee_stage_gate_for_test(
                                            invitation,
                                            OriginalInviteeStage::EligibleNoticeBeforeClose,
                                        ).await;
                                    }
                                }
                            }
                            Err(source) => retained.failure = Some(Arc::clone(source)),
                            Ok(None) => {}
                        }
                        result
                    };
                    let mut retained = notice_state.lock().await;
                    match result {
                        Ok(Some(())) => {}
                        Ok(None) => {}
                        Err(source) => retained.failure = Some(source),
                    }
                    if retained.vm.session.is_some() {
                        if let Err(source) = retained.vm.close_retained().await {
                            let close = Arc::new(enrollment_stage_error(
                                "close original terminal notice VM",
                                source,
                            ));
                            retained.failure = Some(match retained.failure.take() {
                                Some(primary) => {
                                    Arc::new(AgentError::Aura(aura_core::AuraError::Internal {
                                        message: "original terminal notice and VM close failed"
                                            .into(),
                                        source: Some(Arc::new(RetainedOriginalCleanupFailure {
                                            primary,
                                            close,
                                        })),
                                    }))
                                }
                                None => close,
                            });
                        } else {
                            debug_assert!(retained.vm.session.is_none());
                        }
                    }
                    let failure = retained.failure.as_ref().cloned();
                    drop(retained);
                    // Required cleanup publishes the disposition only after the
                    // original registered child has acknowledged its VM close.
                    match failure {
                        None => Ok(()),
                        Some(source) => {
                            tracing::error!(
                                cause_categories = ?terminal_notice_failure_categories(source.as_ref()),
                                "enrollment terminal notice listener failed"
                            );
                            Err(aura_core::AuraError::Internal {
                                message: "enrollment terminal notice listener failed".into(),
                                source: Some(source),
                            })
                        }
                    }
                }) as OriginalNoticeExecution
            });
            let obligation = OriginalNoticeTaskCustody {
                execution: Some(callback),
                state: custody.notice.task_state.clone(),
                candidate_observed,
                completion: Some(completed_tx),
            };
            cfg_if::cfg_if! {
                if #[cfg(target_arch = "wasm32")] {
                    let control = notice_group.spawn_original_local_required_custody(
                        "receive_terminal_notice".into(), Box::new(obligation), None,
                    ).map_err(|rejected| crate::handlers::invitation_service::retain_rejected_local_issuer(rejected.into_local()))?;
                } else {
                    let control = notice_group.spawn_original_required_custody(
                        "receive_terminal_notice".into(), Box::new(obligation), None,
                    ).map_err(|rejected| crate::handlers::invitation_service::retain_rejected_native_issuer(rejected.into_native()))?;
                }
            }
            custody.notice.control = Some(control);
            custody.notice.group = Some(notice_group);
            custody.notice.stop = Some(stop_tx);
            custody.notice.completed = Some(completed);
        }
        enum Selected {
            Response(AgentResult<()>),
            Notice(Result<RetainedNoticeResult, futures::channel::oneshot::Canceled>),
        }
        if custody.vm.response_outcome.is_none()
            && !custody.notice.observed_candidate
            && custody.notice.failure.is_none()
        {
            let selected = {
                let vm = &mut custody.vm;
                let completed = custody.notice.completed.as_mut().ok_or_else(|| {
                    AgentError::internal("original terminal notice receiver was not retained")
                })?;
                let response = Box::pin(self.run_device_enrollment_invitee_window(
                    effects.clone(),
                    admitted.as_ref(),
                    choice,
                    budget,
                    vm,
                ));
                futures::pin_mut!(response);
                match futures::future::select(response, completed).await {
                    futures::future::Either::Left((result, _)) => Selected::Response(result),
                    futures::future::Either::Right((result, _)) => Selected::Notice(result),
                }
            };
            match selected {
                Selected::Response(result) => {
                    custody.vm.response_outcome = Some(result.map_err(Arc::new));
                    if let Some(stop) = custody.notice.stop.take() {
                        let _ = stop.send(());
                    }
                }
                Selected::Notice(result) => {
                    let result = result.map_err(|source| {
                        enrollment_stage_error("observe terminal notice owner", source)
                    })?;
                    match result {
                        Ok(true) => custody.notice.observed_candidate = true,
                        Ok(false) => custody.notice.observed_empty = true,
                        Err(source) => custody.notice.failure = Some(source),
                    }
                }
            }
        }
        if !custody.notice.observed_candidate
            && custody.notice.failure.is_none()
            && !custody.notice.observed_empty
        {
            custody.notice.reconcile_original_task().await?;
            let result = custody
                .notice
                .completed
                .as_mut()
                .ok_or_else(|| {
                    AgentError::internal("original terminal notice receiver was not retained")
                })?
                .await
                .map_err(|source| enrollment_stage_error("drain terminal notice owner", source))?;
            match result {
                Ok(true) => custody.notice.observed_candidate = true,
                Ok(false) => custody.notice.observed_empty = true,
                Err(source) => custody.notice.failure = Some(source),
            }
        }
        let group_result = custody.notice.drain_original_group().await;
        let signed_notice_disposed = custody.notice.retain_signed_disposal().await?;
        if !signed_notice_disposed {
            if let Some(source) = custody.notice.failure.as_ref() {
                let source: Arc<dyn std::error::Error + Send + Sync> = match group_result {
                    Ok(()) => Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>,
                    Err(group) => Arc::new(RetainedOriginalCleanupFailure {
                        primary: Arc::clone(source),
                        close: Arc::new(group),
                    }),
                };
                let vm_close = custody.vm.close_retained().await;
                let source: Arc<dyn std::error::Error + Send + Sync> = match vm_close {
                    Ok(()) => source,
                    Err(close) => Arc::new(RetainedOriginalCleanupFailure {
                        primary: Arc::new(AgentError::Aura(aura_core::AuraError::Internal {
                            message: "original terminal notice listener failed".into(),
                            source: Some(source),
                        })),
                        close: Arc::new(close),
                    }),
                };
                return Err(AgentError::Aura(aura_core::AuraError::Internal {
                    message: "enrollment terminal notice listener failed".into(),
                    source: Some(source),
                }));
            }
            if let Err(primary) = group_result {
                let close = custody.vm.close_retained().await;
                return match close {
                    Ok(()) => Err(primary),
                    Err(close) => Err(AgentError::Aura(aura_core::AuraError::Internal {
                        message: "original notice group and invitee VM close failed".into(),
                        source: Some(Arc::new(RetainedOriginalCleanupFailure {
                            primary: Arc::new(primary),
                            close: Arc::new(close),
                        })),
                    })),
                };
            }
        } else if let Err(source) = group_result {
            custody
                .notice
                .failure
                .get_or_insert_with(|| Arc::new(source));
        }
        if custody.notice.observed_candidate {
            if custody.vm.candidate.is_some() || custody.vm.eligible.is_some() {
                return Err(AgentError::internal("conflicting original terminal owners"));
            }
            custody.vm.close_retained().await?;
            let mut task_state = custody.notice.task_state.lock().await;
            if task_state.vm.session.is_some() {
                return Err(AgentError::internal(
                    "original terminal notice VM close remains unacknowledged",
                ));
            }
            let evidence = task_state.vm.candidate.take().ok_or_else(|| {
                AgentError::internal("original terminal notice candidate was not retained")
            })?;
            drop(task_state);
            custody.terminal = Some(enrollment_vm_admission::VerifiedEnrollmentTerminal::Failed(
                evidence,
            ));
        } else {
            if let Some(Err(source)) = custody.vm.response_outcome.as_ref() {
                let primary = Arc::clone(source);
                let vm_close = custody.vm.close_retained().await;
                // The original VM owner has acknowledged close. Its
                // undelivered blocked receive can no longer be injected;
                // retire it under the same custodian, without treating
                // the signed candidate as an eligible terminal. A failed
                // close leaves the exact frame retained for reconciliation.
                custody
                    .vm
                    .retire_failed_response_receive_after_close(vm_close.is_ok());
                let source: Arc<dyn std::error::Error + Send + Sync> = match vm_close {
                    Ok(()) => primary,
                    Err(close) => Arc::new(RetainedOriginalCleanupFailure {
                        primary,
                        close: Arc::new(close),
                    }),
                };
                return Err(AgentError::Aura(aura_core::AuraError::Internal {
                    message: "original invitee VM response failed".into(),
                    source: Some(source),
                }));
            }
            custody.vm.close_retained().await?;
            if !matches!(custody.vm.response_outcome.as_ref(), Some(Ok(()))) {
                return Err(AgentError::internal(
                    "original invitee response was not retained",
                ));
            }
            custody.terminal = Some(custody.vm.eligible.take().ok_or_else(|| {
                AgentError::internal("original invitee VM has no eligible terminal")
            })?);
        }

        self.settle_original_invitee_terminal(effects.as_ref(), root, custody)
            .await
    }

    async fn settle_original_invitee_terminal(
        &self,
        effects: &AuraEffectSystem,
        root: &mut crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        >,
        custody: &mut OriginalInviteeTerminalCustody,
    ) -> AgentResult<Option<aura_app::runtime_bridge::CeremonyFailureReason>> {
        let admitted = root.origin_observation();
        let terminal = custody
            .terminal
            .as_ref()
            .ok_or_else(|| AgentError::internal("original invitee terminal was not retained"))?;
        if custody.acknowledged.is_none() {
            custody.acknowledged = Some(
                match terminal {
                    enrollment_vm_admission::VerifiedEnrollmentTerminal::Committed(proof) => {
                        root.acknowledge_confirmation(effects, proof).await
                    }
                    enrollment_vm_admission::VerifiedEnrollmentTerminal::Failed(proof) => {
                        match custody.notice.disposal.as_ref() {
                            Some(disposal) => {
                                root.acknowledge_original_notice_failure(effects, proof, disposal)
                                    .await
                            }
                            None => root.acknowledge_failure(effects, proof).await,
                        }
                    }
                }
                .map_err(AgentError::from)?,
            );
        }
        let acknowledged = custody.acknowledged.as_ref().ok_or_else(|| {
            AgentError::internal("original invitee acknowledgment was not retained")
        })?;
        let outcome = match custody
            .terminal
            .as_ref()
            .ok_or_else(|| AgentError::internal("original invitee terminal was not retained"))?
        {
            enrollment_vm_admission::VerifiedEnrollmentTerminal::Committed(confirmed) => {
                let witness = super::enrollment_manifest_admission::retain_verified_confirmation(
                    effects,
                    confirmed,
                    acknowledged,
                )
                .await
                .map_err(AgentError::EnrollmentManifest)?;
                custody.retained = Some(witness);
                None
            }
            enrollment_vm_admission::VerifiedEnrollmentTerminal::Failed(failed) => {
                let witness = super::enrollment_manifest_admission::retain_verified_failure(
                    effects,
                    failed,
                    acknowledged,
                )
                .await
                .map_err(AgentError::EnrollmentManifest)?;
                custody.retained = Some(witness);
                Some(failed.reason())
            }
        };
        crate::core::agent::pending_enrollment_mailbox::verify_retained_mailbox_frames_after_terminal(
            effects, admitted.as_ref(),
        ).await?;
        custody.frames_verified = true;
        if let Some(source) = custody.notice.failure.as_ref() {
            return Err(AgentError::Aura(aura_core::AuraError::Internal {
                message: "original signed notice retained its cleanup failure".into(),
                source: Some(Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>),
            }));
        }
        Ok(outcome)
    }

    async fn run_device_enrollment_invitee_window(
        &self,
        effects: Arc<AuraEffectSystem>,
        admitted: &super::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        choice: EnrollmentInviteeChoice,
        budget: &EnrollmentExecutionChild,
        vm: &mut RetainedInviteeVm,
    ) -> AgentResult<()> {
        if let Some(source) = vm.postcheck_failure.as_ref() {
            return Err(AgentError::Aura(aura_core::AuraError::Internal {
                message: "original signed candidate failed selected postchecks".into(),
                source: Some(Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>),
            }));
        }
        loop {
            if vm.eligible.is_some() {
                vm.close_retained().await?;
                return Ok(());
            }
            let attempt_budget = if let Some(existing) = vm.attempt.as_ref() {
                Arc::clone(existing)
            } else {
                let child = Arc::new(
                    budget
                        .child(
                            effects.as_ref(),
                            std::time::Duration::from_millis(INVITATION_VM_LOOP_TIMEOUT_MS),
                        )
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "device enrollment window",
                                TimeoutRunError::Timeout(source),
                            )
                        })?,
                );
                vm.attempt = Some(Arc::clone(&child));
                child
            };

            let attempt = attempt_budget
                .execute(effects.as_ref(), || {
                    Box::pin(self.run_device_enrollment_invitee_attempt(
                        effects.clone(),
                        admitted,
                        &attempt_budget,
                        choice,
                        vm,
                    ))
                })
                .await
                .map_err(|source| {
                    attempt_budget.map_run_error("device enrollment invitee attempt", source)
                });
            if vm.vm_completed {
                if let Err(source) = attempt {
                    vm.postcheck_failure = Some(Arc::new(source));
                    return Err(AgentError::Aura(aura_core::AuraError::Internal {
                        message: "original signed candidate failed selected postchecks".into(),
                        source: vm.postcheck_failure.as_ref().map(|source| {
                            Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>
                        }),
                    }));
                }
                vm.eligible = Some(vm.candidate.take().ok_or_else(|| {
                    AgentError::internal("completed original VM lacks signed candidate")
                })?);
                #[cfg(test)]
                self.invitation_cache
                    .await_original_invitee_stage_gate_for_test(
                        &admitted.canonical_invitation().invitation_id,
                        crate::runtime::services::invitation_manager::OriginalInviteeStage::EligibleResponseBeforeClose,
                    )
                    .await;
                vm.close_retained().await?;
                return Ok(());
            }
            if vm.candidate.is_some() {
                // A post-observation failure leaves the original candidate
                // and exact attempt endpoint in custody, but it never turns a
                // typed selected-time refusal into a later positive ACK.
                if let Err(source) = attempt {
                    vm.postcheck_failure = Some(Arc::new(source));
                    return Err(AgentError::Aura(aura_core::AuraError::Internal {
                        message: "original signed candidate failed selected postchecks".into(),
                        source: vm.postcheck_failure.as_ref().map(|source| {
                            Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>
                        }),
                    }));
                }
                return Err(AgentError::internal(
                    "candidate did not finish its original VM",
                ));
            }
            let closed = vm.close_retained().await;
            match finish_enrollment_vm_attempt(attempt, closed) {
                Ok(()) => return Ok(()),
                Err(error) if error.is_timeout() && !enrollment_attempt_failed_to_close(&error) => {
                    vm.attempt = None;
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
        budget: &EnrollmentExecutionChild,
        choice: EnrollmentInviteeChoice,
        vm: &mut RetainedInviteeVm,
    ) -> AgentResult<()> {
        if let Some(source) = vm.injection_failure.as_ref() {
            return Err(AgentError::Aura(aura_core::AuraError::Internal {
                message: "original blocked receive injection failed".into(),
                source: Some(Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>),
            }));
        }
        if vm.eligible.is_some() {
            return Ok(());
        }
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

        if vm.session.is_none() {
            vm.session = Some(
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
        }

        let loop_result = budget
            .execute(effects.as_ref(), || Box::pin(async {
                loop {
                    if vm.vm_completed {
                        break Ok(());
                    }
                    if vm.pending_receive.is_some() {
                        if vm.candidate.is_some() {
                            budget.remaining_ms(effects.as_ref()).await.map_err(|source| {
                                budget.map_run_error(
                                    "device enrollment confirmation publication",
                                    TimeoutRunError::Timeout(source),
                                )
                            })?;
                        } else {
                            let request = vm.request.as_ref().ok_or_else(|| {
                                AgentError::internal("original invitee request was not retained")
                            })?;
                            if vm.response_bytes.is_none() {
                                let response = DeviceEnrollmentResponseWrapper(match choice {
                                    EnrollmentInviteeChoice::Accept => DeviceEnrollmentResponse::Accepted(enrollment_vm_admission::sign_acceptance_for_request(effects.as_ref(), admitted, request).await?),
                                    EnrollmentInviteeChoice::Refuse => DeviceEnrollmentResponse::Refused(enrollment_vm_admission::sign_refusal_for_request(effects.as_ref(), admitted, request).await?),
                                });
                                vm.response_bytes = Some(to_vec(&response).map_err(|source| AgentError::Aura(source.into()))?);
                            }
                            if !vm.response_queued {
                                vm.session.as_mut().ok_or_else(|| AgentError::internal("original invitee VM session was not retained"))?
                                    .queue_send_bytes(vm.response_bytes.as_ref().ok_or_else(|| AgentError::internal("original invitee response bytes missing"))?.clone());
                                vm.response_queued = true;
                            }
                        }
                        let blocked = vm.pending_receive.take().ok_or_else(|| AgentError::internal("original invitee blocked receive was not retained"))?;
                        if let Err(source) = vm.session.as_mut().ok_or_else(|| AgentError::internal("original invitee VM session was not retained"))?
                            .inject_blocked_receive(blocked) {
                            let source = Arc::new(AgentError::Aura(aura_core::AuraError::Internal {
                                message: "device enrollment VM stage failed".into(),
                                source: Some(Arc::new(source)),
                            }));
                            vm.injection_failure = Some(Arc::clone(&source));
                            return Err(AgentError::Aura(aura_core::AuraError::Internal {
                                message: "original blocked receive injection failed".into(),
                                source: Some(source),
                            }));
                        }
                        vm.receive_injected = true;
                        continue;
                    }
                    budget
                        .remaining_ms(effects.as_ref())
                        .await
                        .map_err(|source| {
                            budget.map_run_error(
                                "device enrollment VM progression",
                                TimeoutRunError::Timeout(source),
                            )
                        })?;
                    let round = vm.session.as_mut().ok_or_else(|| AgentError::internal("original invitee VM session was not retained"))?
                        .advance_round("Invitee", &peer_roles)
                        .await
                        .map_err(|error| {
                            AgentError::Aura(aura_core::AuraError::Internal {
                                message: "device enrollment VM stage failed".into(),
                                source: Some(std::sync::Arc::new(error)),
                            })
                        })?;

                    if let Some(blocked) = round.blocked_receive {
                        if let Some(request) = vm.request.as_ref() {
                            let confirmation = EnrollmentControlFrame::decode(&blocked.payload)?;
                            let evidence = confirmation
                                .verify_terminal(effects.as_ref(), admitted, request)
                                .await?;
                            vm.candidate = Some(evidence);
                        } else {
                            let frame = EnrollmentControlFrame::decode(&blocked.payload)?;
                            let request = frame.verify_request(effects.as_ref(), admitted).await?;
                            request.validate_against(&expected_request)?;
                            vm.request = Some(request);
                        }
                        // The non-Clone receive moves into the original task
                        // custodian before the selected-time/signing await.
                        vm.pending_receive = Some(blocked);
                        #[cfg(test)]
                        if vm.request.is_some() && vm.candidate.is_none() {
                            self.invitation_cache
                                .await_original_invitee_stage_gate_for_test(
                                    &admitted.canonical_invitation().invitation_id,
                                    crate::runtime::services::invitation_manager::OriginalInviteeStage::VerifiedRequestBeforeResponse,
                                )
                                .await;
                            let selected_close_fault = self.invitation_cache
                                .original_invitee_stage_matches_for_test(
                                    &admitted.canonical_invitation().invitation_id,
                                    crate::runtime::services::invitation_manager::OriginalInviteeStage::VerifiedRequestBeforeResponseWithCloseFault,
                                );
                            #[cfg(not(target_arch = "wasm32"))]
                            if selected_close_fault {
                                let session = vm.session.as_ref().expect("original session retained");
                                effects.fail_original_owned_session_end_once_for_test(session.owner());
                            }
                            let _ = self.invitation_cache
                                .await_original_invitee_stage_gate_for_test(
                                    &admitted.canonical_invitation().invitation_id,
                                    crate::runtime::services::invitation_manager::OriginalInviteeStage::VerifiedRequestBeforeResponseWithCloseFault,
                                )
                                .await;
                            #[cfg(target_arch = "wasm32")]
                            let _ = selected_close_fault;
                        }
                        #[cfg(test)]
                        if vm.candidate.is_some() {
                            self.invitation_cache
                                .await_original_invitee_stage_gate_for_test(
                                    &admitted.canonical_invitation().invitation_id,
                                    crate::runtime::services::invitation_manager::OriginalInviteeStage::SignedResponseCandidate,
                                )
                                .await;
                        }
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
                        if vm.candidate.is_none() {
                            return Err(aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished.into());
                        }
                        vm.vm_completed = true;
                        break Ok(());
                    }

                    if handle_invitation_vm_step(
                        round.step,
                        "device enrollment invitee VM became stuck without a pending receive",
                    )? {
                        if vm.candidate.is_none() {
                            return Err(
                            aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished
                                .into(),
                        );
                        }
                        vm.vm_completed = true;
                        break Ok(());
                    }
                }
            }))
            .await
            .map_err(|error| budget.map_run_error("device enrollment invitee VM", error));

        loop_result?;
        let terminal = vm.candidate.as_ref().ok_or_else(|| {
            AgentError::internal("original invitee VM completed without signed candidate")
        })?;
        if matches!(
            terminal,
            enrollment_vm_admission::VerifiedEnrollmentTerminal::Committed(_)
        ) && choice == EnrollmentInviteeChoice::Refuse
        {
            return Err(
                aura_invitation::protocol::DeviceEnrollmentMessageError::NotEstablished.into(),
            );
        }
        // Keep the original verified domain evidence move-owned until the VM
        // slot and root child subtree have actually acknowledged disposal.
        Ok(())
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

//! Negative terminal notification has its own finite protocol. It never injects
//! a confirmation into the request/response protocol's current program counter.
use super::enrollment_manifest_admission::AdmittedEnrollmentManifest;
use super::enrollment_trust::RetainedEnrollmentVmControl;
use super::enrollment_vm_admission::{
    EnrollmentControlFrame, EnrollmentVmAdmissionError, VerifiedEnrollmentFailureCapability,
};
use super::*;
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::enrollment_window::EnrollmentExecutionChild;
use crate::runtime::session_ingress::OwnedVmSession;
use aura_core::TimeoutBudgetError;
use aura_invitation::protocol::DeviceEnrollmentTerminalNotice;
use std::collections::BTreeMap;

fn notice_session_id(invitation: &InvitationId, digest: &[u8; 32]) -> Uuid {
    aura_invitation::enrollment_mailbox_session::original_terminal_notice_session_id(
        invitation, digest,
    )
}

/// Actual finite-VM send completion, not a peer adoption or cleanup receipt.
/// Only the original sender below constructs this move-owned witness.
pub(crate) struct OriginalCancelledNoticeSendCompletion {
    owner: crate::runtime::RuntimeSessionOwner,
    invitation: InvitationId,
    manifest_digest: [u8; 32],
    notice_digest: [u8; 32],
}
impl OriginalCancelledNoticeSendCompletion {
    pub(crate) fn require_original_binding(
        &self,
        terminal: &OriginalIssuerCancellationTerminal,
        notice: &SignedCancelledNotice,
    ) -> AgentResult<()> {
        let issued = terminal.issued();
        if self.invitation != issued.manifest().invitation
            || self.manifest_digest != issued.digest()
            || self.notice_digest != aura_core::hash::hash(&notice.bytes)
            || self.owner.session_id.as_uuid()
                != notice_session_id(&issued.manifest().invitation, &issued.digest())
        {
            return Err(AgentError::invalid(
                "notice send completion has another original binding",
            ));
        }
        Ok(())
    }
    pub(crate) fn require_live_original(
        &self,
        effects: &Arc<AuraEffectSystem>,
        terminal: &OriginalIssuerCancellationTerminal,
        notice: &SignedCancelledNotice,
    ) -> AgentResult<()> {
        self.require_original_binding(terminal, notice)?;
        terminal.require_original_effects(effects)?;
        terminal.issued().require_runtime_owner(effects.as_ref())?;
        effects
            .assert_owned_choreography_session(&self.owner)
            .map_err(stage)
    }
}

fn stage(error: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::from(aura_core::AuraError::Internal {
        message: "owned enrollment terminal notification".into(),
        source: Some(Arc::new(error)),
    })
}

/// Only malformed incoming wire, binding mismatch and invalid signatures are
/// discardable. Required clock, storage, signer and membership failures retain
/// their original cause and terminate the owned listener.
fn discard_unverified_notice(error: &AgentError) -> bool {
    use std::error::Error;
    let mut source: Option<&(dyn Error + 'static)> = Some(error);
    while let Some(cause) = source {
        if let Some(cause) = cause.downcast_ref::<EnrollmentVmAdmissionError>() {
            return matches!(
                cause,
                EnrollmentVmAdmissionError::Binding | EnrollmentVmAdmissionError::Signature
            );
        }
        source = cause.source();
    }
    false
}

fn pending_terminal_receive(status: AuraVmHostWaitStatus, step: &StepResult) -> bool {
    matches!(status, AuraVmHostWaitStatus::Deferred) && matches!(step, StepResult::Stuck)
}

/// Each variant retains its protocol-specific owner; recovery has no active
/// request/response admission authority.
enum IssuedNoticeWindowCapability<'a> {
    OriginalCancelled(
        &'a crate::runtime::services::enrollment_window::OriginalCancelledNoticeWindowCapability<
            'a,
        >,
    ),
    Active(&'a EnrollmentExecutionChild),
    Cancelled(
        &'a crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability,
    ),
}
impl IssuedNoticeWindowCapability<'_> {
    fn require_owner(
        &self,
        issued: &RetainedEnrollmentVmControl,
        effects: &AuraEffectSystem,
    ) -> AgentResult<()> {
        match self {
            Self::OriginalCancelled(window) => window
                .require_owner(issued, effects)
                .map_err(AgentError::from),
            Self::Active(window) => window
                .require_issued_notice_owner(issued)
                .map_err(AgentError::from),
            Self::Cancelled(window) => window
                .require_issued_owner(issued, effects)
                .map_err(AgentError::from),
        }
    }
    async fn remaining_ms(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<u64, aura_core::TimeoutBudgetError> {
        match self {
            Self::OriginalCancelled(window) => window.remaining_ms(effects).await,
            Self::Active(window) => window.remaining_ms(effects).await,
            Self::Cancelled(window) => window.remaining_ms(effects).await,
        }
    }
    fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<AgentError>,
    ) -> AgentError {
        match self {
            Self::OriginalCancelled(window) => window.map_run_error(stage, source),
            Self::Active(window) => window.map_run_error(stage, source),
            Self::Cancelled(window) => window.map_run_error(stage, source),
        }
    }
}

#[derive(Clone, Copy)]
enum AdmittedNoticeWindowCapability<'a> {
    Active(&'a EnrollmentExecutionChild),
    Recovered(
        &'a crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability,
    ),
}
impl AdmittedNoticeWindowCapability<'_> {
    fn require_owner(
        &self,
        admitted: &AdmittedEnrollmentManifest,
        effects: &Arc<AuraEffectSystem>,
    ) -> Result<(), AuraError> {
        match self {
            Self::Active(window) => window.require_admitted_notice_owner(admitted, effects),
            Self::Recovered(window) => window.require_notice_ingress(admitted, effects.as_ref()),
        }
    }
    async fn remaining_ms(&self, effects: &AuraEffectSystem) -> Result<u64, TimeoutBudgetError> {
        match self {
            Self::Active(window) => window.remaining_ms(effects).await,
            Self::Recovered(window) => window.remaining_ms(effects).await,
        }
    }
    fn map_run_error(
        &self,
        stage: &'static str,
        source: TimeoutRunError<AgentError>,
    ) -> AgentError {
        match self {
            Self::Active(window) => window.map_run_error(stage, source),
            Self::Recovered(window) => window.map_run_error(stage, source),
        }
    }
}

/// No raw manifest or role argument can create enrollment notice ingress.
enum NoticeIngressCapability<'a> {
    Issued(
        &'a RetainedEnrollmentVmControl,
        IssuedNoticeWindowCapability<'a>,
    ),
    Admitted(
        &'a AdmittedEnrollmentManifest,
        AdmittedNoticeWindowCapability<'a>,
    ),
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "NoticeIngressCapability",
    family = "runtime_helper"
)]
async fn open_notice_session(
    effects: Arc<AuraEffectSystem>,
    ingress: NoticeIngressCapability<'_>,
) -> AgentResult<OwnedVmSession> {
    let (binding, digest, active_role) = match ingress {
        NoticeIngressCapability::Issued(issued, window) => {
            window.require_owner(issued, effects.as_ref())?;
            issued
                .require_runtime_owner(effects.as_ref())
                .map_err(AgentError::from)?;
            (issued.manifest(), issued.digest(), "Initiator")
        }
        NoticeIngressCapability::Admitted(admitted, window) => {
            window
                .require_owner(admitted, &effects)
                .map_err(AgentError::from)?;
            (admitted.manifest(), admitted.manifest_digest(), "Invitee")
        }
    };
    let initiator = ChoreographicRole::new(
        binding.initiator_device,
        binding.subject,
        RoleIndex::new(0).expect("fixed initiator role"),
    );
    let invitee = ChoreographicRole::new(
        binding.invitee_device,
        binding.subject,
        RoleIndex::new(1).expect("fixed invitee role"),
    );
    use aura_invitation::protocol::device_enrollment_terminal_notice::telltale_session_types_invitation_device_enrollment_terminal_notice::vm_artifacts;
    crate::runtime::open_owned_manifest_vm_session_admitted(
        effects,
        notice_session_id(&binding.invitation, &digest),
        vec![initiator, invitee],
        &vm_artifacts::composition_manifest(),
        active_role,
        &vm_artifacts::global_type(),
        &vm_artifacts::local_types(),
        crate::runtime::AuraVmSchedulerSignals::default(),
    )
    .await
    .map_err(stage)
}

/// A terminal notice signed for one original cancelled issuance. Only the
/// signing helpers below construct it; retries resend the same bytes.
pub(crate) struct SignedCancelledNotice {
    bytes: Vec<u8>,
}

/// Actual producer outcome retained by the original issuer task. Cancellation
/// is signed terminal evidence, never the absence of a positive receipt.
pub(crate) enum IssuerEnrollmentTerminalOutcome {
    Committed(super::IssuerEnrollmentTerminalReceipt),
    Cancelled(Arc<OriginalIssuerCancellationTerminal>),
}

pub(crate) struct OriginalIssuerCancellationTerminal {
    pub(super) cancellation:
        crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    pub(super) issued: Arc<RetainedEnrollmentVmControl>,
    // Captured once with the actual verified decision. Diagnostic retention
    // must not keep this capability's runtime owner alive after cleanup ACK.
    pub(super) failure: aura_core::AuraError,
}
impl OriginalIssuerCancellationTerminal {
    pub(crate) fn original_domain_failure(&self) -> aura_core::AuraError {
        self.failure.clone()
    }
    pub(crate) fn issued(&self) -> &RetainedEnrollmentVmControl {
        &self.issued
    }
    pub(crate) fn require_original_effects(
        &self,
        effects: &Arc<AuraEffectSystem>,
    ) -> AgentResult<()> {
        self.cancellation.require_runtime_owner(effects)?;
        if self.cancellation.ceremony() != &self.issued.manifest().ceremony
            || self.cancellation.invitation() != &self.issued.manifest().invitation
        {
            return Err(AgentError::invalid(
                "cancelled decision has another original issuance",
            ));
        }
        Ok(())
    }
}

/// Sign the live issuer's cancellation notice once, while the cancelled
/// provisional generation is still held, so it can be released before any
/// send retry (Task 80).
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "issued_original_window_terminal_notice",
    capability_type = VerifiedEnrollmentCancellationCapability,
    family = "runtime_helper"
)]
pub(super) async fn sign_cancelled_notice(
    effects: &Arc<AuraEffectSystem>,
    issued: &RetainedEnrollmentVmControl,
    runner: &CeremonyRunner,
    cancelled: &crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    window: &EnrollmentExecutionChild,
) -> AgentResult<SignedCancelledNotice> {
    let bytes = sign_cancelled_notice_bytes(
        effects,
        issued,
        runner,
        cancelled,
        &IssuedNoticeWindowCapability::Active(window),
    )
    .await?;
    Ok(SignedCancelledNotice { bytes })
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "issued_original_window_terminal_notice",
    capability_type = SignedCancelledNotice,
    family = "runtime_helper"
)]
pub(super) async fn send_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    terminal: &Arc<OriginalIssuerCancellationTerminal>,
    notice: &Arc<SignedCancelledNotice>,
    window: &EnrollmentExecutionChild,
    slot: &mut Option<OwnedVmSession>,
    destination: &crate::handlers::invitation_service::OriginalIssuerTerminalDestination,
) -> AgentResult<()> {
    let issued = terminal.issued();
    let validity = window
        .original_cancelled_notice_window(effects.as_ref(), terminal, notice, destination)
        .map_err(AgentError::from)?;
    #[cfg(test)]
    {
        let foreign = Arc::new(SignedCancelledNotice {
            bytes: notice.bytes.clone(),
        });
        assert!(
            window
                .original_cancelled_notice_window(effects.as_ref(), terminal, &foreign, destination)
                .is_err(),
            "equal signed bytes cannot replace the actual retained notice owner"
        );
        validity
            .require_owner(issued, effects.as_ref())
            .expect("same actual live negative owner");
        validity.assert_original_deadline_for_test();
    }
    validity
        .execute(effects.as_ref(), || {
            Box::pin(async {
                let completed = send_signed_cancelled_notice(
                    effects.clone(),
                    issued,
                    IssuedNoticeWindowCapability::OriginalCancelled(&validity),
                    notice.bytes.clone(),
                    slot,
                )
                .await?;
                // Retain before either validity postcheck or VM close awaits.
                destination.retain_original_cancelled_send(&effects, terminal, notice, completed)
            })
        })
        .await
        .map_err(|source| validity.map_run_error("signed enrollment notice validity", source))
}

/// Sign the terminal notice for the original cancelled issuance. The control
/// transcript is deterministic, so a notice signed once while the provisional
/// generation is still held stays valid for every later send retry.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "IssuedNoticeWindowCapability",
    family = "runtime_helper"
)]
async fn sign_cancelled_notice_bytes(
    effects: &Arc<AuraEffectSystem>,
    issued: &RetainedEnrollmentVmControl,
    runner: &CeremonyRunner,
    cancelled: &crate::runtime::services::ceremony_tracker::VerifiedEnrollmentCancellationCapability,
    window: &IssuedNoticeWindowCapability<'_>,
) -> AgentResult<Vec<u8>> {
    cancelled
        .require_runtime_owner(effects)
        .map_err(AgentError::from)?;
    if cancelled.invitation() != &issued.manifest().invitation
        || cancelled.ceremony() != &issued.manifest().ceremony
    {
        return Err(AgentError::invalid(
            "terminal notice cancellation belongs to another issuance",
        ));
    }
    window.require_owner(issued, effects.as_ref())?;
    issued
        .require_runtime_owner(effects.as_ref())
        .map_err(AgentError::from)?;
    let frame = super::enrollment_vm_admission::sign_terminal_confirmation(
        effects.as_ref(),
        issued,
        runner,
    )
    .await?;
    let notice = DeviceEnrollmentTerminalNotice {
        invitation_id: issued.manifest().invitation.clone(),
        ceremony_id: issued.manifest().ceremony.clone(),
        signed_control: to_vec(&frame).map_err(stage)?,
    };
    let bytes = to_vec(&notice).map_err(stage)?;
    if bytes.len() > DeviceEnrollmentTerminalNotice::MAX_WIRE_BYTES {
        return Err(AgentError::invalid(
            "signed terminal notice exceeds wire bound",
        ));
    }
    Ok(bytes)
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "IssuedNoticeWindowCapability",
    family = "runtime_helper"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn send_signed_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    issued: &RetainedEnrollmentVmControl,
    window: IssuedNoticeWindowCapability<'_>,
    bytes: Vec<u8>,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<OriginalCancelledNoticeSendCompletion> {
    let notice_digest = aura_core::hash::hash(&bytes);
    window.require_owner(issued, effects.as_ref())?;
    issued
        .require_runtime_owner(effects.as_ref())
        .map_err(AgentError::from)?;
    if slot.is_none() {
        let session = slot.insert(
            open_notice_session(
                effects.clone(),
                NoticeIngressCapability::Issued(
                    issued,
                    match &window {
                        IssuedNoticeWindowCapability::OriginalCancelled(owner) => {
                            IssuedNoticeWindowCapability::OriginalCancelled(owner)
                        }
                        IssuedNoticeWindowCapability::Active(owner) => {
                            IssuedNoticeWindowCapability::Active(owner)
                        }
                        IssuedNoticeWindowCapability::Cancelled(owner) => {
                            IssuedNoticeWindowCapability::Cancelled(owner)
                        }
                    },
                ),
            )
            .await?,
        );
        session.queue_send_bytes(bytes);
    }
    // A resumed delivery advances the original program counter. It neither
    // replaces the VM nor queues another copy of its signed payload.
    let session = slot
        .as_mut()
        .ok_or_else(|| AgentError::invalid("original terminal sender missing"))?;
    if session.owner().session_id.as_uuid()
        != notice_session_id(&issued.manifest().invitation, &issued.digest())
    {
        return Err(AgentError::invalid(
            "terminal sender has another original binding",
        ));
    }
    effects
        .assert_owned_choreography_boundary(session.owner(), session.routing_boundary())
        .map_err(stage)?;
    let peer = ChoreographicRole::new(
        issued.manifest().invitee_device,
        issued.manifest().subject,
        RoleIndex::new(1).expect("fixed invitee role"),
    );
    let peers = BTreeMap::from([("Invitee".into(), peer)]);
    loop {
        window
            .remaining_ms(effects.as_ref())
            .await
            .map_err(|source| {
                window.map_run_error("terminal notice send", TimeoutRunError::Timeout(source))
            })?;
        let round = session
            .advance_round("Initiator", &peers)
            .await
            .map_err(stage)?;
        if round.blocked_receive.is_some() {
            return Err(AgentError::invalid(
                "finite terminal sender unexpectedly received",
            ));
        }
        super::vm_loop::handle_invitation_vm_wait_status(
            round.host_wait_status,
            false,
            "terminal notice send timed out",
            "terminal notice send cancelled",
        )?;
        if super::vm_loop::handle_invitation_vm_step(
            round.step,
            "terminal notice sender became stuck",
        )? {
            return Ok(OriginalCancelledNoticeSendCompletion {
                owner: session.owner().clone(),
                invitation: issued.manifest().invitation.clone(),
                manifest_digest: issued.digest(),
                notice_digest,
            });
        }
    }
}

/// A cancelled notice signed once under the original cancelled window, whose
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "CancelledEnrollmentNoticeWindowCapability",
    family = "runtime_helper"
)]
async fn send_recovered_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    issued: &RetainedEnrollmentVmControl,
    window: &crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability,
    bytes: Vec<u8>,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<()> {
    window
        .require_issued_owner(issued, effects.as_ref())
        .map_err(AgentError::from)?;
    send_signed_cancelled_notice(
        effects,
        issued,
        IssuedNoticeWindowCapability::Cancelled(window),
        bytes,
        slot,
    )
    .await
    .map(|_| ())
}

/// The original required invitee task retains the actual notice VM cursor.
/// A signed candidate is not a terminal: the same selected-time check,
/// blocked receive, and finite VM completion must still run before settlement.
#[derive(Default)]
pub(crate) struct RetainedOriginalNoticeVm {
    pub(super) session: Option<OwnedVmSession>,
    pub(super) candidate: Option<VerifiedEnrollmentFailureCapability>,
    blocked: Option<crate::runtime::vm_host_bridge::BlockedVmReceive>,
    injected: bool,
    completed: bool,
    pub(super) eligible: bool,
    injection_failure: Option<Arc<AgentError>>,
    // Observation suppresses withdrawal only. The actual candidate, original
    // postchecks, VM completion and close still authorize terminal settlement.
    candidate_observed: Arc<std::sync::OnceLock<()>>,
}

impl RetainedOriginalNoticeVm {
    pub(crate) fn is_eligible(&self) -> bool {
        self.eligible
    }

    /// The runtime custodian may retire a pending receive only after this same
    /// original VM has actually acknowledged close. Refusal retains its cursor.
    pub(crate) async fn close_for_required_cleanup(
        &mut self,
    ) -> Result<(), crate::runtime::session_ingress::SessionIngressError> {
        self.close_retained().await?;
        self.retire_cancelled_receive_after_close();
        Ok(())
    }
    pub(super) fn candidate_observation(&self) -> Arc<std::sync::OnceLock<()>> {
        Arc::clone(&self.candidate_observed)
    }
    pub(super) fn retire_cancelled_receive_after_close(&mut self) {
        debug_assert!(self.session.is_none());
        self.blocked.take();
    }

    pub(super) async fn close_retained(
        &mut self,
    ) -> Result<(), crate::runtime::session_ingress::SessionIngressError> {
        if let Some(session) = self.session.as_mut() {
            session.close_retained().await?;
            self.session.take();
        }
        Ok(())
    }
}

pub(super) async fn receive_cancelled_notice_retained(
    effects: Arc<AuraEffectSystem>,
    admitted: &AdmittedEnrollmentManifest,
    window: &EnrollmentExecutionChild,
    vm: &mut RetainedOriginalNoticeVm,
    #[cfg(test)] gate: &crate::runtime::services::invitation_manager::InvitationManager,
) -> AgentResult<()> {
    if let Some(source) = vm.injection_failure.as_ref() {
        return Err(AgentError::Aura(aura_core::AuraError::Internal {
            message: "original notice blocked receive injection failed".into(),
            source: Some(Arc::clone(source) as Arc<dyn std::error::Error + Send + Sync>),
        }));
    }
    let ingress = AdmittedNoticeWindowCapability::Active(window);
    ingress
        .require_owner(admitted, &effects)
        .map_err(AgentError::from)?;
    if vm.session.is_none() {
        vm.session = Some(
            open_notice_session(
                effects.clone(),
                NoticeIngressCapability::Admitted(admitted, ingress),
            )
            .await?,
        );
    }
    let peer = ChoreographicRole::new(
        admitted.manifest().initiator_device,
        admitted.manifest().subject,
        RoleIndex::new(0).expect("fixed initiator role"),
    );
    let peers = BTreeMap::from([("Initiator".into(), peer)]);
    loop {
        if vm.completed {
            return Ok(());
        }
        if vm.candidate.is_some() {
            if !vm.injected {
                ingress
                    .remaining_ms(effects.as_ref())
                    .await
                    .map_err(|source| {
                        ingress.map_run_error(
                            "verified terminal notice",
                            TimeoutRunError::Timeout(source),
                        )
                    })?;
                let blocked = vm.blocked.take().ok_or_else(|| {
                    AgentError::internal("original signed notice lost its blocked receive")
                })?;
                if let Err(source) = vm
                    .session
                    .as_mut()
                    .ok_or_else(|| AgentError::internal("original notice VM was not retained"))?
                    .inject_blocked_receive(blocked)
                {
                    let source = Arc::new(stage(source));
                    vm.injection_failure = Some(Arc::clone(&source));
                    return Err(AgentError::Aura(aura_core::AuraError::Internal {
                        message: "original notice blocked receive injection failed".into(),
                        source: Some(source),
                    }));
                }
                vm.injected = true;
            }
            let round = vm
                .session
                .as_mut()
                .ok_or_else(|| AgentError::internal("original notice VM was not retained"))?
                .advance_round("Invitee", &peers)
                .await
                .map_err(stage)?;
            if round.blocked_receive.is_some() {
                return Err(AgentError::invalid(
                    "terminal listener received after finite notice",
                ));
            }
            if super::vm_loop::handle_invitation_vm_wait_status(
                round.host_wait_status,
                false,
                "terminal listener completion timed out",
                "terminal listener completion cancelled",
            )?
            .is_some()
                || super::vm_loop::handle_invitation_vm_step(
                    round.step,
                    "terminal listener became stuck",
                )?
            {
                vm.completed = true;
                return Ok(());
            }
            continue;
        }
        ingress
            .remaining_ms(effects.as_ref())
            .await
            .map_err(|source| {
                ingress.map_run_error("terminal notice receive", TimeoutRunError::Timeout(source))
            })?;
        let round = vm
            .session
            .as_mut()
            .ok_or_else(|| AgentError::internal("original notice VM was not retained"))?
            .advance_round_until_receive(
                "Invitee",
                &peers,
                InvitationHandler::is_transport_no_message,
            )
            .await
            .map_err(stage)?;
        if let Some(blocked) = round.blocked_receive {
            if blocked.payload.len() > DeviceEnrollmentTerminalNotice::MAX_WIRE_BYTES {
                continue;
            }
            let Ok(notice) = from_slice::<DeviceEnrollmentTerminalNotice>(&blocked.payload) else {
                continue;
            };
            if notice.invitation_id != admitted.manifest().invitation
                || notice.ceremony_id != admitted.manifest().ceremony
            {
                continue;
            }
            let frame = match EnrollmentControlFrame::decode(&notice.signed_control) {
                Ok(frame) => frame,
                Err(error)
                    if discard_unverified_notice(&error)
                        || matches!(
                            error,
                            AgentError::Aura(aura_core::AuraError::Serialization { .. })
                        ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            let verified = match frame
                .verify_terminal_notice(effects.as_ref(), admitted)
                .await
            {
                Ok(verified) => verified,
                Err(error) if discard_unverified_notice(&error) => continue,
                Err(error) => return Err(error),
            };
            // This is the earliest genuine signed-candidate boundary. Both
            // nonclone receive and proof enter the original required owner
            // before the next selected-time observation can be cancelled.
            vm.blocked = Some(blocked);
            vm.candidate = Some(verified);
            let _ = vm.candidate_observed.set(());
            #[cfg(test)]
            gate.await_original_invitee_stage_gate_for_test(
                &admitted.canonical_invitation().invitation_id,
                crate::runtime::services::invitation_manager::OriginalInviteeStage::SignedNoticeCandidate,
            )
            .await;
            continue;
        }
        super::vm_loop::handle_invitation_vm_wait_status(
            round.host_wait_status,
            false,
            "terminal listener timed out",
            "terminal listener cancelled",
        )?;
        if pending_terminal_receive(round.host_wait_status, &round.step) {
            continue;
        }
        if super::vm_loop::handle_invitation_vm_step(round.step, "terminal listener became stuck")?
        {
            return Err(AgentError::invalid(
                "terminal listener completed without verified notice",
            ));
        }
    }
}
async fn receive_cancelled_notice_with_window(
    effects: Arc<AuraEffectSystem>,
    admitted: &AdmittedEnrollmentManifest,
    window: AdmittedNoticeWindowCapability<'_>,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<VerifiedEnrollmentFailureCapability> {
    window
        .require_owner(admitted, &effects)
        .map_err(AgentError::from)?;
    let session = slot.insert(
        open_notice_session(
            effects.clone(),
            NoticeIngressCapability::Admitted(admitted, window),
        )
        .await?,
    );
    let peer = ChoreographicRole::new(
        admitted.manifest().initiator_device,
        admitted.manifest().subject,
        RoleIndex::new(0).expect("fixed initiator role"),
    );
    let peers = BTreeMap::from([("Initiator".into(), peer)]);
    loop {
        window
            .remaining_ms(effects.as_ref())
            .await
            .map_err(|source| {
                window.map_run_error("terminal notice receive", TimeoutRunError::Timeout(source))
            })?;
        let round = session
            .advance_round_until_receive(
                "Invitee",
                &peers,
                InvitationHandler::is_transport_no_message,
            )
            .await
            .map_err(stage)?;
        if let Some(blocked) = round.blocked_receive {
            if blocked.payload.len() > DeviceEnrollmentTerminalNotice::MAX_WIRE_BYTES {
                continue;
            }
            let Ok(notice) = from_slice::<DeviceEnrollmentTerminalNotice>(&blocked.payload) else {
                continue;
            };
            if notice.invitation_id != admitted.manifest().invitation
                || notice.ceremony_id != admitted.manifest().ceremony
            {
                continue;
            }
            let frame = match EnrollmentControlFrame::decode(&notice.signed_control) {
                Ok(frame) => frame,
                Err(error)
                    if discard_unverified_notice(&error)
                        || matches!(
                            error,
                            AgentError::Aura(aura_core::AuraError::Serialization { .. })
                        ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            let verified = match frame
                .verify_terminal_notice(effects.as_ref(), admitted)
                .await
            {
                Ok(verified) => verified,
                Err(error) if discard_unverified_notice(&error) => continue,
                Err(error) => return Err(error),
            };
            window
                .remaining_ms(effects.as_ref())
                .await
                .map_err(|source| {
                    window
                        .map_run_error("verified terminal notice", TimeoutRunError::Timeout(source))
                })?;
            session.inject_blocked_receive(blocked).map_err(stage)?;
            // Signed evidence is returned only after this finite VM finishes.
            loop {
                let round = session
                    .advance_round("Invitee", &peers)
                    .await
                    .map_err(stage)?;
                if round.blocked_receive.is_some() {
                    return Err(AgentError::invalid(
                        "terminal listener received after finite notice",
                    ));
                }
                if super::vm_loop::handle_invitation_vm_wait_status(
                    round.host_wait_status,
                    false,
                    "terminal listener completion timed out",
                    "terminal listener completion cancelled",
                )?
                .is_some()
                    || super::vm_loop::handle_invitation_vm_step(
                        round.step,
                        "terminal listener became stuck",
                    )?
                {
                    return Ok(verified);
                }
            }
        }
        super::vm_loop::handle_invitation_vm_wait_status(
            round.host_wait_status,
            false,
            "terminal listener timed out",
            "terminal listener cancelled",
        )?;
        if pending_terminal_receive(round.host_wait_status, &round.step) {
            continue;
        }
        if super::vm_loop::handle_invitation_vm_step(round.step, "terminal listener became stuck")?
        {
            return Err(AgentError::invalid(
                "terminal listener completed without verified notice",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deferred_terminal_receive_waits_within_original_window() {
        assert!(pending_terminal_receive(
            AuraVmHostWaitStatus::Deferred,
            &StepResult::Stuck
        ));
        assert!(!pending_terminal_receive(
            AuraVmHostWaitStatus::Idle,
            &StepResult::Stuck
        ));
        assert!(!pending_terminal_receive(
            AuraVmHostWaitStatus::TimedOut,
            &StepResult::Stuck
        ));
        assert!(!pending_terminal_receive(
            AuraVmHostWaitStatus::Cancelled,
            &StepResult::Stuck
        ));
        assert!(!pending_terminal_receive(
            AuraVmHostWaitStatus::Deferred,
            &StepResult::AllDone
        ));
    }

    #[test]
    fn negative_session_identity_is_distinct_and_exactly_bound() {
        let invitation = InvitationId::new("negative-session-binding");
        let digest = [0x5a; 32];
        let notice = notice_session_id(&invitation, &digest);
        assert_ne!(
            notice,
            InvitationHandler::invitation_session_id(&invitation)
        );
        assert_ne!(notice, notice_session_id(&invitation, &[0x5b; 32]));
        assert_ne!(
            notice,
            notice_session_id(&InvitationId::new("other-negative-session"), &digest)
        );
        assert_eq!(notice, notice_session_id(&invitation, &digest));
    }
}

/// The required cancellation task owns this value before its first awaited
/// signing, retirement, publication or delivery operation.
pub(crate) struct RetainedCancelledNotice {
    registered:
        Arc<crate::runtime::services::ceremony_tracker::RegisteredCancelledNoticeCapability>,
    window: Option<
        crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability,
    >,
    bytes: Option<Vec<u8>>,
    retirement:
        Option<crate::runtime::services::ceremony_tracker::OriginalCancellationRetirementReceipt>,
    refused_retirement: Option<crate::runtime::effects::AcknowledgedEnrollmentRetirement>,
    checkpoint: Option<
        crate::runtime::services::ceremony_tracker::OriginalCancellationNegativeCheckpointReceipt,
    >,
    slot: Option<OwnedVmSession>,
    slot_closed: bool,
    negative_validated: bool,
    first_failure: Option<AuraError>,
    work_failure: Option<AuraError>,
    checkpoint_failure: Option<AuraError>,
    validation_failure: Option<TimeoutBudgetError>,
    sign_observation: Option<aura_core::time::timeout::CheckpointExecution<(), AgentError, ()>>,
    settlement_published: bool,
    delivery_observation: Option<aura_core::time::timeout::CheckpointExecution<(), AgentError, ()>>,
    delivery_finished: bool,
    #[cfg(test)]
    test_registration: Option<
        Arc<crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration>,
    >,
}
impl RetainedCancelledNotice {
    pub(crate) fn new(
        registered: crate::runtime::services::ceremony_tracker::RegisteredCancelledNoticeCapability,
    ) -> Self {
        Self {
            registered: Arc::new(registered),
            window: None,
            bytes: None,
            retirement: None,
            refused_retirement: None,
            checkpoint: None,
            slot: None,
            slot_closed: false,
            negative_validated: false,
            first_failure: None,
            work_failure: None,
            checkpoint_failure: None,
            validation_failure: None,
            sign_observation: None,
            settlement_published: false,
            delivery_observation: None,
            delivery_finished: false,
            #[cfg(test)]
            test_registration: None,
        }
    }
    pub(crate) async fn sign_original(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
        issued: &RetainedEnrollmentVmControl,
        runner: &CeremonyRunner,
    ) -> AgentResult<()> {
        if self.window.is_none() {
            self.window = Some(crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability::registered(
                Arc::clone(&self.registered), effects.as_ref(),
            ).map_err(AgentError::from)?);
        }
        let window = self
            .window
            .as_mut()
            .ok_or_else(|| AgentError::invalid("original notice window missing"))?;
        window.admit_original(effects.as_ref()).await;
        window
            .require_issued_owner(issued, effects.as_ref())
            .map_err(AgentError::from)?;
        if self.bytes.is_some() {
            return Ok(());
        }
        let bytes = &mut self.bytes;
        let first_failure = &mut self.first_failure;
        let actual = window
            .execute(effects.as_ref(), || async {
                match sign_cancelled_notice_bytes(
                    effects,
                    issued,
                    runner,
                    window.cancelled(),
                    &IssuedNoticeWindowCapability::Cancelled(window),
                )
                .await
                {
                    Ok(actual) => {
                        *bytes = Some(actual);
                        Ok(())
                    }
                    Err(source) => {
                        let source = retained_notice_source(source);
                        first_failure.get_or_insert_with(|| source.clone());
                        Err(AgentError::from(source))
                    }
                }
            })
            .await
            .map_err(|source| AgentError::from(AuraError::from(source)))?;
        self.sign_observation = Some(actual);
        match self.sign_observation.as_ref() {
            Some(aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            }) => Ok(()),
            Some(aura_core::time::timeout::CheckpointExecution::Negative { source, .. }) => {
                self.validation_failure
                    .get_or_insert_with(|| source.clone());
                Err(AgentError::from(AuraError::from(source.clone())))
            }
            _ => Err(AgentError::from(self.first_failure.clone().unwrap_or_else(
                || AuraError::permission_denied("original cancellation signing remains unresolved"),
            ))),
        }
    }
    pub(crate) async fn retire_original(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
        issued: &RetainedEnrollmentVmControl,
        runner: &CeremonyRunner,
    ) -> AgentResult<()> {
        if let Some(actual) = &self.retirement {
            return actual
                .require_original(&self.registered, issued, effects.as_ref())
                .map_err(AgentError::from);
        }
        let registered = &self.registered;
        let retirement = &mut self.retirement;
        let refused = &mut self.refused_retirement;
        let first_failure = &mut self.first_failure;
        let work_failure = &mut self.work_failure;
        let observed = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            registered.negative_cleanup_budget(),
            Arc::clone(registered.cancelled()),
            || async {
                let actual = match runner
                    .retire_failed_enrollment_generation(&issued.manifest().ceremony)
                    .await
                {
                    Ok(actual) => actual,
                    Err(source) => {
                        first_failure.get_or_insert_with(|| source.clone());
                        work_failure.get_or_insert_with(|| source.clone());
                        return Err(source);
                    }
                };
                match registered.seal_original_retirement(actual, issued, effects.as_ref()) {
                    Ok(actual) => {
                        *retirement = Some(actual);
                        Ok(())
                    }
                    Err(rejected) => {
                        let (actual, source) = *rejected;
                        *refused = Some(actual);
                        first_failure.get_or_insert_with(|| source.clone());
                        work_failure.get_or_insert_with(|| source.clone());
                        Err(source)
                    }
                }
            },
        )
        .await;
        if let Err(source) = observed.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AgentError::from(AuraError::from(source)));
        }
        match observed.operation {
            Some(Ok(())) => Ok(()),
            Some(Err(source)) => Err(AgentError::from(source)),
            None => Err(AgentError::from(AuraError::permission_denied(
                "original retirement interrupted",
            ))),
        }
    }
    pub(crate) async fn publish_settlement(
        &mut self,
        runner: &CeremonyRunner,
        issued: &RetainedEnrollmentVmControl,
    ) {
        if self.settlement_published {
            return;
        }
        let result = self.first_failure.clone().map_or(Ok(()), Err);
        runner
            .publish_cancelled_generation_settlement(&issued.manifest().ceremony, result)
            .await;
        self.settlement_published = true;
    }
    pub(crate) async fn acknowledge_negative(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
        issued: &RetainedEnrollmentVmControl,
    ) -> AgentResult<()> {
        self.retirement
            .as_ref()
            .ok_or_else(|| AgentError::invalid("original retirement receipt missing"))?
            .require_original(&self.registered, issued, effects.as_ref())
            .map_err(AgentError::from)?;
        let registered = &self.registered;
        let slot = &mut self.slot;
        let closed = &mut self.slot_closed;
        let work_failure = &mut self.work_failure;
        let work = aura_core::time::timeout::execute_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            registered.negative_cleanup_budget(),
            Arc::clone(registered.cancelled()),
            || async {
                if !*closed {
                    if let Some(actual) = slot.as_mut() {
                        if let Err(source) = actual.close_retained().await {
                            let source = retained_notice_source(stage(source));
                            work_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                    }
                    *closed = true;
                }
                Ok(())
            },
        )
        .await;
        if let Err(source) = work.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AgentError::from(AuraError::from(source)));
        }
        match work.operation {
            Some(Ok(())) => {}
            Some(Err(source)) => return Err(source.into()),
            None => {
                return Err(AgentError::invalid(
                    "original cancellation disposal interrupted",
                ));
            }
        }
        let retained = &mut self.checkpoint;
        let checkpoint_failure = &mut self.checkpoint_failure;
        let observed = aura_core::time::timeout::acknowledge_negative_cleanup_with_timeout_budget(
            effects.as_ref(),
            registered.negative_cleanup_budget(),
            Arc::clone(registered.cancelled()),
            || async {
                if retained.is_none() {
                    match registered
                        .acknowledge_negative_checkpoint(effects.as_ref())
                        .await
                    {
                        Ok(actual) => *retained = Some(actual),
                        Err(source) => {
                            checkpoint_failure.get_or_insert_with(|| source.clone());
                            return Err(TimeoutBudgetError::checkpoint_failure(source));
                        }
                    }
                }
                retained.as_ref().ok_or_else(|| {
                    TimeoutBudgetError::checkpoint_failure(AuraError::invalid(
                        "original negative checkpoint missing",
                    ))
                })
            },
        )
        .await;
        if let Err(source) = observed.validation {
            self.validation_failure
                .get_or_insert_with(|| source.clone());
            return Err(AgentError::from(AuraError::from(source)));
        }
        match observed.checkpoint {
            aura_core::time::timeout::CheckpointAttempt::Returned(actual) => {
                actual
                    .require_original(registered)
                    .map_err(AgentError::from)?;
                self.negative_validated = true;
                Ok(())
            }
            aura_core::time::timeout::CheckpointAttempt::Failed { source } => {
                Err(AgentError::from(AuraError::from(source)))
            }
            _ => Err(AgentError::invalid(
                "original negative checkpoint interrupted",
            )),
        }
    }
}
fn retained_notice_source(source: AgentError) -> AuraError {
    match source {
        AgentError::Aura(source) => source,
        source => AuraError::Internal {
            message: "original retained cancellation notice".into(),
            source: Some(Arc::new(source)),
        },
    }
}

/// This terminal retains the actual original owner, signed bytes, native
/// retirement, VM disposal state, checkpoint and every independent fault.
pub(crate) struct AcknowledgedCancelledNotice {
    original: RetainedCancelledNotice,
}
impl RetainedCancelledNotice {
    #[cfg(test)]
    pub(crate) fn observe_registration_for_test(
        &mut self,
        original: Arc<
            crate::runtime::services::ceremony_tracker::OriginalCancellationTaskRegistration,
        >,
    ) {
        self.test_registration = Some(original);
    }
    pub(crate) async fn deliver_original(
        &mut self,
        effects: &Arc<AuraEffectSystem>,
        issued: &RetainedEnrollmentVmControl,
    ) -> AgentResult<()> {
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| AgentError::invalid("original delivery window missing"))?;
        let bytes = self
            .bytes
            .as_ref()
            .ok_or_else(|| AgentError::invalid("original signed notice missing"))?;
        let slot = &mut self.slot;
        let first_failure = &mut self.first_failure;
        let delivery_finished = &mut self.delivery_finished;
        #[cfg(test)]
        let test_registration = &self.test_registration;
        let observed = window
            .execute(effects.as_ref(), || async {
                if *delivery_finished {
                    return Ok(());
                }
                let result = send_recovered_cancelled_notice(
                    effects.clone(),
                    issued,
                    window,
                    bytes.clone(),
                    slot,
                )
                .await;
                match result {
                    Ok(()) => {
                        // Retain actual delivery before the enclosing executor
                        // awaits its independent post-operation checkpoint.
                        *delivery_finished = true;
                        #[cfg(test)]
                        if let (Some(original), Some(session)) =
                            (test_registration.as_ref(), slot.as_ref())
                        {
                            original
                                .notice_delivered_for_test(session, bytes, first_failure.clone())
                                .await;
                        }
                        Ok(())
                    }
                    Err(source) => {
                        let source = retained_notice_source(source);
                        first_failure.get_or_insert_with(|| source.clone());
                        Err(AgentError::from(source))
                    }
                }
            })
            .await
            .map_err(|source| AgentError::from(AuraError::from(source)))?;
        // Retain the complete independent post-observation outcome.
        self.delivery_observation = Some(observed);
        match self.delivery_observation.as_ref() {
            Some(aura_core::time::timeout::CheckpointExecution::Finished {
                operation: Ok(()),
                ..
            }) => Ok(()),
            Some(aura_core::time::timeout::CheckpointExecution::Negative { source, .. }) => {
                self.validation_failure
                    .get_or_insert_with(|| source.clone());
                Err(AgentError::from(AuraError::from(source.clone())))
            }
            _ => Err(AgentError::from(self.first_failure.clone().unwrap_or_else(
                || AuraError::permission_denied("original notice delivery unresolved"),
            ))),
        }
    }
    pub(crate) fn consume_acknowledged(
        self,
        issued: &RetainedEnrollmentVmControl,
        effects: &AuraEffectSystem,
    ) -> Result<AcknowledgedCancelledNotice, (Box<Self>, AgentError)> {
        let checked = (|| {
            self.retirement
                .as_ref()
                .ok_or_else(|| AgentError::invalid("actual original retirement missing"))?
                .require_original(&self.registered, issued, effects)
                .map_err(AgentError::from)?;
            self.checkpoint
                .as_ref()
                .ok_or_else(|| AgentError::invalid("actual original checkpoint missing"))?
                .require_original(&self.registered)
                .map_err(AgentError::from)?;
            if !self.slot_closed || !self.negative_validated {
                return Err(AgentError::invalid(
                    "original negative disposal not acknowledged",
                ));
            }
            Ok(())
        })();
        match checked {
            Ok(()) => Ok(AcknowledgedCancelledNotice { original: self }),
            Err(source) => Err((Box::new(self), source)),
        }
    }
}
impl AcknowledgedCancelledNotice {
    pub(crate) fn domain_failure(&self) -> Option<&AuraError> {
        self.original.first_failure.as_ref()
    }
}

/// Observe the original producer's already-retained settlement while publishing
/// its canonical notification. This borrowed API transfers no terminal custody.
pub(super) async fn publish_original_cancelled_settlement<T>(
    runner: &CeremonyRunner,
    ceremony: &aura_core::CeremonyId,
    settled: &Result<T, AuraError>,
) {
    runner
        .publish_cancelled_generation_settlement(
            ceremony,
            settled.as_ref().map(|_| ()).map_err(Clone::clone),
        )
        .await;
}

/// Physical disposal from the actual retained finite negative VM, never wiredata.
pub(crate) struct RecoveredEnrollmentNoticeVmDisposal {
    admitted: Arc<AdmittedEnrollmentManifest>,
    effects: std::sync::Weak<AuraEffectSystem>,
}
impl RecoveredEnrollmentNoticeVmDisposal {
    pub(crate) fn require_original(
        &self,
        admitted: &AdmittedEnrollmentManifest,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.admitted.as_ref(), admitted)
            || !self
                .effects
                .upgrade()
                .is_some_and(|actual| std::ptr::eq(actual.as_ref(), effects))
        {
            return Err(AuraError::permission_denied(
                "foreign recovered notice VM disposal",
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveredNoticeCrashPhase {
    BeforeClose,
    AfterClose,
}
#[cfg(test)]
pub(crate) struct RecoveredNoticeCrashGate {
    pub(crate) invitation: aura_core::InvitationId,
    pub(crate) phase: RecoveredNoticeCrashPhase,
    pub(crate) entered: tokio::sync::oneshot::Sender<()>,
    pub(crate) release: tokio::sync::oneshot::Receiver<()>,
}
struct RecoveredEnrollmentNoticeCustody {
    #[cfg(test)]
    crash_gate: Option<RecoveredNoticeCrashGate>,
    effects: Arc<AuraEffectSystem>,
    admitted: Arc<AdmittedEnrollmentManifest>,
    window: crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability,
    slot: Option<OwnedVmSession>,
    failure: Option<Arc<VerifiedEnrollmentFailureCapability>>,
    disposal: Option<RecoveredEnrollmentNoticeVmDisposal>,
    prepared: Option<super::enrollment_manifest_admission::PreparedRecoveredEnrollmentFailure>,
    acknowledged: Option<crate::runtime::services::enrollment_window::AcknowledgedEnrollmentWindow>,
    persisted: bool,
    persistence_validated: bool,
    first_failure: Option<AuraError>,
}
fn recovered_notice_run_error(
    window: &crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability,
    source: aura_core::TimeoutRunError<AuraError>,
) -> AuraError {
    match source {
        aura_core::TimeoutRunError::Operation(source) => source,
        aura_core::TimeoutRunError::Timeout(source) => {
            retained_notice_source(window.map_run_error(
                "recovered original negative notice",
                aura_core::TimeoutRunError::Timeout(source),
            ))
        }
    }
}
impl RecoveredEnrollmentNoticeCustody {
    #[cfg(test)]
    async fn observe_crash_boundary(&mut self, phase: RecoveredNoticeCrashPhase) {
        if self
            .crash_gate
            .as_ref()
            .is_some_and(|gate| gate.phase == phase)
        {
            let gate = self.crash_gate.take().expect("same retained crash gate");
            assert_eq!(
                gate.invitation,
                self.admitted.canonical_invitation().invitation_id
            );
            assert!(self.prepared.is_some() && self.disposal.is_some());
            assert_eq!(
                self.acknowledged.is_some(),
                phase == RecoveredNoticeCrashPhase::AfterClose
            );
            gate.entered
                .send(())
                .expect("actual child observes owned boundary");
            let _ = gate.release.await;
        }
    }
    async fn execute_original(&mut self) -> Result<(), AuraError> {
        let result = self.execute_stages().await;
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
        }
        result
    }
    async fn execute_stages(&mut self) -> Result<(), AuraError> {
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if self.prepared.is_none() {
            if self.failure.is_none() {
                let Self {
                    effects,
                    admitted,
                    window,
                    slot,
                    failure,
                    first_failure,
                    ..
                } = self;
                window
                    .execute(effects.as_ref(), || async {
                        match receive_cancelled_notice_with_window(
                            effects.clone(),
                            admitted.as_ref(),
                            AdmittedNoticeWindowCapability::Recovered(window),
                            slot,
                        )
                        .await
                        {
                            Ok(actual) => {
                                *failure = Some(Arc::new(actual));
                                Ok(())
                            }
                            Err(source) => {
                                let source = retained_notice_source(source);
                                first_failure.get_or_insert_with(|| source.clone());
                                Err(source)
                            }
                        }
                    })
                    .await
                    .map_err(|source| recovered_notice_run_error(window, source))?;
            }
            if self.disposal.is_none() {
                let Self {
                    effects,
                    admitted,
                    window,
                    slot,
                    disposal,
                    first_failure,
                    ..
                } = self;
                window
                    .execute(effects.as_ref(), || async {
                        let actual = slot.as_mut().ok_or_else(|| {
                            AuraError::invalid("original recovered notice VM missing")
                        })?;
                        if let Err(source) = actual.close_retained().await {
                            let source = AuraError::Internal {
                                message: "close recovered original negative VM".into(),
                                source: Some(Arc::new(source)),
                            };
                            first_failure.get_or_insert_with(|| source.clone());
                            return Err(source);
                        }
                        *slot = None;
                        *disposal = Some(RecoveredEnrollmentNoticeVmDisposal {
                            admitted: admitted.clone(),
                            effects: Arc::downgrade(effects),
                        });
                        Ok(())
                    })
                    .await
                    .map_err(|source| recovered_notice_run_error(window, source))?;
            }
            let Self {
                effects,
                admitted,
                window,
                failure,
                disposal,
                prepared,
                first_failure,
                ..
            } = self;
            // The proof is borrowed through storage; only the fully acknowledged
            // preparation transfers into this owner before callback Ready.
            let actual = failure
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original verified negative notice missing"))?
                .clone();
            window.execute(effects.as_ref(),||async {
                match super::enrollment_manifest_admission::prepare_recovered_enrollment_failure(effects,admitted,actual,disposal.as_ref().ok_or_else(||AuraError::invalid("original VM disposal missing"))?).await {
                    Ok(actual)=>{ *prepared=Some(actual); Ok(()) },
                    Err(source)=>{ first_failure.get_or_insert_with(||source.clone()); Err(source) }
                }
            }).await.map_err(|source| recovered_notice_run_error(window, source))?;
        }
        #[cfg(test)]
        self.observe_crash_boundary(RecoveredNoticeCrashPhase::BeforeClose)
            .await;
        if self.acknowledged.is_none() {
            self.acknowledged = Some(
                self.window
                    .acknowledge_failure(
                        self.effects.as_ref(),
                        self.prepared.as_ref().ok_or_else(|| {
                            AuraError::invalid("original prepared negative notice missing")
                        })?,
                    )
                    .await?,
            );
        }
        #[cfg(test)]
        self.observe_crash_boundary(RecoveredNoticeCrashPhase::AfterClose)
            .await;
        if !self.persistence_validated {
            let Self {
                effects,
                window,
                prepared,
                acknowledged,
                persisted,
                persistence_validated,
                first_failure,
                ..
            } = self;
            window.execute(effects.as_ref(),||async {
                if *persisted { return Ok(()); }
                let result=super::enrollment_manifest_admission::persist_prepared_recovered_enrollment_failure(effects.as_ref(),prepared.as_ref().ok_or_else(||AuraError::invalid("prepared original failure missing"))?,acknowledged.as_ref().ok_or_else(||AuraError::invalid("original negative acknowledgment missing"))?).await;
                if let Err(source)=&result { first_failure.get_or_insert_with(||source.clone()); }
                result?;
                *persisted=true;
                Ok::<(),AuraError>(())
            }).await.map_err(|source| recovered_notice_run_error(window, source))?;
            *persistence_validated = true;
        }
        Ok(())
    }
    async fn cleanup_original(
        &mut self,
    ) -> Result<aura_core::effects::task::RequiredTaskSettlement, AuraError> {
        if !self.persistence_validated {
            self.execute_original().await?;
        }
        Ok(aura_core::effects::task::RequiredTaskSettlement::Resolved)
    }
}
#[cfg(not(target_arch = "wasm32"))]
impl aura_core::effects::task::RequiredTaskCustody for RecoveredEnrollmentNoticeCustody {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::BoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}
#[cfg(target_arch = "wasm32")]
impl aura_core::effects::task::LocalRequiredTaskCustody for RecoveredEnrollmentNoticeCustody {
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        _: &'a aura_core::effects::task::RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<
        'a,
        Result<aura_core::effects::task::RequiredTaskSettlement, AuraError>,
    > {
        Box::pin(self.cleanup_original())
    }
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "RecoveredEnrollmentNoticeWindowCapability",
    family = "runtime_helper"
)]
pub(crate) fn launch_recovered_enrollment_notice(
    effects: Arc<AuraEffectSystem>,
    admitted: Arc<AdmittedEnrollmentManifest>,
    mut window:crate::runtime::services::enrollment_window::RecoveredEnrollmentNoticeWindowCapability,
    tasks: &crate::task_registry::TaskGroup,
    #[cfg(test)] crash_gate: Option<RecoveredNoticeCrashGate>,
) -> Result<crate::task_registry::OriginalRequiredTaskHandle, AuraError> {
    window.require_admitted_owner(admitted.as_ref(), effects.as_ref())?;
    let prepared = window.take_prepared_failure();
    let custody = RecoveredEnrollmentNoticeCustody {
        #[cfg(test)]
        crash_gate,
        effects,
        admitted,
        window,
        slot: None,
        failure: None,
        disposal: None,
        prepared,
        acknowledged: None,
        persisted: false,
        persistence_validated: false,
        first_failure: None,
    };
    cfg_if::cfg_if! {
     if #[cfg(target_arch="wasm32")] {
      let task=tasks.spawn_original_local_required_custody("original-recovered-negative-notice".into(),Box::new(custody),None).map_err(|rejected|retained_notice_source(crate::handlers::invitation_service::retain_rejected_local_issuer(rejected.into_local())))?;
     } else {
      let task=tasks.spawn_original_required_custody("original-recovered-negative-notice".into(),Box::new(custody),None).map_err(|rejected|retained_notice_source(crate::handlers::invitation_service::retain_rejected_native_issuer(rejected.into_native())))?;
     }
    }
    Ok(task)
}

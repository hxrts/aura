//! Negative terminal notification has its own finite protocol. It never injects
//! a confirmation into the request/response protocol's current program counter.
use super::enrollment_manifest_admission::AdmittedEnrollmentManifest;
use super::enrollment_trust::RetainedEnrollmentVmControl;
use super::enrollment_vm_admission::{
    EnrollmentControlFrame, EnrollmentVmAdmissionError, VerifiedEnrollmentFailureCapability,
};
use super::*;
use crate::runtime::services::ceremony_runner::CeremonyRunner;
use crate::runtime::services::enrollment_window::EnrollmentWindowCapability;
use crate::runtime::session_ingress::OwnedVmSession;
use aura_invitation::protocol::DeviceEnrollmentTerminalNotice;
use std::collections::BTreeMap;

fn notice_session_id(invitation: &InvitationId, digest: &[u8; 32]) -> Uuid {
    let mut transcript = b"aura.invitation.device-enrollment-terminal-session.v1\0".to_vec();
    transcript.extend_from_slice(digest);
    transcript.extend_from_slice(invitation.as_str().as_bytes());
    let digest = hash(&transcript);
    let mut id = [0; 16];
    id.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(id)
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

/// Each variant retains its protocol-specific owner; recovery has no active
/// request/response admission authority.
enum IssuedNoticeWindowCapability<'a> {
    Active(&'a EnrollmentWindowCapability),
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
            Self::Active(window) => window.map_run_error(stage, source),
            Self::Cancelled(window) => window.map_run_error(stage, source),
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
        &'a EnrollmentWindowCapability,
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
                .require_admitted_notice_owner(admitted, &effects)
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
pub(super) struct SignedCancelledNotice {
    bytes: Vec<u8>,
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
    window: &EnrollmentWindowCapability,
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
    issued: &RetainedEnrollmentVmControl,
    notice: &SignedCancelledNotice,
    window: &EnrollmentWindowCapability,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<()> {
    let validity = window
        .issued_notice_validity_child(effects.as_ref(), issued)
        .await
        .map_err(AgentError::from)?;
    validity
        .execute(effects.as_ref(), || {
            Box::pin(send_signed_cancelled_notice(
                effects.clone(),
                issued,
                IssuedNoticeWindowCapability::Active(&validity),
                notice.bytes.clone(),
                slot,
            ))
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
async fn send_signed_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    issued: &RetainedEnrollmentVmControl,
    window: IssuedNoticeWindowCapability<'_>,
    bytes: Vec<u8>,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<()> {
    window.require_owner(issued, effects.as_ref())?;
    issued
        .require_runtime_owner(effects.as_ref())
        .map_err(AgentError::from)?;
    let session = slot.insert(
        open_notice_session(
            effects.clone(),
            NoticeIngressCapability::Issued(
                issued,
                match &window {
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
        if super::vm_loop::handle_invitation_vm_wait_status(
            round.host_wait_status,
            false,
            "terminal notice send timed out",
            "terminal notice send cancelled",
        )?
        .is_some()
            || super::vm_loop::handle_invitation_vm_step(
                round.step,
                "terminal notice sender became stuck",
            )?
        {
            return Ok(());
        }
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "CancelledEnrollmentNoticeWindowCapability",
    family = "runtime_helper"
)]
pub(crate) async fn execute_recovered_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    issued: RetainedEnrollmentVmControl,
    runner: CeremonyRunner,
    window: crate::runtime::services::enrollment_window::CancelledEnrollmentNoticeWindowCapability,
) -> AgentResult<()> {
    window
        .require_issued_owner(&issued, effects.as_ref())
        .map_err(AgentError::from)?;
    // Sign while the cancelled provisional generation is still held, then
    // release it at once: a peer that never answers must not keep later
    // enrollments refused for the whole notice window (Task 80). Retries only
    // resend the already signed notice.
    let bytes = sign_cancelled_notice_bytes(
        &effects,
        &issued,
        &runner,
        window.cancelled(),
        &IssuedNoticeWindowCapability::Cancelled(&window),
    )
    .await?;
    runner
        .retire_failed_enrollment_generation(&issued.manifest().ceremony)
        .await
        .map_err(AgentError::from)?;
    let mut slot = None;
    loop {
        let attempt = window
            .execute(effects.as_ref(), || {
                Box::pin(send_recovered_cancelled_notice(
                    effects.clone(),
                    &issued,
                    &window,
                    bytes.clone(),
                    &mut slot,
                ))
            })
            .await
            .map_err(|source| window.map_run_error("recovered cancellation notice", source));
        match super::device_enrollment::finish_enrollment_vm_slot(attempt, slot.take()).await {
            Ok(()) => return Ok(()),
            Err(error)
                if super::device_enrollment::enrollment_notice_peer_unreachable(
                    &error,
                    issued.manifest().subject,
                ) =>
            {
                window
                    .retry_delay(effects.as_ref(), 5_000)
                    .await
                    .map_err(|source| {
                        window.map_run_error(
                            "recovered cancellation notice retry",
                            TimeoutRunError::Timeout(source),
                        )
                    })?;
            }
            Err(error) => return Err(error),
        }
    }
}

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
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "admitted_original_window_terminal_notice",
    capability_type = EnrollmentWindowCapability,
    family = "runtime_helper"
)]
pub(super) async fn receive_cancelled_notice(
    effects: Arc<AuraEffectSystem>,
    admitted: &AdmittedEnrollmentManifest,
    window: &EnrollmentWindowCapability,
    slot: &mut Option<OwnedVmSession>,
) -> AgentResult<VerifiedEnrollmentFailureCapability> {
    window
        .require_admitted_notice_owner(admitted, &effects)
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
                    continue
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
            session.inject_blocked_receive(&blocked).map_err(stage)?;
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

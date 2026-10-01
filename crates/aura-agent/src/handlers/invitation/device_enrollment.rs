use super::vm_loop::{
    handle_invitation_vm_step, handle_invitation_vm_wait_status, invitation_invalid_error,
    map_invitation_vm_timeout,
};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use std::collections::BTreeMap;

/// How long the initiator waits for the new device to import the code and
/// accept. A person carries the code between devices, so this is far longer
/// than a VM round.
const DEVICE_ENROLLMENT_ACCEPT_WINDOW_MS: u64 = 600_000;

/// Pause between initiator attempts while waiting for the new device.
const DEVICE_ENROLLMENT_RETRY_DELAY_MS: u64 = 5_000;

pub(super) struct InvitationDeviceEnrollmentHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationDeviceEnrollmentHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    /// Device-scoped roles for the enrollment choreography.
    ///
    /// The invitee runtime switches to the subject authority before accepting,
    /// so authority-scoped roles would route the initiator's request to the
    /// invitee's former (prepared) authority. Both devices belong to the
    /// subject authority and are named in the invitation.
    fn enrollment_roles(
        invitation: &Invitation,
    ) -> AgentResult<(ChoreographicRole, ChoreographicRole)> {
        let InvitationType::DeviceEnrollment {
            subject_authority,
            initiator_device_id,
            device_id,
            ..
        } = &invitation.invitation_type
        else {
            return Err(AgentError::internal(
                "Expected DeviceEnrollment invitation type".to_string(),
            ));
        };
        let initiator = ChoreographicRole::new(
            *initiator_device_id,
            *subject_authority,
            RoleIndex::new(0).expect("role index"),
        );
        let invitee = ChoreographicRole::new(
            *device_id,
            *subject_authority,
            RoleIndex::new(1).expect("role index"),
        );
        Ok((initiator, invitee))
    }

    pub(super) async fn resolve_device_enrollment_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<DeviceEnrollmentInvitation>> {
        let own_id = self.handler.context.authority.authority_id();

        if let Some(inv) = self
            .handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await
        {
            if let InvitationType::DeviceEnrollment {
                subject_authority,
                device_id,
                nickname_suggestion: _,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
                baseline_tree_ops,
                ..
            } = &inv.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority: *subject_authority,
                    device_id: *device_id,
                    pending_epoch: *pending_epoch,
                    key_package: key_package.clone(),
                    threshold_config: threshold_config.clone(),
                    public_key_package: public_key_package.clone(),
                    baseline_tree_ops: baseline_tree_ops.clone(),
                }));
            }
        }

        if let Some(stored) =
            InvitationHandler::load_imported_invitation(effects, own_id, invitation_id, None).await
        {
            let shareable = stored.shareable;
            if let InvitationType::DeviceEnrollment {
                subject_authority,
                device_id,
                nickname_suggestion: _,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
                baseline_tree_ops,
                ..
            } = shareable.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority,
                    device_id,
                    pending_epoch,
                    key_package,
                    threshold_config,
                    public_key_package,
                    baseline_tree_ops,
                }));
            }
        }

        Ok(None)
    }

    /// Run the initiator until the new device accepts or the acceptance window
    /// closes. The new device only becomes reachable as a device of this
    /// authority once a person imports the code on it, so each attempt re-opens
    /// the session and re-sends the request.
    pub(super) async fn execute_device_enrollment_initiator(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
    ) -> AgentResult<()> {
        let now_ms = |effects: Arc<AuraEffectSystem>| async move {
            PhysicalTimeEffects::physical_time(effects.as_ref())
                .await
                .map(|time| time.ts_ms)
                .unwrap_or_default()
        };
        let deadline = now_ms(effects.clone())
            .await
            .saturating_add(DEVICE_ENROLLMENT_ACCEPT_WINDOW_MS);
        loop {
            let attempt = self
                .run_device_enrollment_initiator_attempt(
                    effects.clone(),
                    invitation,
                    ceremony_runner.clone(),
                )
                .await;
            match attempt {
                Ok(()) => return Ok(()),
                Err(error) if now_ms(effects.clone()).await < deadline => {
                    tracing::debug!(
                        invitation_id = %invitation.invitation_id,
                        error = %error,
                        "device enrollment initiator attempt ended; retrying until the new device accepts"
                    );
                    let _ = PhysicalTimeEffects::sleep_ms(
                        effects.as_ref(),
                        DEVICE_ENROLLMENT_RETRY_DELAY_MS,
                    )
                    .await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn run_device_enrollment_initiator_attempt(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
    ) -> AgentResult<()> {
        let (subject_authority, ceremony_id, pending_epoch, device_id) =
            match &invitation.invitation_type {
                InvitationType::DeviceEnrollment {
                    subject_authority,
                    ceremony_id,
                    pending_epoch,
                    device_id,
                    ..
                } => (
                    *subject_authority,
                    ceremony_id.clone(),
                    *pending_epoch,
                    *device_id,
                ),
                _ => {
                    return Err(AgentError::internal(
                        "Expected DeviceEnrollment invitation type".to_string(),
                    ));
                }
            };

        let request = DeviceEnrollmentRequestWrapper(DeviceEnrollmentRequest {
            invitation_id: invitation.invitation_id.clone(),
            subject_authority,
            ceremony_id: ceremony_id.clone(),
            pending_epoch,
            device_id,
        });
        let invitation_id = invitation.invitation_id.clone();
        let ceremony_id_for_confirm = ceremony_id.clone();
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let (initiator_role, invitee_role) = Self::enrollment_roles(invitation)?;
        let roles = vec![initiator_role, invitee_role];
        let peer_roles = BTreeMap::from([("Invitee".to_string(), invitee_role)]);
        let manifest = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::local_types();
        let confirm = DeviceEnrollmentConfirmWrapper(DeviceEnrollmentConfirm {
            invitation_id: invitation_id.clone(),
            ceremony_id: ceremony_id_for_confirm.clone(),
            established: true,
            new_epoch: Some(pending_epoch),
        });

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
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
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(
                to_vec(&request).map_err(|error| AgentError::internal(error.to_string()))?,
            );
            session.queue_send_bytes(
                to_vec(&confirm).map_err(|error| AgentError::internal(error.to_string()))?,
            );

            let budget = invitation_timeout_budget(
                effects.as_ref(),
                "device_enrollment_initiator_vm",
                INVITATION_VM_LOOP_TIMEOUT_MS,
            )
            .await?;

            let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
                loop {
                    let round = session
                        .advance_round_until_receive(
                            "Initiator",
                            &peer_roles,
                            InvitationHandler::is_transport_no_message,
                        )
                        .await
                        .map_err(|error| AgentError::internal(error.to_string()))?;

                    if let Some(blocked) = round.blocked_receive {
                        // The only message the initiator receives is the invitee's
                        // acceptance: verify it before counting the new device.
                        let accept: DeviceEnrollmentAcceptWrapper = from_slice(&blocked.payload)
                            .map_err(|error| {
                                invitation_invalid_error(
                                    "malformed device enrollment acceptance",
                                    error,
                                )
                            })?;
                        verify_device_enrollment_acceptance(
                            effects.as_ref(),
                            invitation,
                            subject_authority,
                            &ceremony_id,
                            device_id,
                            &accept.0,
                        )
                        .await?;
                        ceremony_runner
                            .record_local_response(
                                &ceremony_id,
                                aura_core::threshold::ParticipantIdentity::device(device_id),
                            )
                            .await
                            .map_err(|error| AgentError::internal(error.to_string()))?;
                        session
                            .inject_blocked_receive(&blocked)
                            .map_err(|error| AgentError::internal(error.to_string()))?;
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

                    if handle_invitation_vm_step(
                        round.step,
                        "device enrollment initiator VM became stuck without a pending receive",
                    )? {
                        break Ok(());
                    }
                }
            })
            .await
            .map_err(|error| {
                map_invitation_vm_timeout("device enrollment initiator VM", &budget, error)
            });

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    pub(super) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        // Sign as the invited authority: by the time the invitee accepts, its
        // runtime may already have switched to the subject authority, but the
        // initiator only trusts the authority it invited.
        let authority_id = invitation.receiver_id;
        let (subject_authority, ceremony_id, device_id) = match &invitation.invitation_type {
            InvitationType::DeviceEnrollment {
                subject_authority,
                ceremony_id,
                device_id,
                ..
            } => (*subject_authority, ceremony_id.clone(), *device_id),
            _ => {
                return Err(AgentError::internal(
                    "Expected DeviceEnrollment invitation type".to_string(),
                ));
            }
        };

        // Sign the acceptance transcript so the initiator can verify that the
        // invited authority (not an on-path party) accepted this enrollment.
        let transcript = DeviceEnrollmentAcceptanceTranscript {
            invitation,
            acceptor_id: authority_id,
            subject_authority,
            ceremony_id: ceremony_id.clone(),
            device_id,
        };
        let signature =
            sign_invitation_acceptance_transcript(effects.as_ref(), authority_id, &transcript)
                .await?;
        let accept = DeviceEnrollmentAcceptWrapper(DeviceEnrollmentAccept {
            invitation_id: invitation.invitation_id.clone(),
            ceremony_id,
            device_id,
            acceptor_id: authority_id,
            signature,
        });
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let (initiator_role, invitee_role) = Self::enrollment_roles(invitation)?;
        let roles = vec![initiator_role, invitee_role];
        let peer_roles = BTreeMap::from([("Initiator".to_string(), initiator_role)]);
        let manifest = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::local_types();

        let mut session = open_owned_manifest_vm_session_admitted(
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
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(
            to_vec(&accept).map_err(|error| AgentError::internal(error.to_string()))?,
        );

        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "device_enrollment_invitee_vm",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;

        let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            loop {
                let round = session
                    .advance_round("Invitee", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
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
                    break Ok(());
                }

                if handle_invitation_vm_step(
                    round.step,
                    "device enrollment invitee VM became stuck without a pending receive",
                )? {
                    break Ok(());
                }
            }
        })
        .await
        .map_err(|error| map_invitation_vm_timeout("device enrollment invitee VM", &budget, error));

        let _ = session.close().await;
        loop_result
    }
}

/// Verify an invitee's device-enrollment acceptance before it is counted.
///
/// The acceptance must come from the invited authority, match this
/// invitation, ceremony, and device, and carry a valid signature over the
/// acceptance transcript.
pub(super) async fn verify_device_enrollment_acceptance(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
    subject_authority: AuthorityId,
    ceremony_id: &CeremonyId,
    device_id: DeviceId,
    accept: &DeviceEnrollmentAccept,
) -> AgentResult<()> {
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
    let transcript = DeviceEnrollmentAcceptanceTranscript {
        invitation,
        acceptor_id: accept.acceptor_id,
        subject_authority,
        ceremony_id: ceremony_id.clone(),
        device_id,
    };
    verify_invitation_acceptance_signature(
        effects,
        accept.acceptor_id,
        &transcript,
        &accept.signature,
    )
    .await
}

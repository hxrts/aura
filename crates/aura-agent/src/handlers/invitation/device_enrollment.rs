use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use crate::runtime::vm_host_bridge::AuraVmHostWaitStatus;
use std::collections::BTreeMap;

pub(super) struct InvitationDeviceEnrollmentHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationDeviceEnrollmentHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    fn role(authority_id: AuthorityId) -> ChoreographicRole {
        ChoreographicRole::for_authority(authority_id, RoleIndex::new(0).expect("role index"))
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
                initiator_device_id,
                device_id,
                nickname_suggestion: _,
                ceremony_id,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
                baseline_tree_ops,
            } = &inv.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority: *subject_authority,
                    initiator_device_id: *initiator_device_id,
                    device_id: *device_id,
                    ceremony_id: ceremony_id.clone(),
                    pending_epoch: *pending_epoch,
                    key_package: key_package.clone(),
                    threshold_config: threshold_config.clone(),
                    public_key_package: public_key_package.clone(),
                    baseline_tree_ops: baseline_tree_ops.clone(),
                }));
            }
        }

        if let Some(shareable) =
            InvitationHandler::load_imported_invitation(effects, own_id, invitation_id).await
        {
            if let InvitationType::DeviceEnrollment {
                subject_authority,
                initiator_device_id,
                device_id,
                nickname_suggestion: _,
                ceremony_id,
                pending_epoch,
                key_package,
                threshold_config,
                public_key_package,
                baseline_tree_ops,
            } = shareable.invitation_type
            {
                return Ok(Some(DeviceEnrollmentInvitation {
                    subject_authority,
                    initiator_device_id,
                    device_id,
                    ceremony_id,
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

    pub(super) async fn execute_device_enrollment_initiator(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let authority_id = self.handler.context.authority.authority_id();
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
        let roles = vec![Self::role(authority_id), Self::role(invitation.receiver_id)];
        let peer_roles =
            BTreeMap::from([("Invitee".to_string(), Self::role(invitation.receiver_id))]);
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
            session.queue_send_bytes(to_vec(&request).map_err(|error| {
                AgentError::internal(format!("device enrollment request encode failed: {error}"))
            })?);
            session.queue_send_bytes(to_vec(&confirm).map_err(|error| {
                AgentError::internal(format!("device enrollment confirm encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = session
                    .advance_round_until_receive(
                        "Initiator",
                        &peer_roles,
                        InvitationHandler::is_transport_no_message,
                    )
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Deferred => break Ok(()),
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "device enrollment initiator VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "device enrollment initiator VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "device enrollment initiator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

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
        let authority_id = self.handler.context.authority.authority_id();
        let (ceremony_id, device_id) = match &invitation.invitation_type {
            InvitationType::DeviceEnrollment {
                ceremony_id,
                device_id,
                ..
            } => (ceremony_id.clone(), *device_id),
            _ => {
                return Err(AgentError::internal(
                    "Expected DeviceEnrollment invitation type".to_string(),
                ));
            }
        };

        let accept = DeviceEnrollmentAcceptWrapper(DeviceEnrollmentAccept {
            invitation_id: invitation.invitation_id.clone(),
            ceremony_id,
            device_id,
        });
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let roles = vec![Self::role(invitation.sender_id), Self::role(authority_id)];
        let peer_roles =
            BTreeMap::from([("Initiator".to_string(), Self::role(invitation.sender_id))]);
        let manifest = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::device_enrollment::telltale_session_types_invitation_device_enrollment::vm_artifacts::local_types();

        let result = async {
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
            session.queue_send_bytes(to_vec(&accept).map_err(|error| {
                AgentError::internal(format!("device enrollment accept encode failed: {error}"))
            })?);

            let loop_result = loop {
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

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "device enrollment invitee VM timed out while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "device enrollment invitee VM cancelled while waiting for receive"
                                .to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "device enrollment invitee VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            };

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }
}

use super::*;

pub(super) struct InvitationDeviceEnrollmentHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationDeviceEnrollmentHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
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
        use crate::core::AgentError;

        let authority_id = self.handler.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(DeviceEnrollmentRole::Initiator, authority_id);
        role_map.insert(DeviceEnrollmentRole::Invitee, invitation.receiver_id);

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

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            DeviceEnrollmentRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if InvitationHandler::type_matches(request_ctx.type_name, "DeviceEnrollmentRequest") {
                return Some(Box::new(request.clone()));
            }

            if InvitationHandler::type_matches(request_ctx.type_name, "DeviceEnrollmentConfirm") {
                let confirm = DeviceEnrollmentConfirmWrapper(DeviceEnrollmentConfirm {
                    invitation_id: invitation_id.clone(),
                    ceremony_id: ceremony_id_for_confirm.clone(),
                    established: true,
                    new_epoch: Some(pending_epoch),
                });
                return Some(Box::new(confirm));
            }

            None
        });

        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment start failed: {e}")))?;

        let result =
            device_enrollment_execute_as(DeviceEnrollmentRole::Initiator, &mut adapter).await;

        let _ = adapter.end_session().await;
        match result {
            Ok(()) => Ok(()),
            Err(err) if InvitationHandler::is_transport_no_message(&err) => Ok(()),
            Err(err) => Err(AgentError::internal(format!(
                "device enrollment choreography failed: {err}"
            ))),
        }
    }

    pub(super) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(DeviceEnrollmentRole::Initiator, invitation.sender_id);
        role_map.insert(DeviceEnrollmentRole::Invitee, authority_id);

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

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            DeviceEnrollmentRole::Invitee,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if InvitationHandler::type_matches(request.type_name, "DeviceEnrollmentAccept") {
                return Some(Box::new(accept.clone()));
            }
            None
        });

        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment start failed: {e}")))?;

        let result = device_enrollment_execute_as(DeviceEnrollmentRole::Invitee, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("device enrollment failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }
}

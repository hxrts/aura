use super::*;

pub(super) struct InvitationGuardianHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationGuardianHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    pub(super) async fn execute_guardian_invitation_principal(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(GuardianInvitationRole::Principal, authority_id);
        role_map.insert(GuardianInvitationRole::Guardian, invitation.receiver_id);

        let role_description = invitation
            .message
            .clone()
            .unwrap_or_else(|| "guardian invitation".to_string());
        let request = GuardianInvitationRequest(GuardianRequest {
            invitation_id: invitation.invitation_id.clone(),
            principal: authority_id,
            role_description,
            recovery_capabilities: Vec::new(),
            expires_at_ms: invitation.expires_at,
        });
        let invitation_id = invitation.invitation_id.clone();

        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            GuardianInvitationRole::Principal,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if InvitationHandler::type_matches(request_ctx.type_name, "GuardianRequest") {
                return Some(Box::new(request.clone()));
            }

            if InvitationHandler::type_matches(request_ctx.type_name, "GuardianConfirm") {
                let confirm = GuardianInvitationConfirm(GuardianConfirm {
                    invitation_id: invitation_id.clone(),
                    established: true,
                    relationship_id: None,
                });
                return Some(Box::new(confirm));
            }

            None
        });

        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite start failed: {e}")))?;

        let result = guardian_execute_as(GuardianInvitationRole::Principal, &mut adapter).await;

        let _ = adapter.end_session().await;
        match result {
            Ok(()) => Ok(()),
            Err(err) if InvitationHandler::is_transport_no_message(&err) => Ok(()),
            Err(err) => Err(AgentError::internal(format!(
                "guardian invite failed: {err}"
            ))),
        }
    }

    pub(super) async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.context.authority.authority_id();
        let mut role_map = HashMap::new();
        role_map.insert(GuardianInvitationRole::Principal, invitation.sender_id);
        role_map.insert(GuardianInvitationRole::Guardian, authority_id);

        let accept = GuardianInvitationAccept(GuardianAccept {
            invitation_id: invitation.invitation_id.clone(),
            signature: Vec::new(),
            recovery_public_key: Vec::new(),
        });
        let mut adapter = AuraProtocolAdapter::new(
            effects.clone(),
            authority_id,
            GuardianInvitationRole::Guardian,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if InvitationHandler::type_matches(request.type_name, "GuardianAccept") {
                return Some(Box::new(accept.clone()));
            }
            None
        });

        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite start failed: {e}")))?;

        let result = guardian_execute_as(GuardianInvitationRole::Guardian, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("guardian invite failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }
}

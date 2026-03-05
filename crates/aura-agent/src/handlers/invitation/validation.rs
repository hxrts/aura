use super::*;

pub(super) struct InvitationValidationHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationValidationHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    pub(super) async fn validate_cached_invitation_accept(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
        now_ms: u64,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                status = ?invitation.status,
                sender = %invitation.sender_id,
                "Validating invitation for accept"
            );

            if !invitation.is_pending() {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    status = ?invitation.status,
                    sender = %invitation.sender_id,
                    "Invitation is not pending"
                );
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?}, sender: {})",
                    invitation_id, invitation.status, invitation.sender_id
                )));
            }

            if invitation.is_expired(now_ms) {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    expires_at = ?invitation.expires_at,
                    now_ms = now_ms,
                    "Invitation has expired"
                );
                return Err(AgentError::invalid(format!(
                    "Invitation {} has expired (expires_at: {:?}, now: {})",
                    invitation_id, invitation.expires_at, now_ms
                )));
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "Invitation not found in cache or storage, proceeding anyway"
            );
        }

        Ok(())
    }

    pub(super) async fn validate_cached_invitation_decline(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            if !invitation.is_pending() {
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?})",
                    invitation_id, invitation.status
                )));
            }
        }

        Ok(())
    }

    pub(super) async fn validate_cached_invitation_cancel(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        if let Some(invitation) = self
            .handler
            .get_invitation_with_storage(effects, invitation_id)
            .await
        {
            if !invitation.is_pending() {
                return Err(AgentError::invalid(format!(
                    "Invitation {} is not pending (status: {:?})",
                    invitation_id, invitation.status
                )));
            }

            if invitation.sender_id != self.handler.context.authority.authority_id() {
                return Err(AgentError::invalid(format!(
                    "Only sender can cancel invitation {}",
                    invitation_id
                )));
            }
        }

        Ok(())
    }
}

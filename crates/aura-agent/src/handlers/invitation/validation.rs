use super::*;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum InvitationValidationError {
    #[error("invitation {invitation_id} is not pending")]
    NotPending {
        invitation_id: InvitationId,
        status: InvitationStatus,
    },
    #[error("invitation {invitation_id} expired")]
    Expired { invitation_id: InvitationId },
    #[error("invitation {invitation_id} not found")]
    NotFound { invitation_id: InvitationId },
}

impl From<InvitationValidationError> for AgentError {
    fn from(error: InvitationValidationError) -> Self {
        AgentError::Aura(aura_core::AuraError::Invalid {
            message: error.to_string(),
            source: Some(std::sync::Arc::new(error)),
        })
    }
}

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
                return Err(InvitationValidationError::NotPending {
                    invitation_id: invitation_id.clone(),
                    status: invitation.status,
                }
                .into());
            }

            if invitation.is_expired(now_ms) {
                tracing::warn!(
                    invitation_id = %invitation_id,
                    expires_at = ?invitation.expires_at,
                    now_ms = now_ms,
                    "Invitation has expired"
                );
                return Err(InvitationValidationError::Expired {
                    invitation_id: invitation_id.clone(),
                }
                .into());
            }
        } else {
            tracing::info!(
                invitation_id = %invitation_id,
                "Rejecting invitation accept because the invitation is not present in cache or storage"
            );
            return Err(InvitationValidationError::NotFound {
                invitation_id: invitation_id.clone(),
            }
            .into());
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
                return Err(InvitationValidationError::NotPending {
                    invitation_id: invitation_id.clone(),
                    status: invitation.status,
                }
                .into());
            }
        }

        Ok(())
    }
}

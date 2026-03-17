use super::*;

pub(super) struct InvitationCacheHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationCacheHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    pub(super) fn imported_invitation_key(
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> String {
        format!(
            "{}/{}/{}",
            InvitationHandler::IMPORTED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id.as_str()
        )
    }

    pub(super) fn imported_invitation_prefix(authority_id: AuthorityId) -> String {
        format!(
            "{}/{}/",
            InvitationHandler::IMPORTED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid()
        )
    }

    pub(super) fn created_invitation_key(
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> String {
        format!(
            "{}/{}/{}",
            InvitationHandler::CREATED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid(),
            invitation_id.as_str()
        )
    }

    pub(super) async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let key = Self::created_invitation_key(authority_id, &invitation.invitation_id);
        let bytes = serde_json::to_vec(invitation)
            .map_err(|e| crate::core::AgentError::internal(e.to_string()))?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        Ok(())
    }

    pub(super) async fn load_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        let key = Self::created_invitation_key(authority_id, invitation_id);
        let Ok(Some(bytes)) = effects.retrieve(&key).await else {
            return None;
        };
        serde_json::from_slice::<Invitation>(&bytes).ok()
    }

    pub(super) async fn persist_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        shareable: &ShareableInvitation,
    ) -> AgentResult<()> {
        let key = Self::imported_invitation_key(authority_id, &shareable.invitation_id);
        let bytes = serde_json::to_vec(shareable)
            .map_err(|e| crate::core::AgentError::internal(e.to_string()))?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))?;
        Ok(())
    }

    pub(super) async fn load_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
    ) -> Option<ShareableInvitation> {
        let key = Self::imported_invitation_key(authority_id, invitation_id);
        let Ok(Some(bytes)) = effects.retrieve(&key).await else {
            return None;
        };
        serde_json::from_slice::<ShareableInvitation>(&bytes).ok()
    }

    pub(super) async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        self.handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await
    }

    pub(super) async fn get_invitation_with_storage(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        if let Some(inv) = self
            .handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await
        {
            return Some(inv);
        }

        let own_id = self.handler.context.authority.authority_id();

        if let Some(inv) = Self::load_created_invitation(effects, own_id, invitation_id).await {
            return Some(inv);
        }

        if let Some(shareable) =
            Self::load_imported_invitation(effects, own_id, invitation_id).await
        {
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id: self.handler.context.effect_context.context_id(),
                sender_id: shareable.sender_id,
                receiver_id: own_id,
                invitation_type: shareable.invitation_type,
                status: InvitationStatus::Pending,
                created_at: 0,
                expires_at: shareable.expires_at,
                message: shareable.message,
            });
        }

        None
    }
}

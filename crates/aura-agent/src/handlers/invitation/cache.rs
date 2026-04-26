use super::*;

pub(super) struct InvitationCacheHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationCacheHandler<'a> {
    pub(super) fn invitation_status_rank(status: InvitationStatus) -> u8 {
        match status {
            InvitationStatus::Pending => 0,
            InvitationStatus::Accepted
            | InvitationStatus::Declined
            | InvitationStatus::Expired
            | InvitationStatus::Cancelled => 1,
        }
    }

    pub(super) fn should_replace_invitation(existing: &Invitation, candidate: &Invitation) -> bool {
        let existing_rank = Self::invitation_status_rank(existing.status.clone());
        let candidate_rank = Self::invitation_status_rank(candidate.status.clone());
        candidate_rank > existing_rank
            || (candidate_rank == existing_rank
                && candidate.created_at > existing.created_at
                && candidate.status == existing.status)
    }

    pub(super) fn merge_invitation(
        invitations: &mut HashMap<InvitationId, Invitation>,
        candidate: Invitation,
    ) {
        match invitations.entry(candidate.invitation_id.clone()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if Self::should_replace_invitation(entry.get(), &candidate) {
                    entry.insert(candidate);
                }
            }
        }
    }

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

    pub(super) fn created_invitation_prefix(authority_id: AuthorityId) -> String {
        format!(
            "{}/{}/",
            InvitationHandler::CREATED_INVITATION_STORAGE_PREFIX,
            authority_id.uuid()
        )
    }

    fn secret_payload_location(
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
        kind: &'static str,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "invitation_secret_payloads",
            authority_id.to_string(),
            format!("{kind}:{}", invitation_id.as_str()),
        )
    }

    fn has_device_enrollment_payload(invitation_type: &InvitationType) -> bool {
        matches!(invitation_type, InvitationType::DeviceEnrollment { .. })
    }

    fn redact_device_enrollment_payload(invitation: &Invitation) -> Invitation {
        let mut redacted = invitation.clone();
        if let InvitationType::DeviceEnrollment {
            key_package,
            threshold_config,
            public_key_package,
            baseline_tree_ops,
            ..
        } = &mut redacted.invitation_type
        {
            key_package.clear();
            threshold_config.clear();
            public_key_package.clear();
            baseline_tree_ops.clear();
        }
        redacted
    }

    fn redact_imported_device_enrollment_payload(
        invitation: &StoredImportedInvitation,
    ) -> StoredImportedInvitation {
        let mut redacted = invitation.clone();
        if let InvitationType::DeviceEnrollment {
            key_package,
            threshold_config,
            public_key_package,
            baseline_tree_ops,
            ..
        } = &mut redacted.shareable.invitation_type
        {
            key_package.clear();
            threshold_config.clear();
            public_key_package.clear();
            baseline_tree_ops.clear();
        }
        redacted
    }

    async fn secure_store_secret_payload(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
        kind: &'static str,
        bytes: &[u8],
    ) -> AgentResult<()> {
        let location = Self::secret_payload_location(authority_id, invitation_id, kind);
        effects
            .secure_store(
                &location,
                bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| crate::core::AgentError::effects(e.to_string()))
    }

    async fn secure_retrieve_secret_payload(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation_id: &InvitationId,
        kind: &'static str,
    ) -> Option<Vec<u8>> {
        let location = Self::secret_payload_location(authority_id, invitation_id, kind);
        effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .ok()
    }

    pub(super) async fn persist_created_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let key = Self::created_invitation_key(authority_id, &invitation.invitation_id);
        let regular_invitation = if Self::has_device_enrollment_payload(&invitation.invitation_type)
        {
            let secure_bytes = serde_json::to_vec(invitation)
                .map_err(|e| crate::core::AgentError::internal(e.to_string()))?;
            Self::secure_store_secret_payload(
                effects,
                authority_id,
                &invitation.invitation_id,
                "created",
                &secure_bytes,
            )
            .await?;
            Self::redact_device_enrollment_payload(invitation)
        } else {
            invitation.clone()
        };
        let bytes = serde_json::to_vec(&regular_invitation)
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
        let invitation = serde_json::from_slice::<Invitation>(&bytes).ok()?;
        if Self::has_device_enrollment_payload(&invitation.invitation_type) {
            if let Some(secure_bytes) = Self::secure_retrieve_secret_payload(
                effects,
                authority_id,
                invitation_id,
                "created",
            )
            .await
            {
                return serde_json::from_slice::<Invitation>(&secure_bytes).ok();
            }
        }
        Some(invitation)
    }

    pub(super) async fn persist_imported_invitation(
        effects: &AuraEffectSystem,
        authority_id: AuthorityId,
        invitation: &StoredImportedInvitation,
    ) -> AgentResult<()> {
        let key = Self::imported_invitation_key(authority_id, &invitation.invitation_id);
        let regular_invitation =
            if Self::has_device_enrollment_payload(&invitation.shareable.invitation_type) {
                let secure_bytes = serde_json::to_vec(invitation)
                    .map_err(|e| crate::core::AgentError::internal(e.to_string()))?;
                Self::secure_store_secret_payload(
                    effects,
                    authority_id,
                    &invitation.invitation_id,
                    "imported",
                    &secure_bytes,
                )
                .await?;
                Self::redact_imported_device_enrollment_payload(invitation)
            } else {
                invitation.clone()
            };
        let bytes = serde_json::to_vec(&regular_invitation)
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
        preserved: Option<&Invitation>,
    ) -> Option<StoredImportedInvitation> {
        let key = Self::imported_invitation_key(authority_id, invitation_id);
        let Ok(Some(bytes)) = effects.retrieve(&key).await else {
            return None;
        };
        let stored = Self::parse_imported_invitation_bytes(&bytes, preserved)?;
        if Self::has_device_enrollment_payload(&stored.shareable.invitation_type) {
            if let Some(secure_bytes) = Self::secure_retrieve_secret_payload(
                effects,
                authority_id,
                invitation_id,
                "imported",
            )
            .await
            {
                return Self::parse_imported_invitation_bytes(&secure_bytes, preserved);
            }
        }
        Some(stored)
    }

    pub(super) fn parse_imported_invitation_bytes(
        bytes: &[u8],
        _preserved: Option<&Invitation>,
    ) -> Option<StoredImportedInvitation> {
        serde_json::from_slice::<StoredImportedInvitation>(bytes).ok()
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
        let own_id = self.handler.context.authority.authority_id();
        let mut best = self
            .handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await;

        if let Some(inv) = Self::load_created_invitation(effects, own_id, invitation_id).await {
            match &best {
                Some(existing) if !Self::should_replace_invitation(existing, &inv) => {}
                _ => best = Some(inv),
            }
        }

        if let Some(stored) =
            Self::load_imported_invitation(effects, own_id, invitation_id, best.as_ref()).await
        {
            let status = stored.status.clone();
            let created_at = stored.created_at;
            let shareable = stored.shareable;
            let context_id = match &shareable.invitation_type {
                InvitationType::Channel { .. } => {
                    match require_channel_invitation_context(
                        &shareable.invitation_id,
                        shareable.sender_id,
                        shareable.context_id,
                    ) {
                        Ok(context_id) => context_id,
                        Err(error) => {
                            tracing::warn!(
                                invitation_id = %shareable.invitation_id,
                                sender = %shareable.sender_id,
                                error = %error,
                                "Skipping cached imported channel invitation without authoritative context"
                            );
                            return best;
                        }
                    }
                }
                _ => self.handler.context.effect_context.context_id(),
            };
            let invitation = Invitation {
                invitation_id: shareable.invitation_id,
                context_id,
                sender_id: shareable.sender_id,
                receiver_id: own_id,
                invitation_type: shareable.invitation_type,
                status,
                created_at,
                expires_at: shareable.expires_at,
                receiver_nickname: None,
                message: shareable.message,
            };
            match &best {
                Some(existing) if !Self::should_replace_invitation(existing, &invitation) => {}
                _ => best = Some(invitation),
            }
        }

        best
    }
}

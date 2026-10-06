use super::*;

impl InvitationHandler {
    pub(super) async fn load_invitation_for_choreography(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> Option<Invitation> {
        if let Some(inv) = self.invitation_cache.get_invitation(invitation_id).await {
            return Some(inv);
        }

        let own_id = self.context.authority.authority_id();
        if let Some(inv) = Self::load_created_invitation(effects, own_id, invitation_id).await {
            return Some(inv);
        }

        if let Some(stored) =
            Self::load_imported_invitation(effects, own_id, invitation_id, None).await
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
                                "Skipping imported channel invitation choreography without authoritative context"
                            );
                            return None;
                        }
                    }
                }
                _ => self.context.effect_context.context_id(),
            };
            let now_ms = Self::best_effort_current_timestamp_ms(effects).await;
            return Some(Invitation {
                invitation_id: shareable.invitation_id,
                context_id,
                sender_id: shareable.sender_id,
                receiver_id: super::imported_invitation_receiver(
                    &shareable.invitation_type,
                    own_id,
                ),
                invitation_type: shareable.invitation_type,
                status,
                created_at: if created_at == 0 { now_ms } else { created_at },
                expires_at: shareable.expires_at,
                receiver_nickname: None,
                message: shareable.message,
            });
        }

        None
    }

    pub(super) fn invitation_session_id(invitation_id: &InvitationId) -> Uuid {
        let digest = hash(invitation_id.as_str().as_bytes());
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(bytes)
    }

    pub(super) fn is_transport_no_message(err: &ChoreographyError) -> bool {
        match err {
            ChoreographyError::Transport { source } => source
                .downcast_ref::<TransportError>()
                .is_some_and(|inner| {
                    matches!(
                        inner,
                        TransportError::NoMessage | TransportError::DestinationUnreachable { .. }
                    )
                }),
            _ => false,
        }
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    fn invitation_exchange_peer_roles(
        authority_id: AuthorityId,
        peer_id: AuthorityId,
    ) -> (ChoreographicRole, ChoreographicRole, Vec<ChoreographicRole>) {
        let sender_index = RoleIndex::new(0).expect("sender role index");
        let receiver_index = RoleIndex::new(0).expect("receiver role index");
        let local_role = ChoreographicRole::for_authority(authority_id, sender_index);
        let peer_role = ChoreographicRole::for_authority(peer_id, receiver_index);
        (local_role, peer_role, vec![local_role, peer_role])
    }

    #[cfg(feature = "choreo-backend-telltale-machine")]
    async fn execute_invitation_exchange_receiver_vm(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        accepted: bool,
    ) -> AgentResult<()> {
        let authority_id = self.context.authority.authority_id();
        let (_local_role, peer_role, roles) =
            Self::invitation_exchange_peer_roles(authority_id, invitation.sender_id);
        let _ = (effects, invitation, accepted, peer_role, roles);
        Err(AgentError::internal(
            "invitation exchange response requires a receiver signature; unsigned responses are disabled",
        ))
    }
    pub(super) async fn execute_invitation_exchange_receiver(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        accepted: bool,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_receiver_vm(effects, invitation, accepted)
            .await
    }

    pub(crate) async fn execute_guardian_invitation_principal(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationGuardianHandler::new(self)
            .execute_guardian_invitation_principal(effects, invitation)
            .await
    }

    pub(super) async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationGuardianHandler::new(self)
            .execute_guardian_invitation_guardian(effects, invitation)
            .await
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentWindowCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn execute_device_enrollment_initiator_owned(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        ceremony_runner: crate::runtime::services::ceremony_runner::CeremonyRunner,
        budget: crate::runtime::services::enrollment_window::EnrollmentWindowCapability,
    ) -> AgentResult<()> {
        Box::pin(
            InvitationDeviceEnrollmentHandler::new(self).execute_device_enrollment_initiator_owned(
                effects,
                invitation,
                ceremony_runner,
                budget,
            ),
        )
        .await
    }

    pub(crate) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        tasks: &crate::task_registry::TaskGroup,
    ) -> AgentResult<()> {
        Box::pin(
            InvitationDeviceEnrollmentHandler::new(self)
                .execute_device_enrollment_invitee(effects, invitation, tasks),
        )
        .await
    }
}

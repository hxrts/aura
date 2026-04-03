use super::vm_loop::{
    handle_invitation_vm_step, handle_invitation_vm_wait_status, map_invitation_vm_timeout,
};
use super::*;

fn invitation_internal_error(prefix: &'static str, error: impl std::fmt::Display) -> AgentError {
    let mut detail = String::from(prefix);
    detail.push_str(": ");
    detail.push_str(&error.to_string());
    AgentError::internal(detail)
}

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
                receiver_id: own_id,
                invitation_type: shareable.invitation_type,
                status,
                created_at: if created_at == 0 { now_ms } else { created_at },
                expires_at: shareable.expires_at,
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

    fn build_invitation_offer(invitation: &Invitation) -> InvitationOffer {
        let mut material = Vec::new();
        material.extend_from_slice(invitation.invitation_id.as_str().as_bytes());
        material.extend_from_slice(&invitation.sender_id.to_bytes());
        material.extend_from_slice(&invitation.receiver_id.to_bytes());
        if let Some(expires_at) = invitation.expires_at {
            material.extend_from_slice(&expires_at.to_le_bytes());
        }
        let commitment_hash = hash(&material);

        InvitationOffer {
            invitation_id: invitation.invitation_id.clone(),
            invitation_type: invitation.invitation_type.clone(),
            sender: invitation.sender_id,
            message: invitation.message.clone(),
            expires_at_ms: invitation.expires_at,
            commitment: commitment_hash,
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
    async fn execute_invitation_exchange_sender_vm(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let authority_id = self.context.authority.authority_id();
        let (_local_role, peer_role, roles) =
            Self::invitation_exchange_peer_roles(authority_id, invitation.receiver_id);
        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        let offer = ExchangeInvitationOffer(Self::build_invitation_offer(invitation));
        let peer_roles = BTreeMap::from([("Receiver".to_string(), peer_role)]);
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "invitation_exchange_sender_vm",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;

        let manifest =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest();
        let global_type =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::global_type();
        let local_types =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::local_types();
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Sender",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(
            to_vec(&offer)
                .map_err(|error| invitation_internal_error("offer encode failed", error))?,
        );

        let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            loop {
                let round = session
                    .advance_round_until_receive(
                        "Sender",
                        &peer_roles,
                        Self::is_transport_no_message,
                    )
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    let response: ExchangeInvitationResponse = from_slice(&blocked.payload)
                        .map_err(|error| {
                            invitation_internal_error("invitation response decode failed", error)
                        })?;
                    if response.0.accepted {
                        if let InvitationType::Channel {
                            home_id,
                            nickname_suggestion,
                            ..
                        } = &invitation.invitation_type
                        {
                            let reactive = effects.reactive_handler();
                            let now_ms =
                                Self::best_effort_current_timestamp_ms(effects.as_ref()).await;
                            let home_name = require_channel_invitation_name(
                                *home_id,
                                nickname_suggestion.clone(),
                            )?;
                            app_signal_views::materialize_home_signal_for_channel_acceptance(
                                &reactive,
                                *home_id,
                                &home_name,
                                invitation.sender_id,
                                invitation.receiver_id,
                                invitation.context_id,
                                now_ms,
                            )
                            .await
                            .map_err(AgentError::runtime)?;
                        }
                    }
                    let status = if response.0.accepted {
                        aura_invitation::InvitationAckStatus::Accepted
                    } else {
                        aura_invitation::InvitationAckStatus::Declined
                    };
                    let ack = ExchangeInvitationAck(InvitationAck {
                        invitation_id: invitation.invitation_id.clone(),
                        success: true,
                        status,
                    });
                    session.queue_send_bytes(to_vec(&ack).map_err(|error| {
                        invitation_internal_error("invitation ack encode failed", error)
                    })?);
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
                    continue;
                }

                if handle_invitation_vm_wait_status(
                    round.host_wait_status,
                    true,
                    "invitation sender VM timed out while waiting for receive",
                    "invitation sender VM cancelled while waiting for receive",
                )?
                .is_some()
                {
                    break Ok(());
                }

                if handle_invitation_vm_step(
                    round.step,
                    "invitation sender VM became stuck without a pending receive",
                )? {
                    break Ok(());
                }
            }
        })
        .await
        .map_err(|error| map_invitation_vm_timeout("invitation sender VM", &budget, error));

        let _ = session.close().await;
        loop_result
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
        let session_id = Self::invitation_session_id(&invitation.invitation_id);
        let response = ExchangeInvitationResponse(InvitationResponse {
            invitation_id: invitation.invitation_id.clone(),
            accepted,
            message: None,
            signature: Vec::new(),
        });
        let mut response_queued = false;
        let peer_roles = BTreeMap::from([("Sender".to_string(), peer_role)]);
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "invitation_exchange_receiver_vm",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;

        let manifest =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::composition_manifest();
        let global_type =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::global_type();
        let local_types =
            aura_invitation::protocol::exchange::telltale_session_types_invitation::vm_artifacts::local_types();
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Receiver",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;

        let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            loop {
                let round = session
                    .advance_round("Receiver", &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                if let Some(blocked) = round.blocked_receive {
                    if !response_queued {
                        session.queue_send_bytes(to_vec(&response).map_err(|error| {
                            invitation_internal_error("invitation response encode failed", error)
                        })?);
                        response_queued = true;
                    }
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(|error| AgentError::internal(error.to_string()))?;
                    continue;
                }

                if handle_invitation_vm_wait_status(
                    round.host_wait_status,
                    false,
                    "invitation receiver VM timed out while waiting for receive",
                    "invitation receiver VM cancelled while waiting for receive",
                )?
                .is_some()
                {
                    break Ok(());
                }

                if handle_invitation_vm_step(
                    round.step,
                    "invitation receiver VM became stuck without a pending receive",
                )? {
                    break Ok(());
                }
            }
        })
        .await
        .map_err(|error| map_invitation_vm_timeout("invitation receiver VM", &budget, error));

        let _ = session.close().await;
        loop_result
    }

    pub(super) async fn execute_invitation_exchange_sender(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_sender_vm(effects, invitation)
            .await
    }

    pub(crate) async fn execute_channel_invitation_exchange_sender(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        self.execute_invitation_exchange_sender(effects, invitation)
            .await
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

    pub(super) async fn execute_guardian_invitation_principal(
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

    pub(crate) async fn execute_device_enrollment_initiator(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationDeviceEnrollmentHandler::new(self)
            .execute_device_enrollment_initiator(effects, invitation)
            .await
    }

    pub(crate) async fn execute_device_enrollment_invitee(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        InvitationDeviceEnrollmentHandler::new(self)
            .execute_device_enrollment_invitee(effects, invitation)
            .await
    }
}

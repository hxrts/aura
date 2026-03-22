use super::*;
use aura_journal::fact::RelationalFact;

const INVITATION_CHANNEL_JOIN_TIMEOUT_MS: u64 = 2_000;
const CHANNEL_ACCEPTANCE_PEER_CHANNEL_ATTEMPTS: usize = 6;
const CHANNEL_ACCEPTANCE_PEER_CHANNEL_BACKOFF_MS: u64 = 75;

pub(super) struct InvitationChannelHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationChannelHandler<'a> {
    async fn channel_checkpoint_exists(
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
    ) -> bool {
        aura_protocol::amp::get_channel_state(effects, context_id, channel_id)
            .await
            .is_ok()
    }

    async fn channel_has_participant(
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
        participant: AuthorityId,
    ) -> bool {
        aura_protocol::amp::list_channel_participants(effects, context_id, channel_id)
            .await
            .map(|participants| participants.contains(&participant))
            .unwrap_or(false)
    }

    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    pub(super) async fn notify_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<()> {
        let Some(invitation) = self
            .handler
            .load_invitation_for_choreography(effects, invitation_id)
            .await
        else {
            return Ok(());
        };

        let InvitationType::Channel { home_id, .. } = invitation.invitation_type else {
            return Ok(());
        };

        let acceptor_id = self.handler.context.authority.authority_id();
        if invitation.sender_id == acceptor_id {
            return Ok(());
        }
        self.ensure_sender_peer_channel(effects, invitation.sender_id, invitation.context_id)
            .await?;

        let acceptance = ChannelInvitationAcceptance {
            invitation_id: invitation.invitation_id.clone(),
            acceptor_id,
            context_id: invitation.context_id,
            channel_id: home_id,
        };
        let payload =
            serde_json::to_vec(&acceptance).map_err(|e| AgentError::internal(e.to_string()))?;

        let mut metadata = crate::handlers::shared::build_transport_metadata(
            CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE,
            [
                ("invitation-id", invitation.invitation_id.to_string()),
                ("acceptor-id", acceptor_id.to_string()),
                ("channel-id", home_id.to_string()),
                ("acceptor-device-id", effects.device_id().to_string()),
            ],
        );
        let acceptor_hint = effects.lan_transport().and_then(|transport| {
            transport
                .websocket_addrs()
                .first()
                .map(|addr| {
                    if addr.starts_with("ws://") || addr.starts_with("wss://") {
                        addr.clone()
                    } else {
                        format!("ws://{addr}")
                    }
                })
                .or_else(|| {
                    transport
                        .advertised_addrs()
                        .first()
                        .map(|addr| format!("tcp://{addr}"))
                })
        });
        if let Some(acceptor_hint) = acceptor_hint {
            metadata.insert("acceptor-addr".to_string(), acceptor_hint);
        }

        let envelope = TransportEnvelope {
            destination: invitation.sender_id,
            source: acceptor_id,
            context: invitation.context_id,
            payload,
            metadata,
            receipt: None,
        };

        attempt_network_send_envelope(
            effects,
            "channel invitation acceptance membership envelope send failed",
            envelope,
        )
        .await
        .map_err(|error| AgentError::effects(error.to_string()))
    }

    async fn ensure_sender_peer_channel(
        &self,
        effects: &AuraEffectSystem,
        sender_id: AuthorityId,
        invitation_context: ContextId,
    ) -> AgentResult<()> {
        let Some(rendezvous_manager) = effects.rendezvous_manager() else {
            return Err(AgentError::runtime(
                "channel invitation acceptance requires rendezvous manager".to_string(),
            ));
        };

        let authority = self.handler.context.authority.clone();
        let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
            .map_err(|error| AgentError::internal(error.to_string()))?
            .with_rendezvous_manager(rendezvous_manager.clone());
        for attempt in 0..CHANNEL_ACCEPTANCE_PEER_CHANNEL_ATTEMPTS {
            if effects
                .is_channel_established(invitation_context, sender_id)
                .await
            {
                return Ok(());
            }

            let result = handler
                .initiate_channel(effects, invitation_context, sender_id)
                .await
                .map_err(|error| AgentError::runtime(error.to_string()))?;
            if result.success
                && effects
                    .is_channel_established(invitation_context, sender_id)
                    .await
            {
                return Ok(());
            }

            let _ = rendezvous_manager.trigger_discovery().await;
            let _ = effects
                .sleep_ms(CHANNEL_ACCEPTANCE_PEER_CHANNEL_BACKOFF_MS)
                .await;

            if attempt + 1 == CHANNEL_ACCEPTANCE_PEER_CHANNEL_ATTEMPTS {
                let detail = result.error.unwrap_or_else(|| {
                    format!(
                        "peer channel for sender {sender_id} in {invitation_context} did not establish"
                    )
                });
                return Err(AgentError::runtime(detail));
            }
        }

        Err(AgentError::runtime(
            [
                "peer channel for sender ",
                &sender_id.to_string(),
                " did not establish for channel acceptance",
            ]
            .concat(),
        ))
    }

    async fn attempt_channel_checkpoint_provision(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
        log_message: &'static str,
    ) {
        let started_at = match effects.physical_time().await {
            Ok(started_at) => started_at,
            Err(error) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %error,
                    "{log_message}"
                );
                return;
            }
        };
        let budget = match TimeoutBudget::from_start_and_timeout(
            &started_at,
            Duration::from_millis(INVITATION_CHANNEL_JOIN_TIMEOUT_MS),
        ) {
            Ok(budget) => budget,
            Err(error) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %error,
                    "{log_message}"
                );
                return;
            }
        };

        let create_result = execute_with_timeout_budget(effects, &budget, || async {
            effects
                .create_channel(ChannelCreateParams {
                    context: context_id,
                    channel: Some(channel_id),
                    skip_window: None,
                    topic: None,
                })
                .await
        })
        .await;

        match create_result {
            Ok(_) => {}
            Err(TimeoutRunError::Timeout(error)) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %error,
                    timeout_ms = INVITATION_CHANNEL_JOIN_TIMEOUT_MS,
                    "{log_message}"
                );
            }
            Err(TimeoutRunError::Operation(error)) => {
                if !Self::channel_checkpoint_exists(effects, context_id, channel_id).await {
                    tracing::warn!(
                        context_id = %context_id,
                        channel_id = %channel_id,
                        error = %error,
                        "{log_message}"
                    );
                }
            }
        }
    }

    async fn attempt_channel_membership_provision(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
        participant: AuthorityId,
        log_message: &'static str,
    ) {
        let started_at = match effects.physical_time().await {
            Ok(started_at) => started_at,
            Err(error) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    participant = %participant,
                    error = %error,
                    "{log_message}"
                );
                return;
            }
        };
        let budget = match TimeoutBudget::from_start_and_timeout(
            &started_at,
            Duration::from_millis(INVITATION_CHANNEL_JOIN_TIMEOUT_MS),
        ) {
            Ok(budget) => budget,
            Err(error) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    participant = %participant,
                    error = %error,
                    "{log_message}"
                );
                return;
            }
        };

        let join_result = execute_with_timeout_budget(effects, &budget, || async {
            effects
                .join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant,
                })
                .await
        })
        .await;

        match join_result {
            Ok(()) => {}
            Err(TimeoutRunError::Timeout(error)) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    participant = %participant,
                    error = %error,
                    timeout_ms = INVITATION_CHANNEL_JOIN_TIMEOUT_MS,
                    "{log_message}"
                );
            }
            Err(TimeoutRunError::Operation(error)) => {
                if !Self::channel_has_participant(effects, context_id, channel_id, participant).await
                {
                    tracing::debug!(
                        context_id = %context_id,
                        channel_id = %channel_id,
                        participant = %participant,
                        error = %error,
                        "{log_message}"
                    );
                }
            }
        }
    }

    async fn require_channel_join(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
        participant: AuthorityId,
    ) -> AgentResult<()> {
        let started_at = effects
            .physical_time()
            .await
            .map_err(|error| AgentError::effects(error.to_string()))?;
        let budget = TimeoutBudget::from_start_and_timeout(
            &started_at,
            Duration::from_millis(INVITATION_CHANNEL_JOIN_TIMEOUT_MS),
        )
        .map_err(|error| AgentError::effects(error.to_string()))?;

        let join_result = execute_with_timeout_budget(effects, &budget, || async {
            effects
                .join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant,
                })
                .await
        })
        .await;

        match join_result {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Timeout(error)) => Err(AgentError::effects(error.to_string())),
            Err(TimeoutRunError::Operation(error)) => {
                if Self::channel_has_participant(effects, context_id, channel_id, participant).await
                {
                    Ok(())
                } else {
                    Err(AgentError::effects(error.to_string()))
                }
            }
        }
    }

    async fn require_channel_checkpoint(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        channel_id: ChannelId,
    ) -> AgentResult<()> {
        let started_at = effects
            .physical_time()
            .await
            .map_err(|error| AgentError::effects(error.to_string()))?;
        let budget = TimeoutBudget::from_start_and_timeout(
            &started_at,
            Duration::from_millis(INVITATION_CHANNEL_JOIN_TIMEOUT_MS),
        )
        .map_err(|error| AgentError::effects(error.to_string()))?;

        let create_result = execute_with_timeout_budget(effects, &budget, || async {
            effects
                .create_channel(ChannelCreateParams {
                    context: context_id,
                    channel: Some(channel_id),
                    skip_window: None,
                    topic: None,
                })
                .await
        })
        .await;

        match create_result {
            Ok(_) => Ok(()),
            Err(TimeoutRunError::Timeout(error)) => Err(AgentError::effects(error.to_string())),
            Err(TimeoutRunError::Operation(error)) => {
                if Self::channel_checkpoint_exists(effects, context_id, channel_id).await {
                    Ok(())
                } else {
                    Err(AgentError::effects(error.to_string()))
                }
            }
        }
    }

    #[aura_macros::best_effort_boundary]
    pub(super) async fn provision_amp_channel_for_inbound_chat_fact(
        &self,
        effects: &AuraEffectSystem,
        fact: &RelationalFact,
    ) {
        let RelationalFact::Generic {
            envelope: chat_envelope,
            ..
        } = fact
        else {
            return;
        };

        if chat_envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
            return;
        }

        let Some(ChatFact::ChannelCreated {
            context_id,
            channel_id,
            creator_id,
            ..
        }) = ChatFact::from_envelope(chat_envelope)
        else {
            return;
        };

        if aura_protocol::amp::get_channel_state(effects, context_id, channel_id)
            .await
            .is_ok()
        {
            return;
        }

        self.attempt_channel_checkpoint_provision(
            effects,
            context_id,
            channel_id,
            "Failed to provision AMP channel checkpoint from inbound chat fact",
        )
        .await;

        let local_authority = self.handler.context.authority.authority_id();
        let mut participants = vec![local_authority];
        if creator_id != local_authority {
            participants.push(creator_id);
        }

        for participant in participants {
            self.attempt_channel_membership_provision(
                effects,
                context_id,
                channel_id,
                participant,
                "AMP join provisioning from inbound chat fact failed",
            )
            .await;
        }
    }

    pub(super) async fn resolve_channel_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<ChannelInviteDetails>> {
        let own_id = self.handler.context.authority.authority_id();

        if let Some(inv) = self
            .handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await
        {
            if let InvitationType::Channel {
                home_id,
                nickname_suggestion,
                bootstrap,
            } = &inv.invitation_type
            {
                let home_name =
                    require_channel_invitation_name(*home_id, nickname_suggestion.clone())?;
                return Ok(Some(ChannelInviteDetails {
                    context_id: inv.context_id,
                    channel_id: *home_id,
                    home_id: home_id.to_string(),
                    home_name,
                    sender_id: inv.sender_id,
                    bootstrap: bootstrap.clone(),
                }));
            }
        }

        if let Some(shareable) =
            InvitationHandler::load_imported_invitation(effects, own_id, invitation_id).await
        {
            if let InvitationType::Channel {
                home_id,
                nickname_suggestion,
                bootstrap,
            } = shareable.invitation_type
            {
                let home_name = require_channel_invitation_name(home_id, nickname_suggestion)?;
                return Ok(Some(ChannelInviteDetails {
                    context_id: require_channel_invitation_context(
                        &shareable.invitation_id,
                        shareable.sender_id,
                        shareable.context_id,
                    )?,
                    channel_id: home_id,
                    home_id: home_id.to_string(),
                    home_name,
                    sender_id: shareable.sender_id,
                    bootstrap,
                }));
            }
        }

        let Ok(envelopes) = crate::handlers::shared::load_relational_fact_envelopes_by_type(
            effects,
            own_id,
            INVITATION_FACT_TYPE_ID,
        )
        .await
        else {
            return Ok(None);
        };

        for envelope in &envelopes {
            let Some(inv_fact) = InvitationFact::from_envelope(envelope) else {
                continue;
            };

            let InvitationFact::Sent {
                invitation_id: seen_id,
                sender_id,
                receiver_id,
                invitation_type,
                context_id,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != *invitation_id {
                continue;
            }

            if receiver_id != own_id {
                return Ok(None);
            }

            if let InvitationType::Channel {
                home_id,
                nickname_suggestion,
                bootstrap,
            } = invitation_type
            {
                let home_name = require_channel_invitation_name(home_id, nickname_suggestion)?;
                return Ok(Some(ChannelInviteDetails {
                    context_id,
                    channel_id: home_id,
                    home_id: home_id.to_string(),
                    home_name,
                    sender_id,
                    bootstrap,
                }));
            }

            return Ok(None);
        }

        Ok(None)
    }

    pub(super) async fn resolve_channel_context_from_chat_facts(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> ContextId {
        let own_id = self.handler.context.authority.authority_id();
        let Ok(envelopes) = crate::handlers::shared::load_relational_fact_envelopes_by_type(
            effects,
            own_id,
            CHAT_FACT_TYPE_ID,
        )
        .await
        else {
            return invite.context_id;
        };

        for envelope in envelopes {
            let Some(ChatFact::ChannelCreated {
                context_id,
                channel_id,
                creator_id,
                ..
            }) = ChatFact::from_envelope(&envelope)
            else {
                continue;
            };

            if channel_id == invite.channel_id && creator_id == invite.sender_id {
                return context_id;
            }
        }

        invite.context_id
    }

    pub(super) async fn materialize_channel_invitation_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
    ) -> AgentResult<()> {
        let own_id = self.handler.context.authority.authority_id();

        self.require_channel_checkpoint(effects, invite.context_id, invite.channel_id)
            .await?;
        self.require_channel_join(effects, invite.context_id, invite.channel_id, own_id)
            .await?;

        let existing_channel_name = self
            .handler
            .channel_created_fact_name(effects, own_id, invite.context_id, invite.channel_id)
            .await;
        if existing_channel_name.as_deref() != Some(invite.home_name.as_str()) {
            let now_ms = effects.current_timestamp().await.unwrap_or(0);
            let fact = match existing_channel_name {
                Some(_) => ChatFact::channel_updated_ms(
                    invite.context_id,
                    invite.channel_id,
                    Some(invite.home_name.clone()),
                    Some(format!("Home channel {}", invite.home_id)),
                    None,
                    None,
                    now_ms,
                    invite.sender_id,
                )
                .to_generic(),
                None => ChatFact::channel_created_ms(
                    invite.context_id,
                    invite.channel_id,
                    invite.home_name.clone(),
                    Some(format!("Home channel {}", invite.home_id)),
                    false,
                    now_ms,
                    invite.sender_id,
                )
                .to_generic(),
            };

            effects
                .commit_relational_facts(vec![fact])
                .await
                .map_err(|e| AgentError::effects(e.to_string()))?;
        }

        let reactive = effects.reactive_handler();
        let now_ms = InvitationHandler::best_effort_current_timestamp_ms(effects).await;
        crate::reactive::app_signal_views::materialize_home_signal_for_channel_invitation(
            &reactive,
            own_id,
            invite.channel_id,
            &invite.home_name,
            invite.sender_id,
            invite.context_id,
            now_ms,
        )
        .await
        .map_err(AgentError::runtime)?;

        Ok(())
    }

    pub(super) async fn materialize_channel_bootstrap_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
        bootstrap_id: Hash32,
    ) -> AgentResult<()> {
        if let Ok(state) =
            aura_protocol::amp::get_channel_state(effects, invite.context_id, invite.channel_id)
                .await
        {
            if let Some(existing) = state.bootstrap {
                if existing.bootstrap_id != bootstrap_id {
                    tracing::warn!(
                        context_id = %invite.context_id,
                        channel_id = %invite.channel_id,
                        existing_bootstrap_id = %existing.bootstrap_id,
                        incoming_bootstrap_id = %bootstrap_id,
                        "Received channel invitation bootstrap that conflicts with existing channel bootstrap"
                    );
                }
                return Ok(());
            }
        }

        let now_ms = effects.current_timestamp().await.unwrap_or(0);
        let own_id = self.handler.context.authority.authority_id();
        let recipients: Vec<_> = BTreeSet::from([invite.sender_id, own_id])
            .into_iter()
            .collect();
        let bootstrap_fact = aura_journal::fact::ChannelBootstrap {
            context: invite.context_id,
            channel: invite.channel_id,
            bootstrap_id,
            dealer: invite.sender_id,
            recipients,
            created_at: PhysicalTime {
                ts_ms: now_ms,
                uncertainty: None,
            },
            expires_at: None,
        };

        effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelBootstrap(bootstrap_fact),
            ))
            .await
            .map_err(|e| AgentError::effects(e.to_string()))?;

        Ok(())
    }
}

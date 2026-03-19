use super::*;

const INVITATION_CHANNEL_JOIN_TIMEOUT_MS: u64 = 2_000;
const INVITATION_VIEW_UPDATE_TIMEOUT_MS: u64 = 500;

pub(super) struct InvitationChannelHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationChannelHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    async fn best_effort_await_view_update(
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
            Duration::from_millis(INVITATION_VIEW_UPDATE_TIMEOUT_MS),
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

        match execute_with_timeout_budget(effects, &budget, || async {
            effects.await_next_view_update().await;
            Ok::<(), AgentError>(())
        })
        .await
        {
            Ok(()) => {}
            Err(TimeoutRunError::Timeout(error)) => {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %error,
                    timeout_ms = INVITATION_VIEW_UPDATE_TIMEOUT_MS,
                    "{log_message}"
                );
            }
            Err(TimeoutRunError::Operation(_)) => {}
        }
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
                let lowered = error.to_string().to_ascii_lowercase();
                if !lowered.contains("already") && !lowered.contains("exists") {
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
                let lowered = error.to_string().to_ascii_lowercase();
                if lowered.contains("already") || lowered.contains("exists") {
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
                let lowered = error.to_string().to_ascii_lowercase();
                if lowered.contains("already") || lowered.contains("exists") {
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
                let home_name = nickname_suggestion
                    .clone()
                    .unwrap_or_else(|| home_id.to_string());
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
                let home_name = nickname_suggestion.unwrap_or_else(|| home_id.to_string());
                return Ok(Some(ChannelInviteDetails {
                    context_id: shareable
                        .context_id
                        .unwrap_or_else(|| default_context_id_for_authority(shareable.sender_id)),
                    channel_id: home_id,
                    home_id: home_id.to_string(),
                    home_name,
                    sender_id: shareable.sender_id,
                    bootstrap,
                }));
            }
        }

        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return Ok(None);
        };

        for fact in facts.iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != INVITATION_FACT_TYPE_ID {
                continue;
            }

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
                let home_name = nickname_suggestion.unwrap_or_else(|| home_id.to_string());
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
        let Ok(facts) = effects.load_committed_facts(own_id).await else {
            return invite.context_id;
        };

        for fact in facts.into_iter().rev() {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }

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

        if !self
            .handler
            .channel_created_fact_exists(effects, own_id, invite.context_id, invite.channel_id)
            .await
        {
            let now_ms = effects.current_timestamp().await.unwrap_or(0);
            let fact = ChatFact::channel_created_ms(
                invite.context_id,
                invite.channel_id,
                invite.home_name.clone(),
                Some(format!("Home channel {}", invite.home_id)),
                false,
                now_ms,
                invite.sender_id,
            )
            .to_generic();

            effects
                .commit_relational_facts(vec![fact])
                .await
                .map_err(|e| AgentError::effects(e.to_string()))?;
        }

        self.handler
            .materialize_home_signal_for_channel_invitation(effects, invite)
            .await?;

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
        self.best_effort_await_view_update(
            effects,
            invite.context_id,
            invite.channel_id,
            "Timed out waiting for channel invitation bootstrap view update",
        )
        .await;

        Ok(())
    }
}

use super::*;

pub(super) struct InvitationChannelHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationChannelHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

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

        if let Err(error) = effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: None,
            })
            .await
        {
            let lowered = error.to_string().to_ascii_lowercase();
            if !lowered.contains("already") && !lowered.contains("exists") {
                tracing::warn!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %error,
                    "Failed to provision AMP channel checkpoint from inbound chat fact"
                );
                return;
            }
        }

        let local_authority = self.handler.context.authority.authority_id();
        let mut participants = vec![local_authority];
        if creator_id != local_authority {
            participants.push(creator_id);
        }

        for participant in participants {
            if let Err(error) = effects
                .join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant,
                })
                .await
            {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    participant = %participant,
                    error = %error,
                    "AMP join provisioning from inbound chat fact failed"
                );
            }
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

        if let Err(error) = effects
            .join_channel(ChannelJoinParams {
                context: invite.context_id,
                channel: invite.channel_id,
                participant: own_id,
            })
            .await
        {
            tracing::debug!(
                context_id = %invite.context_id,
                channel_id = %invite.channel_id,
                error = %error,
                "Failed to join invited channel (continuing)"
            );
        }

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
                .map_err(|e| AgentError::effects(format!("commit invited channel fact: {e}")))?;
            effects.await_next_view_update().await;
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
            .map_err(|e| AgentError::effects(format!("insert AMP bootstrap fact: {e}")))?;

        Ok(())
    }
}

use super::*;
use aura_protocol::amp::{ChannelMembershipFact, ChannelParticipantEvent};

pub(super) struct InvitationContactHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationContactHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    pub(super) async fn notify_contact_invitation_acceptance(
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

        if !matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
            return Ok(());
        }

        let acceptor_id = self.handler.context.authority.authority_id();
        if invitation.sender_id == acceptor_id {
            return Ok(());
        }

        let acceptance = ContactInvitationAcceptance {
            invitation_id: invitation.invitation_id.clone(),
            acceptor_id,
        };
        let payload =
            serde_json::to_vec(&acceptance).map_err(|e| AgentError::internal(e.to_string()))?;

        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE.to_string(),
        );
        metadata.insert(
            "invitation-id".to_string(),
            invitation.invitation_id.to_string(),
        );
        metadata.insert("acceptor-id".to_string(), acceptor_id.to_string());
        metadata.insert(
            "acceptor-device-id".to_string(),
            effects.device_id().to_string(),
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
        tracing::info!(
            invitation_id = %invitation.invitation_id,
            acceptor_id = %acceptor_id,
            acceptor_hint = ?acceptor_hint,
            "contact invitation acceptance websocket hint"
        );
        if let Some(acceptor_hint) = acceptor_hint {
            metadata.insert("acceptor-addr".to_string(), acceptor_hint);
        }

        let envelope = TransportEnvelope {
            destination: invitation.sender_id,
            source: acceptor_id,
            context: default_context_id_for_authority(invitation.sender_id),
            payload,
            metadata,
            receipt: None,
        };

        effects
            .send_envelope(envelope)
            .await
            .map_err(|e| AgentError::effects(e.to_string()))?;

        Ok(())
    }

    pub(super) async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<usize> {
        let mut processed = 0usize;
        let mut deferred_envelopes = Vec::new();
        let mut scanned = 0usize;
        const MAX_SCANS_PER_TICK: usize = 4096;

        while scanned < MAX_SCANS_PER_TICK {
            let envelope = match effects.receive_envelope().await {
                Ok(env) => env,
                Err(TransportError::NoMessage) => break,
                Err(e) => {
                    tracing::warn!("Error receiving contact invitation acceptance: {}", e);
                    break;
                }
            };
            scanned = scanned.saturating_add(1);

            let Some(content_type) = envelope.metadata.get("content-type") else {
                deferred_envelopes.push(envelope);
                continue;
            };

            if content_type == CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE {
                let acceptance: ContactInvitationAcceptance =
                    match serde_json::from_slice(&envelope.payload) {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Invalid contact invitation acceptance payload"
                            );
                            continue;
                        }
                    };

                if acceptance.acceptor_id == self.handler.context.authority.authority_id() {
                    continue;
                }

                let acceptor_addr = envelope.metadata.get("acceptor-addr").map(String::as_str);
                let acceptor_device_id = envelope
                    .metadata
                    .get("acceptor-device-id")
                    .and_then(|value| value.parse().ok());
                if acceptor_addr.is_some() || acceptor_device_id.is_some() {
                    let now_ms = effects.current_timestamp().await.unwrap_or(0);
                    self.handler
                        .cache_peer_descriptor_for_peer(
                            effects.as_ref(),
                            acceptance.acceptor_id,
                            acceptor_device_id,
                            acceptor_addr,
                            now_ms,
                        )
                        .await;
                }

                let Some(invitation) = InvitationHandler::load_created_invitation(
                    effects.as_ref(),
                    self.handler.context.authority.authority_id(),
                    &acceptance.invitation_id,
                )
                .await
                else {
                    tracing::debug!(
                        invitation_id = %acceptance.invitation_id,
                        "Ignoring acceptance for unknown invitation"
                    );
                    continue;
                };

                if !matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                    continue;
                }

                if invitation.status == InvitationStatus::Accepted {
                    continue;
                }

                let now_ms = effects.current_timestamp().await.unwrap_or(0);
                let context_id = self.handler.context.authority.default_context_id();

                let fact = InvitationFact::accepted_ms(
                    acceptance.invitation_id.clone(),
                    acceptance.acceptor_id,
                    now_ms,
                );
                execute_journal_append(
                    fact,
                    &self.handler.context.authority,
                    context_id,
                    effects.as_ref(),
                )
                .await?;
                effects.await_next_view_update().await;

                let contact_fact = ContactFact::Added {
                    context_id,
                    owner_id: self.handler.context.authority.authority_id(),
                    contact_id: acceptance.acceptor_id,
                    nickname: acceptance.acceptor_id.to_string(),
                    added_at: PhysicalTime {
                        ts_ms: now_ms,
                        uncertainty: None,
                    },
                };

                effects
                    .commit_generic_fact_bytes(
                        context_id,
                        CONTACT_FACT_TYPE_ID.into(),
                        contact_fact.to_bytes(),
                    )
                    .await
                    .map_err(|e| AgentError::effects(e.to_string()))?;
                effects.await_next_view_update().await;

                let mut updated = invitation.clone();
                updated.status = InvitationStatus::Accepted;
                InvitationHandler::persist_created_invitation(
                    effects.as_ref(),
                    self.handler.context.authority.authority_id(),
                    &updated,
                )
                .await?;
                if let InvitationType::Channel {
                    home_id,
                    nickname_suggestion,
                    ..
                } = &updated.invitation_type
                {
                    let reactive = effects.reactive_handler();
                    let now_ms =
                        InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref()).await;
                    let home_name =
                        require_channel_invitation_name(*home_id, nickname_suggestion.clone())?;
                    crate::reactive::app_signal_views::materialize_home_signal_for_channel_acceptance(
                        &reactive,
                        *home_id,
                        &home_name,
                        updated.sender_id,
                        updated.receiver_id,
                        updated.context_id,
                        now_ms,
                    )
                    .await
                    .map_err(AgentError::runtime)?;
                }
                self.handler
                    .invitation_cache
                    .cache_invitation(updated)
                    .await;

                processed = processed.saturating_add(1);
                continue;
            }

            if content_type == CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE {
                let acceptance: ChannelInvitationAcceptance =
                    match serde_json::from_slice(&envelope.payload) {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Invalid channel invitation acceptance payload"
                            );
                            continue;
                        }
                    };

                if acceptance.acceptor_id == self.handler.context.authority.authority_id() {
                    continue;
                }

                let acceptor_addr = envelope.metadata.get("acceptor-addr").map(String::as_str);
                let acceptor_device_id = envelope
                    .metadata
                    .get("acceptor-device-id")
                    .and_then(|value| value.parse().ok());
                if acceptor_addr.is_some() || acceptor_device_id.is_some() {
                    let now_ms = effects.current_timestamp().await.unwrap_or(0);
                    self.handler
                        .cache_peer_descriptor_for_peer(
                            effects.as_ref(),
                            acceptance.acceptor_id,
                            acceptor_device_id,
                            acceptor_addr,
                            now_ms,
                        )
                        .await;
                }

                let Some(invitation) = InvitationHandler::load_created_invitation(
                    effects.as_ref(),
                    self.handler.context.authority.authority_id(),
                    &acceptance.invitation_id,
                )
                .await
                else {
                    tracing::debug!(
                        invitation_id = %acceptance.invitation_id,
                        "Ignoring channel acceptance for unknown invitation"
                    );
                    continue;
                };

                if !matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                    continue;
                }

                let now_ms = effects.current_timestamp().await.unwrap_or(0);
                let fact = InvitationFact::accepted_ms(
                    acceptance.invitation_id.clone(),
                    acceptance.acceptor_id,
                    now_ms,
                );
                execute_journal_append(
                    fact,
                    &self.handler.context.authority,
                    acceptance.context_id,
                    effects.as_ref(),
                )
                .await?;
                effects.await_next_view_update().await;

                let timestamp = ChannelMembershipFact::random_timestamp(effects.as_ref()).await;
                let membership = ChannelMembershipFact::new(
                    acceptance.context_id,
                    acceptance.channel_id,
                    acceptance.acceptor_id,
                    ChannelParticipantEvent::Joined,
                    timestamp,
                )
                .to_generic();

                effects
                    .commit_relational_facts(vec![membership])
                    .await
                    .map_err(|e| AgentError::effects(e.to_string()))?;
                effects.await_next_view_update().await;

                let mut updated = invitation.clone();
                updated.status = InvitationStatus::Accepted;
                InvitationHandler::persist_created_invitation(
                    effects.as_ref(),
                    self.handler.context.authority.authority_id(),
                    &updated,
                )
                .await?;
                if let InvitationType::Channel {
                    home_id,
                    nickname_suggestion,
                    ..
                } = &updated.invitation_type
                {
                    let reactive = effects.reactive_handler();
                    let now_ms =
                        InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref()).await;
                    let home_name =
                        require_channel_invitation_name(*home_id, nickname_suggestion.clone())?;
                    crate::reactive::app_signal_views::materialize_home_signal_for_channel_acceptance(
                        &reactive,
                        *home_id,
                        &home_name,
                        updated.sender_id,
                        updated.receiver_id,
                        updated.context_id,
                        now_ms,
                    )
                    .await
                    .map_err(AgentError::runtime)?;
                }
                self.handler
                    .invitation_cache
                    .cache_invitation(updated)
                    .await;

                processed = processed.saturating_add(1);
                continue;
            }

            if content_type == CHAT_FACT_CONTENT_TYPE {
                let fact: RelationalFact = match from_slice(&envelope.payload) {
                    Ok(fact) => fact,
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "Invalid chat fact payload envelope"
                        );
                        continue;
                    }
                };

                super::channel::InvitationChannelHandler::new(self.handler)
                    .provision_amp_channel_for_inbound_chat_fact(effects.as_ref(), &fact)
                    .await;

                effects
                    .commit_relational_facts(vec![fact])
                    .await
                    .map_err(|e| AgentError::effects(e.to_string()))?;
                effects.await_next_view_update().await;

                processed = processed.saturating_add(1);
                continue;
            }

            if content_type == INVITATION_CONTENT_TYPE {
                let code = match String::from_utf8(envelope.payload) {
                    Ok(code) => code,
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "Invalid invitation payload envelope"
                        );
                        continue;
                    }
                };

                let code = code.trim();
                if code.is_empty() {
                    tracing::warn!("Received empty invitation payload envelope");
                    continue;
                }

                match self
                    .handler
                    .import_invitation_code(effects.as_ref(), code)
                    .await
                {
                    Ok(invitation) => {
                        if matches!(invitation.invitation_type, InvitationType::Channel { .. })
                            && InvitationHandler::sender_contact_exists(
                                effects.as_ref(),
                                self.handler.context.authority.authority_id(),
                                invitation.sender_id,
                            )
                            .await
                        {
                            if let Err(error) = self
                                .handler
                                .accept_invitation(effects.clone(), &invitation.invitation_id)
                                .await
                            {
                                tracing::warn!(
                                    invitation_id = %invitation.invitation_id,
                                    sender_id = %invitation.sender_id,
                                    error = %error,
                                    "Failed to auto-accept inbound channel invitation"
                                );
                            }
                        }
                        processed = processed.saturating_add(1);
                    }
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "Failed to import inbound invitation envelope"
                        );
                    }
                }
                continue;
            }

            deferred_envelopes.push(envelope);
        }

        for envelope in deferred_envelopes {
            effects.requeue_envelope(envelope);
        }

        Ok(processed)
    }

    pub(super) async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<(AuthorityId, String)>> {
        let own_id = self.handler.context.authority.authority_id();

        tracing::debug!(
            invitation_id = %invitation_id,
            own_authority = %own_id,
            "resolve_contact_invitation: starting lookup"
        );

        if let Some(inv) = self
            .handler
            .invitation_cache
            .get_invitation(invitation_id)
            .await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                invitation_type = ?inv.invitation_type,
                sender_id = %inv.sender_id,
                "resolve_contact_invitation: found in cache"
            );
            if let InvitationType::Contact { nickname } = &inv.invitation_type {
                let other = if inv.sender_id == own_id {
                    inv.receiver_id
                } else {
                    inv.sender_id
                };
                let nickname = nickname.clone().unwrap_or_else(|| other.to_string());
                tracing::debug!(
                    contact_id = %other,
                    nickname = %nickname,
                    "resolve_contact_invitation: resolved from cache"
                );
                return Ok(Some((other, nickname)));
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in cache"
            );
        }

        if let Some(shareable) =
            InvitationHandler::load_imported_invitation(effects, own_id, invitation_id).await
        {
            tracing::debug!(
                invitation_id = %invitation_id,
                invitation_type = ?shareable.invitation_type,
                sender_id = %shareable.sender_id,
                "resolve_contact_invitation: found in persisted store"
            );
            if let InvitationType::Contact { nickname } = shareable.invitation_type {
                if shareable.sender_id != own_id {
                    let other = shareable.sender_id;
                    let nickname = nickname.unwrap_or_else(|| other.to_string());
                    tracing::debug!(
                        contact_id = %other,
                        nickname = %nickname,
                        "resolve_contact_invitation: resolved from persisted store"
                    );
                    return Ok(Some((other, nickname)));
                }
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in persisted store"
            );
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
                message,
                ..
            } = inv_fact
            else {
                continue;
            };

            if seen_id != *invitation_id {
                continue;
            }

            let aura_invitation::InvitationType::Contact { nickname } = invitation_type else {
                return Ok(None);
            };

            if receiver_id != own_id {
                return Ok(None);
            }

            let nickname = nickname
                .or_else(|| {
                    message
                        .as_deref()
                        .and_then(|m| m.split("from ").nth(1))
                        .and_then(|s| s.split_whitespace().next())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| sender_id.to_string());

            return Ok(Some((sender_id, nickname)));
        }

        Ok(None)
    }
}

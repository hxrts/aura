use super::contact_confirmation::{
    contact_acceptance_digest, settled_contact_invitation_decision, ContactInvitationDecision,
    CONTACT_INVITATION_RESPONSE_CONTENT_TYPE,
};
use super::*;
use aura_journal::fact::RelationalFact;
use aura_protocol::amp::{ChannelMembershipFact, ChannelParticipantEvent};
use aura_protocol::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};

const CONTACT_INVITATION_ACCEPTANCE_PROCESS_TIMEOUT_MS: u64 = 20_000;
const CONTACT_ACCEPTANCE_PEER_CHANNEL_ATTEMPTS: usize = 6;
const CONTACT_ACCEPTANCE_PEER_CHANNEL_BACKOFF_MS: u64 = 75;

#[derive(Debug, thiserror::Error)]
enum ContactInvitationAcceptanceError {
    #[error("contact invitation acceptance peer channel is not yet established for {sender_id}")]
    PeerChannelNotEstablished { sender_id: AuthorityId },
}

pub(super) struct InvitationContactHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationContactHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    fn verified_invitation_payload<T>(
        &self,
        envelope: &TransportEnvelope,
        payload: T,
    ) -> AgentResult<VerifiedIngress<T>> {
        let metadata = VerifiedIngressMetadata::new(
            IngressSource::Authority(envelope.source),
            envelope.context,
            None,
            aura_core::Hash32::from_bytes(&envelope.payload),
            1,
        );
        let schema_version = envelope
            .metadata
            .get("wire-format-version")
            .and_then(|version| version.parse::<u16>().ok())
            .unwrap_or(aura_protocol::messages::WIRE_FORMAT_VERSION);
        let content_type = envelope.metadata.get("content-type").map(String::as_str);
        let known_acceptance_type = matches!(
            content_type,
            Some(CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE)
                | Some(CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE)
                | Some(CHAT_FACT_CONTENT_TYPE)
        );
        let evidence = IngressVerificationEvidence::builder(metadata)
            .peer_identity(
                envelope.source != self.handler.context.authority.authority_id(),
                "invitation acceptance must come from a remote authority",
            )
            .and_then(|builder| {
                builder.envelope_authenticity(
                    !envelope.payload.is_empty(),
                    "invitation acceptance payload must be present",
                )
            })
            .and_then(|builder| {
                builder.capability_authorization(
                    envelope.receipt.is_some(),
                    "invitation acceptance requires guard-chain receipt evidence",
                )
            })
            .and_then(|builder| {
                builder.namespace_scope(
                    known_acceptance_type,
                    "invitation acceptance content-type must match accepted namespaces",
                )
            })
            .and_then(|builder| {
                builder.schema_version(
                    schema_version <= aura_protocol::messages::WIRE_FORMAT_VERSION,
                    "unsupported invitation acceptance schema",
                )
            })
            .and_then(|builder| {
                builder.replay_freshness(
                    envelope
                        .receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.nonce != 0),
                    "invitation acceptance receipt nonce must be non-zero",
                )
            })
            .and_then(|builder| {
                builder.signer_membership(
                    envelope
                        .receipt
                        .as_ref()
                        .is_some_and(|receipt| receipt.src == envelope.source),
                    "invitation acceptance receipt signer must match source authority",
                )
            })
            .and_then(|builder| {
                builder.proof_evidence(
                    envelope
                        .receipt
                        .as_ref()
                        .is_some_and(|receipt| !receipt.sig.is_empty()),
                    "invitation acceptance receipt signature evidence must be present",
                )
            })
            .and_then(|builder| builder.build())
            .map_err(|_| AgentError::internal("verify invitation ingress failed"))?;
        DecodedIngress::new(payload, evidence.metadata().clone())
            .verify(evidence)
            .map_err(|_| AgentError::internal("promote invitation ingress failed"))
    }

    async fn publish_channel_acceptance_chat_projection(
        &self,
        effects: &AuraEffectSystem,
        original: &aura_core::TimeoutBudget,
        context_id: ContextId,
        home_id: ChannelId,
        home_name: &str,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
    ) -> AgentResult<()> {
        let now_ms = InvitationHandler::best_effort_current_timestamp_ms(effects).await;
        let fact = aura_chat::ChatFact::channel_updated_ms(
            context_id,
            home_id,
            Some(home_name.to_string()),
            None,
            Some(2),
            Some(vec![receiver_id]),
            now_ms,
            sender_id,
        );
        let committed = effects
            .commit_relational_facts_required(vec![fact.to_generic()])
            .await
            .map_err(AgentError::from)?;
        committed
            .await_processed_in_original_window(original)
            .await?;
        Ok(())
    }

    async fn seed_sender_descriptor_for_authority_context(
        &self,
        effects: &AuraEffectSystem,
        sender_id: AuthorityId,
    ) {
        let Some(rendezvous_manager) = effects.rendezvous_manager() else {
            return;
        };

        let authority_context = default_context_id_for_authority(sender_id);
        if rendezvous_manager
            .get_descriptor(authority_context, sender_id)
            .await
            .is_some()
        {
            return;
        }

        let local_context_id = self.handler.context.authority.default_context_id();
        let descriptor = if let Some(existing) = rendezvous_manager
            .get_descriptor(local_context_id, sender_id)
            .await
        {
            Some(existing)
        } else {
            rendezvous_manager
                .get_lan_discovered_peer(sender_id)
                .await
                .map(|peer| peer.descriptor)
        };

        let Some(mut descriptor) = descriptor else {
            return;
        };

        descriptor.context_id = authority_context;
        let _ = rendezvous_manager.cache_descriptor(descriptor).await;
    }

    async fn ensure_sender_peer_channel(
        &self,
        effects: &AuraEffectSystem,
        sender_id: AuthorityId,
    ) -> AgentResult<()> {
        let Some(rendezvous_manager) = effects.rendezvous_manager() else {
            return Ok(());
        };

        let authority = self.handler.context.authority.clone();
        let handler = crate::handlers::rendezvous::RendezvousHandler::new(authority)
            .map_err(|error| AgentError::internal(error.to_string()))?
            .with_rendezvous_manager(rendezvous_manager.clone());
        let backoff = ExponentialBackoffPolicy::new(
            Duration::from_millis(CONTACT_ACCEPTANCE_PEER_CHANNEL_BACKOFF_MS),
            Duration::from_millis(CONTACT_ACCEPTANCE_PEER_CHANNEL_BACKOFF_MS),
            invitation_timeout_profile(effects).jitter(),
        )
        .map_err(|error| AgentError::runtime(error.to_string()))?;
        let retry_policy = invitation_timeout_profile(effects)
            .apply_retry_policy(&RetryBudgetPolicy::new(
                CONTACT_ACCEPTANCE_PEER_CHANNEL_ATTEMPTS as u32,
                backoff,
            ))
            .map_err(|error| AgentError::runtime(error.to_string()))?;
        let authority_context = default_context_id_for_authority(sender_id);

        execute_with_retry_budget(effects, &retry_policy, |_attempt| async {
            self.seed_sender_descriptor_for_authority_context(effects, sender_id)
                .await;

            if effects
                .is_channel_established(authority_context, sender_id)
                .await
            {
                return Ok(());
            }

            let result = handler
                .initiate_channel(effects, authority_context, sender_id)
                .await
                .map_err(|error| AgentError::runtime(error.to_string()))?;
            if result.success
                && effects
                    .is_channel_established(authority_context, sender_id)
                    .await
            {
                return Ok(());
            }

            Err(AgentError::runtime(
                ContactInvitationAcceptanceError::PeerChannelNotEstablished { sender_id }
                    .to_string(),
            ))
        })
        .await
        .map_err(|error| match error {
            RetryRunError::Timeout(timeout_error) => AgentError::timeout(timeout_error.to_string()),
            RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
        })
    }

    /// Signs acceptance from the required retained Contact import owner.
    #[aura_macros::capability_boundary(category = "capability_gated",
    capability = "required_contact_acceptance", capability_type = super::contact_confirmation::RequiredContactAcceptanceCapability,
    family = "runtime_helper")]
    pub(super) async fn build_contact_invitation_acceptance(
        &self,
        original: &super::contact_confirmation::RequiredContactAcceptanceCapability<'_>,
    ) -> AgentResult<Vec<u8>> {
        original.require_handler(self.handler)?;
        let effects = original.effects();
        let invitation = original.invitation();
        let acceptor_id = self.handler.context.authority.authority_id();
        if let Err(error) = self
            .ensure_sender_peer_channel(effects, invitation.sender_id)
            .await
        {
            tracing::debug!(
                invitation_id = %invitation.invitation_id,
                sender_id = %invitation.sender_id,
                acceptor_id = %acceptor_id,
                error = %error,
                "contact acceptance peer-channel warmup did not converge before notify"
            );
        }

        let nickname_suggestion = local_account_nickname_suggestion(effects).await;
        let signature = sign_invitation_acceptance_transcript(
            effects,
            acceptor_id,
            &contact_invitation_acceptance_transcript(
                invitation,
                acceptor_id,
                nickname_suggestion.clone(),
            ),
        )
        .await?;
        let acceptance = ContactInvitationAcceptance {
            invitation_id: invitation.invitation_id.clone(),
            acceptor_id,
            signature,
            nickname_suggestion,
        };
        let payload = serde_json::to_vec(&acceptance).map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Serialization {
                message: "encode required contact acceptance".into(),
                source: Some(Arc::new(source)),
            })
        })?;
        Ok(payload)
    }

    /// Sends a signed acceptance to the inviter.
    #[aura_macros::capability_boundary(category = "capability_gated",
    capability = "contact_acceptance_continuation", capability_type = super::contact_confirmation::ContactAcceptanceContinuationCapability,
    family = "runtime_helper")]
    pub(super) async fn send_contact_invitation_acceptance(
        &self,
        continuation: &super::contact_confirmation::ContactAcceptanceContinuationCapability<'_>,
    ) -> AgentResult<()> {
        continuation.require_handler(self.handler)?;
        let effects = continuation.effects();
        let invitation = continuation.invitation();
        let payload = continuation.payload().to_vec();
        let acceptor_id = self.handler.context.authority.authority_id();
        let delivery_context = default_context_id_for_authority(invitation.sender_id);
        let flow_receipt = execute_charge_flow_budget(
            FlowCost::new(1),
            delivery_context,
            invitation.sender_id,
            effects,
        )
        .await?;

        let mut metadata = crate::handlers::shared::build_transport_metadata(
            CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE,
            [
                ("invitation-id", invitation.invitation_id.to_string()),
                ("acceptor-id", acceptor_id.to_string()),
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
            context: delivery_context,
            payload,
            metadata,
            receipt: flow_receipt.map(transport_receipt_from_flow),
        };
        let mut envelope = envelope;
        attach_invitation_test_receipt_if_needed(effects, &mut envelope);

        crate::runtime::transport_boundary::send_guarded_transport_envelope(effects, envelope)
            .await
            .map_err(aura_core::AuraError::from)
            .map_err(AgentError::from)?;

        Ok(())
    }

    pub(super) async fn process_contact_invitation_acceptances(
        &self,
        effects: Arc<AuraEffectSystem>,
    ) -> AgentResult<usize> {
        let mut processed = 0usize;
        let mut deferred_envelopes = Vec::new();
        let mut in_flight_envelope: Option<TransportEnvelope> = None;
        let mut scanned = 0usize;
        const MAX_SCANS_PER_TICK: usize = 4096;
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "process_contact_invitation_acceptances",
            CONTACT_INVITATION_ACCEPTANCE_PROCESS_TIMEOUT_MS,
        )
        .await?;

        let process_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
            while scanned < MAX_SCANS_PER_TICK {
                let envelope = match effects.receive_envelope().await {
                    Ok(env) => env,
                    Err(TransportError::NoMessage) => break,
                    Err(
                        error @ (TransportError::InvalidEnvelope { .. }
                        | TransportError::ReceiptValidationFailed { .. }),
                    ) => {
                        tracing::warn!(
                            error = %error,
                            "Skipping invalid envelope while scanning contact invitation mailbox"
                        );
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!("Error receiving contact invitation acceptance: {}", e);
                        break;
                    }
                };
                scanned = scanned.saturating_add(1);
                in_flight_envelope = Some(envelope);

                let Some(content_type) = in_flight_envelope
                    .as_ref()
                    .and_then(|envelope| envelope.metadata.get("content-type"))
                    .cloned()
                else {
                    if let Some(envelope) = in_flight_envelope.take() {
                        deferred_envelopes.push(envelope);
                    }
                    continue;
                };

                if content_type == CONTACT_INVITATION_RESPONSE_CONTENT_TYPE {
                    // An inviter's answer to an acceptance whose wait already ended.
                    if let Some(envelope) = in_flight_envelope.take() {
                        if let Err(error) = self
                            .handler
                            .apply_contact_invitation_response(effects.as_ref(), &envelope)
                            .await
                        {
                            tracing::warn!(
                                error = %error,
                                "Failed to apply contact invitation response"
                            );
                        }
                    }
                    continue;
                }

                if content_type == CONTACT_INVITATION_ACCEPTANCE_CONTENT_TYPE {
                    let acceptance: ContactInvitationAcceptance = match in_flight_envelope
                        .as_ref()
                        .map(|envelope| serde_json::from_slice(&envelope.payload))
                    {
                        Some(Ok(data)) => data,
                        Some(Err(e)) => {
                            tracing::warn!(
                                error = %e,
                                "Invalid contact invitation acceptance payload"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                        None => continue,
                    };
                    let acceptance = match self.verified_invitation_payload(
                        in_flight_envelope
                            .as_ref()
                            .expect("in-flight envelope exists while processing acceptance"),
                        acceptance,
                    ) {
                        Ok(acceptance) => acceptance,
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                "Rejected unverified contact invitation acceptance"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                    };
                    let acceptance = acceptance.payload();

                    if acceptance.acceptor_id == self.handler.context.authority.authority_id() {
                        in_flight_envelope = None;
                        continue;
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
                        in_flight_envelope = None;
                        continue;
                    };

                    if !matches!(invitation.invitation_type, InvitationType::Contact { .. }) {
                        in_flight_envelope = None;
                        continue;
                    }

                    let Some(envelope) = in_flight_envelope.as_ref() else {
                        continue;
                    };
                    if envelope.source != acceptance.acceptor_id {
                        tracing::warn!(
                            envelope_source = %envelope.source,
                            acceptor_id = %acceptance.acceptor_id,
                            invitation_id = %acceptance.invitation_id,
                            "Rejected contact invitation acceptance with mismatched source authority"
                        );
                        in_flight_envelope = None;
                        continue;
                    }

                    if let Err(error) = verify_invitation_acceptance_signature(
                        effects.as_ref(),
                        acceptance.acceptor_id,
                        &contact_invitation_acceptance_transcript(
                            &invitation,
                            acceptance.acceptor_id,
                            acceptance.nickname_suggestion.clone(),
                        ),
                        &acceptance.signature,
                    )
                    .await
                    {
                        tracing::warn!(
                            error = %error,
                            invitation_id = %acceptance.invitation_id,
                            acceptor_id = %acceptance.acceptor_id,
                            "Rejected contact invitation acceptance with invalid signature"
                        );
                        in_flight_envelope = None;
                        continue;
                    }

                    // The acceptance is authenticated: answer it, binding the
                    // response to this exact acceptance.
                    let acceptance_digest = in_flight_envelope
                        .as_ref()
                        .map(|envelope| contact_acceptance_digest(&envelope.payload))
                        .unwrap_or_default();
                    let acceptor_device_id: Option<aura_core::DeviceId> = in_flight_envelope
                        .as_ref()
                        .and_then(|envelope| envelope.metadata.get("acceptor-device-id"))
                        .and_then(|value| value.parse().ok());
                    let now_ms =
                        InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref()).await;
                    // Learn the authenticated acceptor's route first: any answer,
                    // confirmation or rejection, is sent back over it.
                    let acceptor_addr = in_flight_envelope
                        .as_ref()
                        .and_then(|envelope| envelope.metadata.get("acceptor-addr"))
                        .map(String::as_str);
                    if acceptor_addr.is_some() || acceptor_device_id.is_some() {
                        self.handler
                            .cache_verified_peer_descriptor_for_peer(
                                effects.as_ref(),
                                acceptance.acceptor_id,
                                acceptor_device_id,
                                acceptor_addr,
                                now_ms,
                            )
                            .await;
                    }
                    let decision = settled_contact_invitation_decision(
                        &invitation.status,
                        invitation.is_expired(now_ms),
                        invitation.receiver_id == acceptance.acceptor_id,
                    );
                    if let Some(decision) = decision {
                        tracing::debug!(
                            invitation_id = %acceptance.invitation_id,
                            status = ?invitation.status,
                            decision = ?decision,
                            "Answering acceptance for an invitation that is no longer pending"
                        );
                        if let Err(error) = self
                            .handler
                            .send_contact_invitation_response(
                                effects.as_ref(),
                                &acceptance.invitation_id,
                                acceptance.acceptor_id,
                                acceptor_device_id,
                                acceptance_digest,
                                decision,
                            )
                            .await
                        {
                            tracing::warn!(
                                invitation_id = %acceptance.invitation_id,
                                error = %error,
                                "Failed to send contact invitation response"
                            );
                        }
                        in_flight_envelope = None;
                        continue;
                    }

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

                    let contact_fact = ContactFact::Added {
                        context_id,
                        owner_id: self.handler.context.authority.authority_id(),
                        contact_id: acceptance.acceptor_id,
                        nickname: acceptance
                            .nickname_suggestion
                            .clone()
                            .unwrap_or_else(|| acceptance.acceptor_id.to_string()),
                        added_at: PhysicalTime {
                            ts_ms: now_ms,
                            uncertainty: None,
                        },
                        invitation_code: None,
                    };

                    effects
                        .commit_generic_fact_bytes(
                            context_id,
                            CONTACT_FACT_TYPE_ID.into(),
                            contact_fact.to_bytes(),
                        )
                        .await
                        .map_err(|e| AgentError::effects(e.to_string()))?;

                    let mut updated = invitation.clone();
                    updated.status = InvitationStatus::Accepted;
                    updated.receiver_id = acceptance.acceptor_id;
                    InvitationHandler::persist_created_invitation(
                        effects.as_ref(),
                        self.handler.context.authority.authority_id(),
                        &updated,
                    )
                    .await?;
                    if let InvitationType::Channel {
                        home_id,
                        nickname_suggestion,
                        home,
                        ..
                    } = &updated.invitation_type
                    {
                        let now_ms =
                            InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref())
                                .await;
                        let home_name =
                            require_channel_invitation_name(*home_id, nickname_suggestion.clone())?;
                        self.publish_channel_acceptance_chat_projection(
                            effects.as_ref(),
                            &budget,
                            updated.context_id,
                            *home_id,
                            &home_name,
                            updated.sender_id,
                            updated.receiver_id,
                        )
                        .await?;
                        if *home {
                            let evidence = crate::reactive::app_signal_views::AcceptedHomeEvidence::from_accepted_invitation(
                                &updated, &home_name, now_ms,
                            )
                            .map_err(AgentError::runtime)?;
                            crate::reactive::app_signal_views::materialize_home_signal_for_channel_acceptance(
                                effects.as_ref(),
                                evidence,
                            )
                        .await
                        .map_err(AgentError::runtime)?;
                        }
                    }
                    self.handler
                        .invitation_cache
                        .cache_invitation(updated)
                        .await;
                    if let Err(error) = self
                        .handler
                        .send_contact_invitation_response(
                            effects.as_ref(),
                            &acceptance.invitation_id,
                            acceptance.acceptor_id,
                            acceptor_device_id,
                            acceptance_digest,
                            ContactInvitationDecision::Confirmed,
                        )
                        .await
                    {
                        tracing::warn!(
                            invitation_id = %acceptance.invitation_id,
                            error = %error,
                            "Failed to send contact invitation confirmation; the acceptor may resend"
                        );
                    }
                    if let Err(error) = self
                        .ensure_sender_peer_channel(effects.as_ref(), acceptance.acceptor_id)
                        .await
                    {
                        tracing::debug!(
                            invitation_id = %acceptance.invitation_id,
                            acceptor_id = %acceptance.acceptor_id,
                            error = %error,
                            "contact acceptance sender peer-channel warmup did not converge after acceptance processing"
                        );
                    }

                    processed = processed.saturating_add(1);
                    in_flight_envelope = None;
                    continue;
                }

                if content_type == CHANNEL_INVITATION_ACCEPTANCE_CONTENT_TYPE {
                    let acceptance: ChannelInvitationAcceptance = match in_flight_envelope
                        .as_ref()
                        .map(|envelope| serde_json::from_slice(&envelope.payload))
                    {
                        Some(Ok(data)) => data,
                        Some(Err(e)) => {
                            tracing::warn!(
                                error = %e,
                                "Invalid channel invitation acceptance payload"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                        None => continue,
                    };
                    let acceptance = match self.verified_invitation_payload(
                        in_flight_envelope
                            .as_ref()
                            .expect("in-flight envelope exists while processing acceptance"),
                        acceptance,
                    ) {
                        Ok(acceptance) => acceptance,
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                "Rejected unverified channel invitation acceptance"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                    };
                    let acceptance = acceptance.payload();

                    if acceptance.acceptor_id == self.handler.context.authority.authority_id() {
                        in_flight_envelope = None;
                        continue;
                    }

                    let invitation = InvitationHandler::load_created_invitation(
                        effects.as_ref(),
                        self.handler.context.authority.authority_id(),
                        &acceptance.invitation_id,
                    )
                    .await;

                    let Some(invitation) = invitation else {
                        in_flight_envelope = None;
                        continue;
                    };
                    if !matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                        in_flight_envelope = None;
                        continue;
                    }
                    let now_ms =
                        InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref()).await;
                    if invitation.is_expired(now_ms)
                        || invitation.status == InvitationStatus::Accepted
                    {
                        in_flight_envelope = None;
                        continue;
                    }

                    let Some(envelope) = in_flight_envelope.as_ref() else {
                        continue;
                    };
                    if envelope.source != acceptance.acceptor_id {
                        tracing::warn!(
                            envelope_source = %envelope.source,
                            acceptor_id = %acceptance.acceptor_id,
                            invitation_id = %acceptance.invitation_id,
                            "Rejected channel invitation acceptance with mismatched source authority"
                        );
                        in_flight_envelope = None;
                        continue;
                    }
                    if let Err(error) = verify_invitation_acceptance_signature(
                        effects.as_ref(),
                        acceptance.acceptor_id,
                        &channel_invitation_acceptance_transcript(
                            &invitation,
                            acceptance.acceptor_id,
                            acceptance.context_id,
                            acceptance.channel_id,
                            acceptance.channel_name.clone(),
                        ),
                        &acceptance.signature,
                    )
                    .await
                    {
                        tracing::warn!(
                            error = %error,
                            invitation_id = %acceptance.invitation_id,
                            acceptor_id = %acceptance.acceptor_id,
                            "Rejected channel invitation acceptance with invalid signature"
                        );
                        in_flight_envelope = None;
                        continue;
                    }

                    let acceptor_addr = in_flight_envelope
                        .as_ref()
                        .and_then(|envelope| envelope.metadata.get("acceptor-addr"))
                        .map(String::as_str);
                    let acceptor_device_id = in_flight_envelope
                        .as_ref()
                        .and_then(|envelope| envelope.metadata.get("acceptor-device-id"))
                        .and_then(|value| value.parse().ok());
                    if acceptor_addr.is_some() || acceptor_device_id.is_some() {
                        self.handler
                            .cache_verified_peer_descriptor_for_peer(
                                effects.as_ref(),
                                acceptance.acceptor_id,
                                acceptor_device_id,
                                acceptor_addr,
                                now_ms,
                            )
                            .await;
                    }

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
                    effects.await_reactive_publications_in_original_window(&budget).await.map_err(AgentError::from)?;

                    if aura_protocol::amp::journal::channel_participant_departed(effects.as_ref(), acceptance.context_id, acceptance.channel_id, acceptance.acceptor_id).await.map_err(AgentError::from)? {
                        return Err(AgentError::from(aura_core::AuraError::Invalid {
                            message: "contact acceptance cannot supersede original channel departure".into(),
                            source: Some(Arc::new(aura_core::effects::amp::AmpChannelError::RejoinRequiresMembershipEvidence {
                                context: acceptance.context_id, channel: acceptance.channel_id, participant: acceptance.acceptor_id,
                            })),
                        }));
                    }
                    let timestamp = ChannelMembershipFact::random_timestamp(effects.as_ref()).await.map_err(AgentError::from)?;
                    let membership = ChannelMembershipFact::new(
                        acceptance.context_id,
                        acceptance.channel_id,
                        acceptance.acceptor_id,
                        ChannelParticipantEvent::Joined,
                        timestamp,
                    )
                    .to_generic();

                    effects
                        .insert_relational_fact(membership.clone())
                        .await
                        .map_err(AgentError::from)?;
                    effects
                        .commit_relational_facts(vec![membership])
                        .await
                        .map_err(AgentError::from)?;
                    effects.await_reactive_publications_in_original_window(&budget).await.map_err(AgentError::from)?;

                    let now_ms =
                        InvitationHandler::best_effort_current_timestamp_ms(effects.as_ref())
                            .await;
                    let local_authority = self.handler.context.authority.authority_id();
                    let mut updated = invitation.clone();
                    updated.status = InvitationStatus::Accepted;
                    updated.receiver_id = acceptance.acceptor_id;
                    tracing::debug!(
                        invitation_id = %updated.invitation_id,
                        sender_id = %updated.sender_id,
                        receiver_id = %updated.receiver_id,
                        invitation_context = %updated.context_id,
                        acceptance_context = %acceptance.context_id,
                        channel_id = %acceptance.channel_id,
                        "processed channel invitation acceptance for created invitation"
                    );
                    InvitationHandler::persist_created_invitation(
                        effects.as_ref(),
                        local_authority,
                        &updated,
                    )
                    .await?;
                    self.handler.invitation_cache.cache_invitation(updated.clone()).await;
                    let InvitationType::Channel {
                        home_id,
                        nickname_suggestion,
                        home,
                        ..
                    } = &updated.invitation_type
                    else {
                        return Err(AgentError::internal(
                            "channel invitation acceptance persisted a non-channel invitation"
                                .to_string(),
                        ));
                    };
                    let home_name =
                        require_channel_invitation_name(*home_id, nickname_suggestion.clone())?;
                    self.publish_channel_acceptance_chat_projection(
                        effects.as_ref(),
                            &budget,
                        updated.context_id,
                        *home_id,
                        &home_name,
                        updated.sender_id,
                        updated.receiver_id,
                    )
                    .await?;
                    if *home {
                        let evidence = crate::reactive::app_signal_views::AcceptedHomeEvidence::from_accepted_invitation(
                            &updated, &home_name, now_ms,
                        )
                        .map_err(AgentError::runtime)?;
                        crate::reactive::app_signal_views::materialize_home_signal_for_channel_acceptance(
                            effects.as_ref(),
                            evidence,
                        )
                        .await
                        .map_err(AgentError::runtime)?;
                        self.handler
                            .commit_home_membership(
                                effects.as_ref(),
                                &ChannelInviteDetails {
                                    context_id: updated.context_id,
                                    channel_id: *home_id,
                                    home_name: home_name.clone(),
                                    sender_id: updated.sender_id,
                                    bootstrap: None,
                                    home: true,
                                },
                                updated.receiver_id,
                                false,
                            )
                            .await?;
                    }
                    let _ = home_name;

                    processed = processed.saturating_add(1);
                    in_flight_envelope = None;
                    continue;
                }

                if content_type == CHAT_FACT_CONTENT_TYPE {
                    let fact: RelationalFact = match in_flight_envelope
                        .as_ref()
                        .map(|envelope| from_slice(&envelope.payload))
                    {
                        Some(Ok(fact)) => fact,
                        Some(Err(error)) => {
                            tracing::warn!(
                                error = %error,
                                "Invalid chat fact payload envelope"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                        None => continue,
                    };
                    let fact = match self.verified_invitation_payload(
                        in_flight_envelope
                            .as_ref()
                            .expect("in-flight envelope exists while processing chat fact"),
                        fact,
                    ) {
                        Ok(fact) => fact,
                        Err(error) => {
                            tracing::warn!(
                                error = %error,
                                "Rejected unverified inbound chat fact"
                            );
                            in_flight_envelope = None;
                            continue;
                        }
                    };
                    let fact = fact.payload();
                    // The receipt binds the envelope source to the sender, so a
                    // moderation fact must name that sender as its actor.
                    if let RelationalFact::Generic { envelope: inner, .. } = fact {
                        let source = in_flight_envelope
                            .as_ref()
                            .map(|envelope| envelope.source);
                        if let Some(actor) =
                            aura_social::moderation::facts::claimed_moderation_actor(inner)
                        {
                            if Some(actor) != source {
                                tracing::warn!(
                                    claimed_actor = %actor,
                                    source = ?source,
                                    "Rejected inbound moderation fact whose actor is not its sender"
                                );
                                in_flight_envelope = None;
                                continue;
                            }
                        }
                    }

                    super::channel::InvitationChannelHandler::new(self.handler)
                        .provision_amp_channel_for_inbound_chat_fact(effects.as_ref(), fact)
                        .await;

                    effects
                        .commit_relational_facts(vec![fact.clone()])
                        .await
                        .map_err(|e| AgentError::effects(e.to_string()))?;
                    effects.await_reactive_publications_in_original_window(&budget).await.map_err(AgentError::from)?;
                    if let Err(error) = self.send_delivery_receipt(effects.as_ref(), fact).await {
                        tracing::debug!(error = %error, "message delivery receipt not sent");
                    }

                    processed = processed.saturating_add(1);
                    in_flight_envelope = None;
                    continue;
                }

                if content_type == INVITATION_CONTENT_TYPE {
                    let payload = in_flight_envelope
                        .take()
                        .map(|envelope| envelope.payload)
                        .unwrap_or_default();
                    let code = match String::from_utf8(payload) {
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

                    tracing::info!(
                        authority = %self.handler.context.authority.authority_id(),
                        invitation_code_len = code.len(),
                        "Processing inbound invitation envelope"
                    );

                    match self
                        .handler
                        .import_invitation_code(effects.as_ref(), code)
                        .await
                    {
                        Ok(_invitation) => {
                            tracing::info!(
                                authority = %self.handler.context.authority.authority_id(),
                                invitation_id = %_invitation.invitation_id,
                                "Imported inbound invitation envelope"
                            );
                            self.install_direct_message_bootstrap_if_applicable(
                                effects.as_ref(),
                                &_invitation,
                            )
                            .await;
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

                if let Some(envelope) = in_flight_envelope.take() {
                    deferred_envelopes.push(envelope);
                }
            }

            Ok(())
        })
        .await;

        if let Some(envelope) = in_flight_envelope.take() {
            deferred_envelopes.push(envelope);
        }

        for envelope in deferred_envelopes {
            effects.requeue_envelope(envelope);
        }

        match process_result {
            Ok(()) => Ok(processed),
            Err(TimeoutRunError::Timeout(error)) => {
                tracing::warn!(
                    scanned,
                    processed,
                    error = %error,
                    "contact invitation acceptance processing timed out; requeued remaining envelopes"
                );
                Ok(processed)
            }
            Err(TimeoutRunError::Operation(error)) => Err(error),
        }
    }

    /// Tell the sender of an inbound message that it reached this authority, so
    /// their view advances from sent to delivered.
    async fn send_delivery_receipt(
        &self,
        effects: &AuraEffectSystem,
        fact: &RelationalFact,
    ) -> AgentResult<()> {
        let RelationalFact::Generic { envelope, .. } = fact else {
            return Ok(());
        };
        if envelope.type_id.as_str() != aura_chat::CHAT_FACT_TYPE_ID {
            return Ok(());
        }
        let Some(aura_chat::ChatFact::MessageSentSealed {
            context_id,
            channel_id,
            message_id,
            sender_id,
            ..
        }) = aura_chat::ChatFact::from_envelope(envelope)
        else {
            return Ok(());
        };
        let own_authority = self.handler.context.authority.authority_id();
        if sender_id == own_authority {
            return Ok(());
        }
        let updated_at_ms = InvitationHandler::best_effort_current_timestamp_ms(effects).await;
        let receipt_fact = aura_chat::ChatFact::message_delivery_updated_ms(
            context_id,
            channel_id,
            message_id,
            aura_chat::ChatMessageDeliveryStatus::Delivered,
            updated_at_ms,
            own_authority,
        )
        .to_generic();
        let payload = aura_core::util::serialization::to_vec(&receipt_fact)
            .map_err(|error| AgentError::internal(error.to_string()))?;
        let delivery_context = default_context_id_for_authority(sender_id);
        let mut receipt_envelope = TransportEnvelope {
            destination: sender_id,
            source: own_authority,
            context: delivery_context,
            payload,
            metadata: crate::handlers::shared::build_transport_metadata(
                CHAT_FACT_CONTENT_TYPE,
                [("channel-id", channel_id.to_string())],
            ),
            receipt: super::execute_charge_flow_budget(
                FlowCost::new(1),
                delivery_context,
                sender_id,
                effects,
            )
            .await?
            .map(transport_receipt_from_flow),
        };
        super::attach_invitation_test_receipt_if_needed(effects, &mut receipt_envelope);
        super::execution::attempt_network_send_envelope(
            effects,
            "message delivery receipt send failed",
            receipt_envelope,
        )
        .await
    }

    /// Install the bootstrap key for an inbound direct-message channel invitation.
    ///
    /// Direct chats have no accept step on the receiver, so without this the
    /// receiver never holds the DM epoch-0 key and cannot open messages. Only
    /// invitations for the deterministic pair channel between the sender and
    /// this authority qualify.
    async fn install_direct_message_bootstrap_if_applicable(
        &self,
        effects: &AuraEffectSystem,
        invitation: &Invitation,
    ) {
        let InvitationType::Channel {
            home_id,
            bootstrap: Some(_),
            ..
        } = &invitation.invitation_type
        else {
            return;
        };
        let own_id = self.handler.context.authority.authority_id();
        let dm_channel =
            aura_app::ui::workflows::messaging::pair_dm_channel_id(invitation.sender_id, own_id);
        if *home_id != dm_channel {
            return;
        }
        if let Err(error) = self
            .handler
            .materialize_channel_acceptance_if_needed(effects, &invitation.invitation_id)
            .await
        {
            tracing::warn!(
                error = %error,
                invitation_id = %invitation.invitation_id,
                "Failed to install direct-message bootstrap from inbound invitation"
            );
        }
    }

    pub(super) async fn resolve_contact_invitation(
        &self,
        effects: &AuraEffectSystem,
        invitation_id: &InvitationId,
    ) -> AgentResult<Option<(AuthorityId, String, Option<String>)>> {
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
                return Ok(Some((other, nickname, None)));
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in cache"
            );
        }

        if let Some(stored) =
            InvitationHandler::load_imported_invitation(effects, own_id, invitation_id, None).await
        {
            let shareable = stored.shareable;
            tracing::debug!(
                invitation_id = %invitation_id,
                invitation_type = ?shareable.invitation_type,
                sender_id = %shareable.sender_id,
                "resolve_contact_invitation: found in persisted store"
            );
            if let InvitationType::Contact { nickname } = &shareable.invitation_type {
                if shareable.sender_id != own_id {
                    let other = shareable.sender_id;
                    let nickname = nickname.clone().unwrap_or_else(|| other.to_string());
                    tracing::debug!(
                        contact_id = %other,
                        nickname = %nickname,
                        "resolve_contact_invitation: resolved from persisted store"
                    );
                    return Ok(Some((other, nickname, None)));
                }
            }
        } else {
            tracing::debug!(
                invitation_id = %invitation_id,
                "resolve_contact_invitation: not found in persisted store"
            );
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

            let nickname = nickname.unwrap_or_else(|| sender_id.to_string());

            // No code available via the fact-level fallback path — the
            // invitation fact does not carry the exported code string.
            // The cache and persisted-store paths above already returned
            // with a code when one was derivable.
            return Ok(Some((sender_id, nickname, None)));
        }

        Ok(None)
    }
}

//! AMP channel lifecycle harness for Quint-driven simulations.
//!
//! Drives real Aura agents (Bob, Alice, Carol) with shared transport wiring and
//! maps Quint actions to AMP channel operations (create/invite/accept/join/send/recv/leave).

use super::action_registry::{ActionBuilder, ActionRegistry};
#[path = "amp_transition_model.rs"]
mod transition_model;
use aura_agent::core::{default_context_id_for_authority, AgentBuilder, AgentConfig};
use aura_agent::handlers::{InvitationStatus, InvitationType};
use aura_agent::{AuraAgent, EffectContext, SharedTransport};
use aura_amp::{amp_recv, get_channel_state, AmpJournalEffects};
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::effects::random::RandomCoreEffects;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::transport::TransportError;
use aura_core::effects::{
    time::PhysicalTimeEffects, JournalEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation, ThresholdSigningEffects,
};
use aura_core::effects::{
    ActionEffect, ActionResult, AmpChannelEffects, ChannelCreateParams, ChannelJoinParams,
    ChannelLeaveParams, ChannelSendParams, ExecutionMode, TransportEffects,
};
use aura_core::hash::hash;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId};
use aura_core::{AuraError, Hash32, Result};
use aura_journal::fact::ProtocolRelationalFact;
use aura_journal::fact::{ChannelBootstrap, RelationalFact};
use aura_journal::DomainFact;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

const AMP_MESSAGE_CONTENT_TYPE: &str = "application/aura-amp";

/// A bounded AMP scan borrows unrelated envelopes from their ingress owner.
/// Restore them on success, failure, and cancellation of the scanning future.
struct DeferredIngressEnvelopes<'a> {
    effects: &'a aura_agent::AuraEffectSystem,
    envelopes: Vec<TransportEnvelope>,
}

impl Drop for DeferredIngressEnvelopes<'_> {
    fn drop(&mut self) {
        for envelope in self.envelopes.drain(..) {
            assert_eq!(
                self.effects.requeue_envelope(envelope),
                aura_agent::QueueEnvelopeOutcome::Queued,
                "AMP scan failed to restore an envelope to its ingress owner",
            );
        }
    }
}

/// AMP channel harness using real simulation agents.
pub struct AmpChannelHarness {
    context_id: ContextId,
    agents: HashMap<String, Arc<AuraAgent>>,
    authorities: HashMap<String, AuthorityId>,
    invitation_codes: Mutex<HashMap<(String, String), String>>,
}

impl AmpChannelHarness {
    // Replicate actual committed membership entries in this closed actor fixture.
    // Keep their original keys/order/payload; never construct memberships from
    // trace actor labels or the invariant's expected participant set. Include
    // the original producer checkpoint required by canonical AMP reduction.
    async fn synchronize_committed_membership(
        &self,
        source: &aura_agent::AuraEffectSystem,
        channel: ChannelId,
        participant: AuthorityId,
        transition: aura_amp::ChannelParticipantEvent,
    ) -> Result<()> {
        let journal = source.get_journal().await?;
        let mut delta = aura_core::Journal::new();
        let prefix = format!("relational:{}:", self.context_id);
        let mut selected = 0usize;
        let mut checkpoint_selected = false;
        for (key, value) in journal.read_facts().iter() {
            if !key.as_str().starts_with(&prefix) {
                continue;
            }
            let aura_core::FactValue::Bytes(bytes) = value else {
                return Err(AuraError::invalid(
                    "AMP source relational entry is not canonical bytes",
                ));
            };
            let content: aura_journal::fact::FactContent =
                serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                    message: "decode actual AMP source membership entry".into(),
                    source: Some(Arc::new(source)),
                })?;
            if let aura_journal::fact::FactContent::Relational(RelationalFact::Protocol(
                ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
            )) = &content
            {
                if checkpoint.context == self.context_id && checkpoint.channel == channel {
                    delta.facts.insert(key.clone(), value.clone())?;
                    checkpoint_selected = true;
                }
                continue;
            }
            let aura_journal::fact::FactContent::Relational(RelationalFact::Generic {
                envelope,
                ..
            }) = content
            else {
                continue;
            };
            let Some(membership) = aura_amp::ChannelMembershipFact::from_envelope(&envelope) else {
                continue;
            };
            if membership.context() != self.context_id
                || membership.channel() != channel
                || membership.participant() != participant
                || !matches!(
                    (membership.event(), transition),
                    (
                        aura_amp::ChannelParticipantEvent::Joined,
                        aura_amp::ChannelParticipantEvent::Joined
                    ) | (
                        aura_amp::ChannelParticipantEvent::Left,
                        aura_amp::ChannelParticipantEvent::Left
                    )
                )
            {
                continue;
            }
            delta.facts.insert(key.clone(), value.clone())?;
            selected += 1;
        }
        if selected == 0 {
            return Err(AuraError::invalid(
                "actual AMP membership transition has no committed source entry",
            ));
        }
        if !checkpoint_selected {
            return Err(AuraError::invalid(
                "actual AMP membership transition has no original channel checkpoint",
            ));
        }
        for name in ["bob", "alice", "carol"] {
            let agent = self.agent_for(name)?;
            let target = agent.runtime().effects();
            if std::ptr::eq(source, target.as_ref()) {
                continue;
            }
            let current = target.get_journal().await?;
            let merged = target.merge_facts(current, delta.clone()).await?;
            target.persist_journal(&merged).await?;
            let acknowledged =
                aura_amp::list_channel_participants(target.as_ref(), self.context_id, channel)
                    .await?;
            let expected_presence = matches!(transition, aura_amp::ChannelParticipantEvent::Joined);
            if acknowledged.contains(&participant) != expected_presence {
                return Err(AuraError::invalid(format!(
                    "{name} did not acknowledge original committed AMP {transition:?} for {participant}",
                )));
            }
        }
        Ok(())
    }
    /// Build a new harness with three agents (bob/alice/carol).
    pub async fn new(seed: u64, base_path: PathBuf) -> Result<Arc<Self>> {
        let shared_transport = SharedTransport::new();

        let bob_authority = authority_from_label("bob");
        let alice_authority = authority_from_label("alice");
        let carol_authority = authority_from_label("carol");
        let context_id = default_context_id_for_authority(bob_authority);

        let bob = build_agent(
            seed,
            "bob",
            bob_authority,
            base_path.join("bob"),
            shared_transport.clone(),
        )
        .await?;
        let alice = build_agent(
            seed + 1,
            "alice",
            alice_authority,
            base_path.join("alice"),
            shared_transport.clone(),
        )
        .await?;
        let carol = build_agent(
            seed + 2,
            "carol",
            carol_authority,
            base_path.join("carol"),
            shared_transport.clone(),
        )
        .await?;

        let mut agents = HashMap::new();
        agents.insert("bob".to_string(), bob);
        agents.insert("alice".to_string(), alice);
        agents.insert("carol".to_string(), carol);

        let mut authorities = HashMap::new();
        authorities.insert("bob".to_string(), bob_authority);
        authorities.insert("alice".to_string(), alice_authority);
        authorities.insert("carol".to_string(), carol_authority);

        Ok(Arc::new(Self {
            context_id,
            agents,
            authorities,
            invitation_codes: Mutex::new(HashMap::new()),
        }))
    }

    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn agent_for(&self, name: &str) -> Result<Arc<AuraAgent>> {
        let key = normalize_name(name);
        self.agents
            .get(&key)
            .cloned()
            .ok_or_else(|| AuraError::invalid(format!("unknown agent name: {name}")))
    }

    fn authority_for(&self, name: &str) -> Result<AuthorityId> {
        if let Ok(id) = AuthorityId::from_str(name) {
            return Ok(id);
        }
        let key = normalize_name(name);
        self.authorities
            .get(&key)
            .copied()
            .ok_or_else(|| AuraError::invalid(format!("unknown authority name: {name}")))
    }

    async fn ensure_channel_exists(
        &self,
        effects: &Arc<aura_agent::AuraEffectSystem>,
        channel: ChannelId,
    ) -> Result<()> {
        match get_channel_state(effects.as_ref(), self.context_id, channel).await {
            Ok(_) => return Ok(()),
            Err(error)
                if aura_amp::ChannelStateUnavailable::find(&error).is_some_and(|absence| {
                    absence.context() == self.context_id && absence.channel() == channel
                }) => {}
            Err(error) => return Err(error),
        }

        effects
            .create_channel(ChannelCreateParams {
                context: self.context_id,
                channel: Some(channel),
                skip_window: None,
                topic: None,
            })
            .await
            .map_err(|error| AuraError::Internal {
                message: "create channel failed".into(),
                source: Some(Arc::new(error)),
            })?;

        Ok(())
    }

    async fn ensure_bootstrap(
        &self,
        effects: &Arc<aura_agent::AuraEffectSystem>,
        dealer: AuthorityId,
        channel: ChannelId,
        recipients: Vec<AuthorityId>,
    ) -> Result<ChannelBootstrapPackage> {
        let state = get_channel_state(effects.as_ref(), self.context_id, channel).await?;
        let mut requested_recipients = BTreeSet::new();
        for recipient in recipients {
            requested_recipients.insert(recipient);
        }

        if requested_recipients.is_empty() {
            return Err(AuraError::invalid(
                "AMP bootstrap recipients cannot be empty".to_string(),
            ));
        }

        // Late joiners receive the existing epoch-0 key (docs/112 §1.2).
        if let Some(existing) = state.bootstrap.clone() {
            let location = SecureStorageLocation::amp_bootstrap_key(
                &self.context_id,
                &channel,
                &existing.bootstrap_id,
            );
            let key = effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .map_err(|e| AuraError::internal(format!("bootstrap key read failed: {e}")))?;

            return Ok(ChannelBootstrapPackage {
                bootstrap_id: existing.bootstrap_id,
                key,
            });
        }

        let key_bytes = effects.random_bytes_32().await;
        let bootstrap_id = Hash32::from_bytes(&key_bytes);
        let location =
            SecureStorageLocation::amp_bootstrap_key(&self.context_id, &channel, &bootstrap_id);
        effects
            .secure_store(
                &location,
                &key_bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await
            .map_err(|e| AuraError::internal(format!("bootstrap key write failed: {e}")))?;

        let now = effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time read failed: {e}")))?;
        let bootstrap_fact = ChannelBootstrap {
            context: self.context_id,
            channel,
            bootstrap_id,
            dealer,
            recipients: requested_recipients.into_iter().collect(),
            created_at: now,
            expires_at: None,
        };

        effects
            .insert_relational_fact(RelationalFact::Protocol(
                ProtocolRelationalFact::AmpChannelBootstrap(bootstrap_fact),
            ))
            .await
            .map_err(|e| AuraError::internal(format!("bootstrap fact insert failed: {e}")))?;

        Ok(ChannelBootstrapPackage {
            bootstrap_id,
            key: key_bytes.to_vec(),
        })
    }

    async fn accept_invitation_for_channel(
        &self,
        receiver: &str,
        agent: &AuraAgent,
        channel: ChannelId,
    ) -> Result<()> {
        let invitation_service = agent
            .invitations()
            .map_err(|e| AuraError::internal(format!("invitation service unavailable: {e}")))?;

        let channel_key = channel.to_string();
        let key = (normalize_name(receiver), channel_key);
        let code = {
            let codes = self.invitation_codes.lock().await;
            codes.get(&key).cloned()
        }
        .ok_or_else(|| AuraError::not_found("matching channel invitation not found"))?;

        let invitation = invitation_service
            .import_and_cache(&code)
            .await
            .map_err(|e| AuraError::internal(format!("import invitation: {e}")))?;

        if let InvitationType::Channel { home_id, .. } = &invitation.invitation_type {
            let invite_channel = *home_id;
            if invite_channel != channel {
                return Err(AuraError::invalid("invitation channel mismatch for accept"));
            }
        }

        let result = invitation_service
            .accept(&invitation.invitation_id)
            .await
            .map_err(|e| AuraError::internal(format!("accept invitation: {e}")))?;
        if matches!(result.new_status, InvitationStatus::Accepted) {
            return Ok(());
        }

        Err(AuraError::invalid(format!(
            "invitation acceptance ended in unexpected status: {:?}",
            result.new_status
        )))
    }

    async fn receive_amp_message(
        &self,
        agent: &AuraAgent,
        channel: ChannelId,
        expected_payload: &str,
    ) -> Result<()> {
        let effects = agent.runtime().effects();

        let mut deferred = DeferredIngressEnvelopes {
            effects: effects.as_ref(),
            envelopes: Vec::new(),
        };
        let mut attempts = 0usize;
        while attempts < 64 {
            attempts += 1;
            match effects.receive_envelope().await {
                Ok(envelope) => {
                    let content_type = envelope.metadata.get("content-type");
                    if content_type.is_some_and(|ct| ct == AMP_MESSAGE_CONTENT_TYPE) {
                        self.ensure_channel_exists(&effects, channel).await?;
                        let msg = amp_recv(
                            effects.as_ref(),
                            self.context_id,
                            envelope.source,
                            envelope.payload,
                        )
                        .await
                        .map_err(|e| AuraError::invalid(format!("amp_recv failed: {e}")))?;

                        if msg.header.channel != channel {
                            continue;
                        }

                        let payload = String::from_utf8(msg.payload)
                            .map_err(|e| AuraError::invalid(format!("invalid AMP payload: {e}")))?;
                        if payload != expected_payload {
                            return Err(AuraError::invalid(format!(
                                "AMP payload mismatch: expected '{expected_payload}', got '{payload}'"
                            )));
                        }
                        return Ok(());
                    } else {
                        deferred.envelopes.push(envelope);
                    }
                }
                Err(TransportError::NoMessage) => break,
                Err(err) => {
                    return Err(AuraError::internal(format!(
                        "receive AMP envelope failed: {err}"
                    )))
                }
            }
        }

        Err(AuraError::not_found("AMP message not received"))
    }

    async fn commit_epoch_bump(
        &self,
        channel: ChannelId,
        new_epoch: u64,
        participants: &[Arc<AuraAgent>],
    ) -> Result<()> {
        let effects: Vec<_> = participants
            .iter()
            .map(|agent| agent.runtime().effects())
            .collect();
        let fact = aura_agent::rekey_simulated_channel(self.context_id, channel, &effects).await?;
        if fact.committed().new_epoch != new_epoch {
            return Err(AuraError::invalid(
                "native rekey committed an unexpected epoch",
            ));
        }
        let location =
            SecureStorageLocation::amp_channel_base_key(&self.context_id, &channel, new_epoch);
        let mut expected_key = None;
        for agent in self.agents.values() {
            let effects = agent.runtime().effects();
            let is_member = participants
                .iter()
                .any(|participant| Arc::ptr_eq(participant, agent));
            if is_member {
                let committed = effects.load_committed_facts(agent.authority_id()).await?;
                let evidence = fact.to_generic();
                if !committed.iter().any(|entry| {
                    matches!(&entry.content,
                        aura_journal::fact::FactContent::Relational(actual) if actual == &evidence)
                }) {
                    return Err(AuraError::invalid(
                        "member did not retain the native channel agreement evidence",
                    ));
                }
                let key = effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await?;
                if expected_key
                    .as_ref()
                    .is_some_and(|expected| expected != &key)
                {
                    return Err(AuraError::invalid(
                        "native ceremony produced divergent channel keys",
                    ));
                }
                expected_key = Some(key);
            } else if effects.secure_exists(&location).await? {
                return Err(AuraError::invalid(
                    "departed member retained the successor channel key",
                ));
            }
        }
        Ok(())
    }
}

/// Build an action registry with AMP channel handlers.
pub fn amp_channel_registry(harness: Arc<AmpChannelHarness>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    let transition_harness = harness.clone();

    registry.register(
        ActionBuilder::new("createChannel")
            .description("Create an AMP channel and join as creator")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "creator": {"type": "string"},
                    "cid": {"type": "string"},
                    "actor": {"type": "string"},
                    "channel": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let creator = param_string(params, &["creator", "actor"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let creator =
                            creator.ok_or_else(|| AuraError::invalid("missing creator/actor"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;

                        let agent = harness.agent_for(&creator)?;
                        let authority = harness.authority_for(&creator)?;
                        let channel = channel_id_from_input(&cid);
                        let effects = agent.runtime().effects();

                        effects
                            .create_channel(ChannelCreateParams {
                                context: harness.context_id(),
                                channel: Some(channel),
                                skip_window: None,
                                topic: None,
                            })
                            .await
                            .map_err(|e| {
                                AuraError::invalid(format!("create channel failed: {e}"))
                            })?;

                        effects
                            .join_channel(ChannelJoinParams {
                                context: harness.context_id(),
                                channel,
                                participant: authority,
                            })
                            .await
                            .map_err(|source| AuraError::Internal {
                                message: "commit actual AMP channel join".into(),
                                source: Some(Arc::new(source)),
                            })?;

                        harness
                            .synchronize_committed_membership(
                                effects.as_ref(),
                                channel,
                                authority,
                                aura_amp::ChannelParticipantEvent::Joined,
                            )
                            .await?;

                        // The creation action owns both protocol checkpoint and
                        // canonical chat identity. Invitations consume that
                        // committed identity rather than reconstructing context
                        // from the AMP id or an observed membership event.
                        let created_at =
                            effects
                                .physical_time()
                                .await
                                .map_err(|error| AuraError::Internal {
                                    message: "read channel creation time".into(),
                                    source: Some(Arc::new(error)),
                                })?;
                        let creation = aura_chat::ChatFact::channel_created_ms(
                            harness.context_id(),
                            channel,
                            cid,
                            None,
                            false,
                            created_at.ts_ms,
                            authority,
                        )
                        .to_generic();
                        effects.commit_relational_facts(vec![creation]).await?;

                        // This closed three-actor lifecycle owns its complete
                        // invitation roster before issuing any bootstrap code.
                        let recipients = harness
                            .authorities
                            .values()
                            .copied()
                            .filter(|recipient| *recipient != authority)
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                        harness
                            .ensure_bootstrap(&effects, authority, channel, recipients)
                            .await?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("inviteMember")
            .description("Invite a member to join a channel")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "sender": {"type": "string"},
                    "receiver": {"type": "string"},
                    "cid": {"type": "string"},
                    "actor": {"type": "string"},
                    "member": {"type": "string"},
                    "channel": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let sender = param_string(params, &["sender", "actor"]);
                    let receiver = param_string(params, &["receiver", "member"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let sender =
                            sender.ok_or_else(|| AuraError::invalid("missing sender/actor"))?;
                        let receiver = receiver
                            .ok_or_else(|| AuraError::invalid("missing receiver/member"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;

                        let agent = harness.agent_for(&sender)?;
                        let receiver_id = harness.authority_for(&receiver)?;
                        let dealer_id = harness.authority_for(&sender)?;
                        let channel = channel_id_from_input(&cid);

                        let invitation_service = agent
                            .invitations()
                            .map_err(|e| AuraError::internal(format!("invitation service: {e}")))?;

                        let effects = agent.runtime().effects();
                        let bootstrap = harness
                            .ensure_bootstrap(&effects, dealer_id, channel, vec![receiver_id])
                            .await?;

                        let invitation = invitation_service
                            .invite_to_channel(
                                receiver_id,
                                channel.to_string(),
                                None,
                                Some(channel.to_string()),
                                Some(bootstrap),
                                None,
                                None,
                            )
                            .await
                            .map_err(|source| AuraError::Internal {
                                message: "create actual AMP channel invitation".into(),
                                source: Some(Arc::new(source)),
                            })?;

                        let code = invitation_service
                            .export_invitation_with_sender_hint(&invitation)
                            .await
                            .map_err(|error| AuraError::Internal {
                                message: "export signed channel invitation".into(),
                                source: Some(Arc::new(error)),
                            })?;
                        let key = (normalize_name(&receiver), channel.to_string());
                        {
                            let mut codes = harness.invitation_codes.lock().await;
                            codes.insert(key, code);
                        }

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("acceptInvite")
            .description("Accept a channel invitation")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "receiver": {"type": "string"},
                    "cid": {"type": "string"},
                    "actor": {"type": "string"},
                    "channel": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let receiver = param_string(params, &["receiver", "actor"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let receiver =
                            receiver.ok_or_else(|| AuraError::invalid("missing receiver/actor"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;

                        let agent = harness.agent_for(&receiver)?;
                        let channel = channel_id_from_input(&cid);

                        harness
                            .accept_invitation_for_channel(&receiver, &agent, channel)
                            .await?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("joinChannel")
            .description("Join a channel after accepting invitation")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "participant": {"type": "string"},
                    "cid": {"type": "string"},
                    "actor": {"type": "string"},
                    "member": {"type": "string"},
                    "channel": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let participant = param_string(params, &["participant", "actor", "member"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let participant =
                            participant.ok_or_else(|| AuraError::invalid("missing participant"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;

                        let agent = harness.agent_for(&participant)?;
                        let authority = harness.authority_for(&participant)?;
                        let channel = channel_id_from_input(&cid);
                        let effects = agent.runtime().effects();

                        harness.ensure_channel_exists(&effects, channel).await?;

                        effects
                            .join_channel(ChannelJoinParams {
                                context: harness.context_id(),
                                channel,
                                participant: authority,
                            })
                            .await
                            .map_err(|source| AuraError::Internal {
                                message: "commit joining actor's actual AMP membership".into(),
                                source: Some(Arc::new(source)),
                            })?;

                        harness
                            .synchronize_committed_membership(
                                effects.as_ref(),
                                channel,
                                authority,
                                aura_amp::ChannelParticipantEvent::Joined,
                            )
                            .await?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("sendMessage")
            .description("Send AMP message on channel")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "sender": {"type": "string"},
                    "cid": {"type": "string"},
                    "mid": {"type": "string"},
                    "actor": {"type": "string"},
                    "channel": {"type": "string"},
                    "message": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let sender = param_string(params, &["sender", "actor"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let message = param_string(params, &["mid", "message"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let sender =
                            sender.ok_or_else(|| AuraError::invalid("missing sender/actor"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;
                        let message =
                            message.ok_or_else(|| AuraError::invalid("missing message id"))?;

                        let agent = harness.agent_for(&sender)?;
                        let authority = harness.authority_for(&sender)?;
                        let channel = channel_id_from_input(&cid);
                        let effects = agent.runtime().effects();

                        effects
                            .send_message(ChannelSendParams {
                                context: harness.context_id(),
                                channel,
                                sender: authority,
                                plaintext: message.as_bytes().to_vec(),
                                reply_to: None,
                            })
                            .await
                            .map_err(|e| AuraError::invalid(format!("send message failed: {e}")))?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("receiveMessage")
            .description("Receive AMP message on channel")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "receiver": {"type": "string"},
                    "cid": {"type": "string"},
                    "mid": {"type": "string"},
                    "actor": {"type": "string"},
                    "channel": {"type": "string"},
                    "message": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let receiver = param_string(params, &["receiver", "actor"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let message = param_string(params, &["mid", "message"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let receiver =
                            receiver.ok_or_else(|| AuraError::invalid("missing receiver/actor"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;
                        let message =
                            message.ok_or_else(|| AuraError::invalid("missing message id"))?;

                        let agent = harness.agent_for(&receiver)?;
                        let channel = channel_id_from_input(&cid);
                        harness
                            .receive_amp_message(&agent, channel, &message)
                            .await?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("leaveChannel")
            .description("Leave channel and trigger epoch bump")
            .parameter_schema(json!({
                "type": "object",
                "properties": {
                    "leaver": {"type": "string"},
                    "cid": {"type": "string"},
                    "actor": {"type": "string"},
                    "member": {"type": "string"},
                    "channel": {"type": "string"}
                },
                "required": []
            }))
            .execute_fn({
                let harness = harness.clone();
                move |params, _, state| {
                    let result_state = state.clone();
                    let leaver = param_string(params, &["leaver", "actor", "member"]);
                    let cid = param_string(params, &["cid", "channel"]);
                    let harness = harness.clone();
                    Box::pin(async move {
                        let leaver = leaver.ok_or_else(|| AuraError::invalid("missing leaver"))?;
                        let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;

                        let leaver_agent = harness.agent_for(&leaver)?;
                        let leaver_id = harness.authority_for(&leaver)?;
                        let channel = channel_id_from_input(&cid);

                        let effects = leaver_agent.runtime().effects();
                        effects
                            .leave_channel(ChannelLeaveParams {
                                context: harness.context_id(),
                                channel,
                                participant: leaver_id,
                            })
                            .await
                            .map_err(|source| AuraError::Internal {
                                message: "commit actual AMP channel leave".into(),
                                source: Some(Arc::new(source)),
                            })?;

                        harness
                            .synchronize_committed_membership(
                                effects.as_ref(),
                                channel,
                                leaver_id,
                                aura_amp::ChannelParticipantEvent::Left,
                            )
                            .await?;

                        let bob = harness.agent_for("bob")?;
                        let alice = harness.agent_for("alice")?;

                        let channel_state = get_channel_state(
                            bob.runtime().effects().as_ref(),
                            harness.context_id(),
                            channel,
                        )
                        .await
                        .map_err(|e| AuraError::invalid(format!("state lookup failed: {e}")))?;

                        let new_epoch = channel_state.chan_epoch + 1;
                        harness
                            .commit_epoch_bump(channel, new_epoch, &[bob, alice])
                            .await?;

                        Ok(success_result(result_state, vec![]))
                    })
                }
            })
            .build(),
    );

    registry.register(
        ActionBuilder::new("assertInvariant")
            .description("Check runtime epoch and membership after the channel leave")
            .execute_fn(move |params, _, state| {
                let result_state = state.clone();
                let cid = param_string(params, &["cid", "channel"]);
                let harness = harness.clone();
                Box::pin(async move {
                    let cid = cid.ok_or_else(|| AuraError::invalid("missing channel id"))?;
                    let channel = channel_id_from_input(&cid);
                    let expected = BTreeSet::from([
                        harness.authority_for("bob")?,
                        harness.authority_for("alice")?,
                    ]);
                    for name in ["bob", "alice"] {
                        let agent = harness.agent_for(name)?;
                        let effects = agent.runtime().effects();
                        let current = get_channel_state(
                            effects.as_ref(), harness.context_id(), channel,
                        ).await?;
                        if current.chan_epoch != 1 {
                            return Err(AuraError::invalid(format!(
                                "{name} has epoch {}, expected 1 after Carol leaves",
                                current.chan_epoch,
                            )));
                        }
                        let participants = aura_amp::list_channel_participants(
                            effects.as_ref(), harness.context_id(), channel,
                        ).await?.into_iter().collect::<BTreeSet<_>>();
                        if participants != expected {
                            return Err(AuraError::invalid(format!(
                                "{name} has divergent canonical channel membership: {participants:?}",
                            )));
                        }
                    }
                    Ok(success_result(result_state, vec![]))
                })
            })
            .build(),
    );

    transition_model::register(&mut registry, transition_harness);
    registry
}

fn success_result(state: Value, effects: Vec<ActionEffect>) -> ActionResult {
    ActionResult {
        success: true,
        resulting_state: state,
        effects_produced: effects,
        error: None,
    }
}

fn param_string(params: &Value, keys: &[&str]) -> Option<String> {
    let map = params.as_object()?;
    for key in keys {
        if let Some(value) = map.get(*key).and_then(|v| v.as_str()) {
            return Some(value.to_string());
        }
    }
    None
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn authority_from_label(label: &str) -> AuthorityId {
    let material = format!("amp-harness:{label}:authority");
    AuthorityId::new_from_entropy(hash(material.as_bytes()))
}

fn device_from_label(label: &str) -> DeviceId {
    let material = format!("amp-harness:{label}:device");
    DeviceId::new_from_entropy(hash(material.as_bytes()))
}

fn channel_id_from_input(input: &str) -> ChannelId {
    ChannelId::from_str(input).unwrap_or_else(|_| ChannelId::from_bytes(hash(input.as_bytes())))
}

async fn build_agent(
    seed: u64,
    label: &str,
    authority: AuthorityId,
    base_path: PathBuf,
    shared_transport: SharedTransport,
) -> Result<Arc<AuraAgent>> {
    std::fs::create_dir_all(&base_path)
        .map_err(|e| AuraError::internal(format!("create agent storage directory failed: {e}")))?;

    let mut config = AgentConfig {
        device_id: device_from_label(label),
        ..Default::default()
    };
    config.storage.base_path = base_path;

    let context = default_context_id_for_authority(authority);
    let ctx = EffectContext::new(authority, context, ExecutionMode::Simulation { seed });

    let agent = AgentBuilder::new()
        .with_config(config)
        .with_authority(authority)
        .build_simulation_async_with_shared_transport(seed, &ctx, shared_transport)
        .await
        .map_err(|source| AuraError::Internal {
            message: "build actual AMP simulation agent".into(),
            source: Some(Arc::new(source)),
        })?;

    // Invitation sender custody requires the original protected active epoch
    // and physical signer. The service publishes genuine genesis and key
    // records; channel bootstrap data does not establish an authority identity.
    agent
        .threshold_signing()
        .bootstrap_authority(&authority)
        .await
        .map_err(|source| AuraError::Internal {
            message: "bootstrap actual AMP simulation authority".into(),
            source: Some(Arc::new(source)),
        })?;

    Ok(Arc::new(agent))
}

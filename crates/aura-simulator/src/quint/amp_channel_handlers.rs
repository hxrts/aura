//! AMP channel lifecycle harness for Quint-driven simulations.
//!
//! Drives real Aura agents (Bob, Alice, Carol) with shared transport wiring and
//! maps Quint actions to AMP channel operations (create/invite/accept/join/send/recv/leave).

use super::action_registry::{ActionBuilder, ActionRegistry, NoOpHandler};
use aura_agent::core::{default_context_id_for_authority, AgentBuilder, AgentConfig};
use aura_agent::handlers::{InvitationServiceApi, InvitationType};
use aura_agent::{AuraAgent, EffectContext, SharedTransport};
use aura_amp::{amp_recv, get_channel_state, AmpJournalEffects};
use aura_core::effects::amp::ChannelBootstrapPackage;
use aura_core::effects::random::RandomCoreEffects;
use aura_core::effects::transport::TransportError;
use aura_core::effects::{
    time::PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::effects::{
    ActionEffect, ActionResult, AmpChannelEffects, ChannelCreateParams, ChannelJoinParams,
    ChannelLeaveParams, ChannelSendParams, ExecutionMode, TransportEffects,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId};
use aura_core::{AuraError, Hash32, Result};
use aura_journal::fact::{ChannelBootstrap, CommittedChannelEpochBump, RelationalFact};
use aura_journal::fact::ProtocolRelationalFact;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

const AMP_MESSAGE_CONTENT_TYPE: &str = "application/aura-amp";

/// AMP channel harness using real simulation agents.
pub struct AmpChannelHarness {
    context_id: ContextId,
    agents: HashMap<String, Arc<AuraAgent>>,
    authorities: HashMap<String, AuthorityId>,
    invitation_codes: Mutex<HashMap<(String, String), String>>,
}

impl AmpChannelHarness {
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
        if get_channel_state(effects.as_ref(), self.context_id, channel)
            .await
            .is_ok()
        {
            return Ok(());
        }

        effects
            .create_channel(ChannelCreateParams {
                context: self.context_id,
                channel: Some(channel),
                skip_window: None,
                topic: None,
            })
            .await
            .map_err(|e| AuraError::invalid(format!("create channel failed: {e}")))?;

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

        if let Some(existing) = state.bootstrap.clone() {
            let existing_recipients: BTreeSet<_> = existing.recipients.iter().copied().collect();
            if !requested_recipients.is_subset(&existing_recipients) {
                return Err(AuraError::invalid(
                    "AMP bootstrap already exists; refusing to add new recipients".to_string(),
                ));
            }

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
            let invite_channel = channel_id_from_input(home_id);
            if invite_channel != channel {
                return Err(AuraError::invalid("invitation channel mismatch for accept"));
            }
        }

        let result = invitation_service
            .accept(&invitation.invitation_id)
            .await
            .map_err(|e| AuraError::internal(format!("accept invitation: {e}")))?;
        if result.success {
            return Ok(());
        }

        Err(AuraError::invalid(result.error.unwrap_or_else(|| {
            "invitation acceptance failed".to_string()
        })))
    }

    async fn receive_amp_message(
        &self,
        agent: &AuraAgent,
        channel: ChannelId,
        expected_payload: &str,
    ) -> Result<()> {
        let effects = agent.runtime().effects();

        let mut attempts = 0usize;
        while attempts < 64 {
            attempts += 1;
            match effects.receive_envelope().await {
                Ok(envelope) => {
                    let content_type = envelope.metadata.get("content-type");
                    if content_type.is_some_and(|ct| ct == AMP_MESSAGE_CONTENT_TYPE) {
                        self.ensure_channel_exists(&effects, channel).await?;
                        let msg = amp_recv(effects.as_ref(), self.context_id, envelope.payload)
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
        let bump_id = Hash32::from_bytes(format!("amp-bump:{channel}:{new_epoch}").as_bytes());
        let consensus_id =
            Hash32::from_bytes(format!("amp-consensus:{channel}:{new_epoch}").as_bytes());

        let bump = CommittedChannelEpochBump {
            context: self.context_id,
            channel,
            parent_epoch: new_epoch.saturating_sub(1),
            new_epoch,
            chosen_bump_id: bump_id,
            consensus_id,
            transcript_ref: None,
        };

        for agent in participants {
            let effects = agent.runtime().effects();
            effects
                .insert_relational_fact(RelationalFact::Protocol(
                    ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump.clone()),
                ))
                .await
                .map_err(|e| AuraError::internal(format!("commit bump fact failed: {e}")))?;
        }

        Ok(())
    }
}

/// Build an action registry with AMP channel handlers.
pub fn amp_channel_registry(harness: Arc<AmpChannelHarness>) -> ActionRegistry {
    let mut registry = ActionRegistry::new();

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
                            .map_err(|e| AuraError::invalid(format!("join channel failed: {e}")))?;

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
                                Some(bootstrap),
                                None,
                                None,
                            )
                            .await
                            .map_err(|e| AuraError::internal(format!("invite failed: {e}")))?;

                        let code = InvitationServiceApi::export_invitation(&invitation);
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
                            .map_err(|e| AuraError::invalid(format!("join channel failed: {e}")))?;

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
                let harness = harness;
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
                            .map_err(|e| {
                                AuraError::invalid(format!("leave channel failed: {e}"))
                            })?;

                        let bob = harness.agent_for("bob")?;
                        let alice = harness.agent_for("alice")?;

                        for agent in [bob.clone(), alice.clone()] {
                            let effects = agent.runtime().effects();
                            let _ = effects
                                .leave_channel(ChannelLeaveParams {
                                    context: harness.context_id(),
                                    channel,
                                    participant: leaver_id,
                                })
                                .await;
                        }

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
        NoOpHandler::new("assertInvariant")
            .with_description("No-op assertion marker emitted by Quint AMP harness"),
    );

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
        .map_err(|e| AuraError::internal(format!("build agent failed: {e}")))?;

    Ok(Arc::new(agent))
}

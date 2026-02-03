//! # Demo Simulator (Real Runtime Peers)
//!
//! Demo mode should exercise the same runtime assembly path as production.
//! This simulator instantiates real `AuraAgent` runtimes for Alice, Carol, and a
//! Mobile device peer and runs a small automation loop on their behalf (e.g.,
//! auto-accept guardian setup).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::handlers::InvitationType;
use aura_agent::{AuraAgent, AuraEffectSystem, EffectContext, SharedTransport};
use aura_chat::ChatFact;
use aura_core::effects::{
    AmpChannelEffects, ChannelJoinParams, ChannelSendParams, ExecutionMode, PhysicalTimeEffects,
    TimeEffects, TransportEffects,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_effects::time::PhysicalTimeHandler;
use aura_invitation::{DeviceEnrollmentAccept, DeviceEnrollmentRequest};
use aura_journal::fact::{
    ChannelBootstrap, ChannelCheckpoint, ProtocolRelationalFact, RelationalFact,
};
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_recovery::guardian_ceremony::{CeremonyProposal, CeremonyResponse, CeremonyResponseMsg};
use serde::Serialize;
use std::str::FromStr;

use crate::error::TerminalResult;
use crate::ids;

#[derive(Debug, Clone, Serialize)]
struct GuardianAcceptance {
    guardian_id: AuthorityId,
    setup_id: String,
    accepted: bool,
    public_key: Vec<u8>,
    timestamp: TimeStamp,
}

/// Demo simulator that manages Alice, Carol, and Mobile peer runtimes.
pub struct DemoSimulator {
    seed: u64,
    shared_transport: SharedTransport,
    bob_authority: AuthorityId,
    alice: Arc<AuraAgent>,
    carol: Arc<AuraAgent>,
    mobile: Arc<AuraAgent>,
    event_loop_handle: Option<JoinHandle<()>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl DemoSimulator {
    /// Create a new demo simulator with the given seed and base data dir.
    pub async fn new(
        seed: u64,
        base_path: PathBuf,
        bob_authority: AuthorityId,
        _bob_context: ContextId,
    ) -> TerminalResult<Self> {
        let shared_transport = SharedTransport::new();

        // Peer identities MUST match demo hint derivations.
        let alice_authority = ids::authority_id(&format!("demo:{}:{}:authority", seed, "Alice"));
        let carol_authority =
            ids::authority_id(&format!("demo:{}:{}:authority", seed + 1, "Carol"));
        let mobile_authority =
            ids::authority_id(&format!("demo:{}:{}:authority", seed + 2, "Mobile"));

        let alice_device = ids::device_id(&format!("demo:{}:{}:device", seed, "Alice"));
        let carol_device = ids::device_id(&format!("demo:{}:{}:device", seed + 1, "Carol"));
        let mobile_device = ids::device_id(&format!("demo:{}:{}:device", seed + 2, "Mobile"));

        // Each peer has its own storage sandbox under the demo directory.
        let peers_root = base_path.join("peers");
        let alice_dir = peers_root.join("alice");
        let carol_dir = peers_root.join("carol");
        let mobile_dir = peers_root.join("mobile");

        let (alice, carol, mobile) = tokio::try_join!(
            build_demo_peer_agent(
                seed,
                "Alice",
                alice_authority,
                ids::context_id(&format!("demo:{}:{}:context", seed, "Alice")),
                alice_device,
                alice_dir,
                shared_transport.clone(),
            ),
            build_demo_peer_agent(
                seed + 1,
                "Carol",
                carol_authority,
                ids::context_id(&format!("demo:{}:{}:context", seed + 1, "Carol")),
                carol_device,
                carol_dir,
                shared_transport.clone(),
            ),
            build_demo_peer_agent(
                seed + 2,
                "Mobile",
                mobile_authority,
                ids::context_id(&format!("demo:{}:{}:context", seed + 2, "Mobile")),
                mobile_device,
                mobile_dir,
                shared_transport.clone(),
            )
        )?;

        Ok(Self {
            seed,
            shared_transport,
            bob_authority,
            alice,
            carol,
            mobile,
            event_loop_handle: None,
            shutdown_tx: None,
        })
    }

    /// Access the shared transport wiring used by Bob + peers.
    pub fn shared_transport(&self) -> SharedTransport {
        self.shared_transport.clone()
    }

    /// Get the simulation seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn alice_authority(&self) -> AuthorityId {
        self.alice.authority_id()
    }

    pub fn carol_authority(&self) -> AuthorityId {
        self.carol.authority_id()
    }

    pub fn mobile_authority(&self) -> AuthorityId {
        self.mobile.authority_id()
    }

    pub fn mobile_agent(&self) -> Arc<AuraAgent> {
        self.mobile.clone()
    }

    pub fn alice_agent(&self) -> Arc<AuraAgent> {
        self.alice.clone()
    }

    pub fn carol_agent(&self) -> Arc<AuraAgent> {
        self.carol.clone()
    }

    pub fn mobile_device_id(&self) -> aura_core::DeviceId {
        self.mobile.runtime().device_id()
    }

    /// Start background automation loops for peer runtimes.
    pub async fn start(&mut self) -> TerminalResult<()> {
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        let alice = self.alice.clone();
        let carol = self.carol.clone();
        let mobile = self.mobile.clone();
        let bob_authority = self.bob_authority;
        self.event_loop_handle = Some(tokio::spawn(async move {
            let mut tick = interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    _ = tick.tick() => {
                        let _ = process_peer_transport_messages("Alice", &alice, bob_authority).await;
                        let _ = process_peer_transport_messages("Carol", &carol, bob_authority).await;

                        // Mobile processes transport messages for device enrollment choreography
                        // and ceremony messages for key package installation.
                        let _ = process_peer_transport_messages("Mobile", &mobile, bob_authority).await;
                        let _ = mobile.process_ceremony_acceptances().await;
                    }
                }
            }
        }));

        Ok(())
    }

    pub async fn stop(&mut self) -> TerminalResult<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        if let Some(handle) = self.event_loop_handle.take() {
            let _ = handle.await;
        }
        Ok(())
    }
}

async fn build_demo_peer_agent(
    seed: u64,
    name: &str,
    authority_id: AuthorityId,
    context_id: ContextId,
    device_id: aura_core::DeviceId,
    storage_dir: PathBuf,
    shared_transport: SharedTransport,
) -> TerminalResult<Arc<AuraAgent>> {
    let mut config = AgentConfig::default();
    config.device_id = device_id;
    config.storage.base_path = storage_dir;

    let ctx = EffectContext::new(authority_id, context_id, ExecutionMode::Simulation { seed });

    let agent = AgentBuilder::new()
        .with_config(config)
        .with_authority(authority_id)
        .build_simulation_async_with_shared_transport(seed, &ctx, shared_transport)
        .await
        .map_err(|e| {
            aura_core::AuraError::internal(format!("Failed to build {name} agent: {e}"))
        })?;

    Ok(Arc::new(agent))
}

/// Peer-side automation: currently only guardian setup auto-acceptance.
async fn process_peer_transport_messages(
    name: &str,
    agent: &AuraAgent,
    bob_authority: AuthorityId,
) -> TerminalResult<()> {
    let effects = agent.runtime().effects();

    async fn accept_channel_invitation(
        name: &str,
        agent: &AuraAgent,
        effects: &Arc<AuraEffectSystem>,
        invitation_service: &aura_agent::InvitationServiceApi,
        invitation: &aura_agent::Invitation,
        context: ContextId,
    ) {
        if let Err(err) = invitation_service.accept(&invitation.invitation_id).await {
            tracing::warn!(
                "{name} failed to accept channel invitation {}: {err}",
                invitation.invitation_id
            );
            return;
        }

        let InvitationType::Channel {
            home_id, bootstrap, ..
        } = invitation.invitation_type.clone()
        else {
            return;
        };

        let channel_id = ChannelId::from_str(&home_id)
            .unwrap_or_else(|_| ChannelId::from_bytes(hash(home_id.as_bytes())));

        // Ensure AMP channel state exists locally for decryption.
        if let Some(package) = bootstrap {
            let now = effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: PhysicalTimeHandler::new().physical_time_now_ms(),
                uncertainty: None,
            });
            let window = aura_protocol::amp::config::AmpRuntimeConfig::default()
                .default_skip_window
                .get();

            let checkpoint = ChannelCheckpoint {
                context,
                channel: channel_id,
                chan_epoch: 0,
                base_gen: 0,
                window,
                ck_commitment: aura_core::Hash32::default(),
                skip_window_override: Some(window),
            };

            let bootstrap_fact = ChannelBootstrap {
                context,
                channel: channel_id,
                bootstrap_id: package.bootstrap_id,
                dealer: invitation.sender_id,
                recipients: vec![invitation.sender_id, agent.authority_id()],
                created_at: now,
                expires_at: None,
            };

            if let Err(err) = effects
                .insert_relational_fact(RelationalFact::Protocol(
                    ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
                ))
                .await
            {
                tracing::warn!("{name} failed to seed AMP checkpoint: {err}");
            }

            if let Err(err) = effects
                .insert_relational_fact(RelationalFact::Protocol(
                    ProtocolRelationalFact::AmpChannelBootstrap(bootstrap_fact),
                ))
                .await
            {
                tracing::warn!("{name} failed to seed AMP bootstrap: {err}");
            }
        }

        let params = ChannelJoinParams {
            context,
            channel: channel_id,
            participant: agent.authority_id(),
        };

        if let Err(err) = effects.join_channel(params).await {
            tracing::warn!(
                "{name} failed to join channel {} after accepting invite: {err}",
                home_id
            );
        }
    }

    loop {
        let envelope = match effects.receive_envelope().await {
            Ok(env) => env,
            Err(aura_core::effects::TransportError::NoMessage) => break,
            Err(e) => {
                tracing::warn!("{name} transport receive error: {e}");
                break;
            }
        };

        tracing::debug!(
            "{name} received envelope from {} with content-type {:?}",
            envelope.source,
            envelope.metadata.get("content-type")
        );

        if let Some(content_type) = envelope.metadata.get("content-type").cloned() {
            match content_type.as_str() {
                "application/aura-guardian-proposal" => {
                    if let Some(ceremony_id) = envelope.metadata.get("ceremony-id").cloned() {
                        let mut response_metadata = std::collections::HashMap::new();
                        response_metadata.insert(
                            "content-type".to_string(),
                            "application/aura-guardian-acceptance".to_string(),
                        );
                        response_metadata.insert("ceremony-id".to_string(), ceremony_id.clone());
                        response_metadata
                            .insert("guardian-id".to_string(), agent.authority_id().to_string());
                        if let Ok(bob_device_id) = std::env::var("AURA_DEMO_BOB_DEVICE_ID") {
                            response_metadata
                                .insert("aura-destination-device-id".to_string(), bob_device_id);
                        }

                        let acceptance = GuardianAcceptance {
                            guardian_id: agent.authority_id(),
                            setup_id: ceremony_id,
                            accepted: true,
                            public_key: agent.authority_id().to_bytes().to_vec(),
                            timestamp: TimeStamp::PhysicalClock(PhysicalTime {
                                ts_ms: PhysicalTimeHandler::new().physical_time_now_ms(),
                                uncertainty: None,
                            }),
                        };

                        let payload = serde_json::to_vec(&acceptance).unwrap_or_default();

                        let response = aura_core::effects::TransportEnvelope {
                            destination: envelope.source,
                            source: agent.authority_id(),
                            context: envelope.context,
                            payload,
                            metadata: response_metadata,
                            receipt: None,
                        };

                        if let Err(e) = effects.send_envelope(response).await {
                            tracing::warn!("{name} failed to send guardian acceptance: {e}");
                        }
                    }
                }
                "application/aura-invitation" => {
                    let code = match String::from_utf8(envelope.payload) {
                        Ok(code) => code,
                        Err(err) => {
                            tracing::warn!("{name} received invalid invitation payload: {err}");
                            continue;
                        }
                    };

                    let invitation_service = match agent.invitations() {
                        Ok(service) => service,
                        Err(err) => {
                            tracing::warn!("{name} failed to load invitation service: {err}");
                            continue;
                        }
                    };

                    let invitation = match invitation_service.import_and_cache(&code).await {
                        Ok(invitation) => invitation,
                        Err(err) => {
                            tracing::warn!("{name} failed to import invitation: {err}");
                            continue;
                        }
                    };

                    if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                        accept_channel_invitation(
                            name,
                            agent,
                            &effects,
                            &invitation_service,
                            &invitation,
                            envelope.context,
                        )
                        .await;
                    }
                }
                "application/aura-amp" => {
                    // Only respond to Bob's messages in demo mode.
                    if envelope.source != bob_authority {
                        continue;
                    }

                    let payload = envelope.payload.clone();
                    let context = envelope.context;

                    let message = match aura_protocol::amp::amp_recv(
                        effects.as_ref(),
                        context,
                        payload,
                    )
                    .await
                    {
                        Ok(msg) => msg,
                        Err(err) => {
                            let err_str = err.to_string();
                            if err_str.contains("channel state not found") {
                                tracing::debug!(
                                    "{name} AMP message arrived before channel state; requeuing"
                                );
                                effects.requeue_envelope(envelope);
                                break;
                            }

                            tracing::warn!(
                                "{name} failed to decrypt AMP message from {}: {err}",
                                envelope.source
                            );
                            continue;
                        }
                    };

                    let params = ChannelSendParams {
                        context,
                        channel: message.header.channel,
                        sender: agent.authority_id(),
                        plaintext: message.payload,
                        reply_to: None,
                    };

                    if let Err(err) = effects.send_message(params).await {
                        tracing::warn!("{name} failed to echo AMP message: {err}");
                    } else {
                        tracing::debug!("{name} echoed AMP message back to Bob");
                    }
                }
                "application/aura-choreography" => {
                    // Handle choreography-based guardian ceremony messages
                    tracing::debug!(
                        "{name} received choreography message from {} with {} bytes",
                        envelope.source,
                        envelope.payload.len()
                    );

                    // Try to deserialize as CeremonyProposal (bincode format)
                    if let Ok(proposal) = from_slice::<CeremonyProposal>(&envelope.payload) {
                        tracing::info!(
                            "{name} received guardian ceremony proposal for ceremony {}",
                            proposal.ceremony_id
                        );

                        // Create response accepting the ceremony
                        let response_msg = CeremonyResponseMsg {
                            ceremony_id: proposal.ceremony_id,
                            guardian_id: agent.authority_id(),
                            response: CeremonyResponse::Accept,
                            signature: Vec::new(), // Signature would be added in production
                        };

                        // Serialize response in bincode format (same as choreography uses)
                        let payload = match to_vec(&response_msg) {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!("{name} failed to serialize ceremony response: {e}");
                                continue;
                            }
                        };

                        // Include choreography metadata so the response is routed correctly
                        let mut response_metadata = std::collections::HashMap::new();
                        response_metadata.insert(
                            "content-type".to_string(),
                            "application/aura-choreography".to_string(),
                        );
                        if let Some(session_id) = envelope.metadata.get("session-id") {
                            response_metadata.insert("session-id".to_string(), session_id.clone());
                        }

                        let response = aura_core::effects::TransportEnvelope {
                            destination: envelope.source,
                            source: agent.authority_id(),
                            context: envelope.context,
                            payload,
                            metadata: response_metadata,
                            receipt: None,
                        };

                        if let Err(e) = effects.send_envelope(response).await {
                            tracing::warn!(
                                "{name} failed to send choreography ceremony response: {e}"
                            );
                        } else {
                            tracing::info!(
                                "{name} sent guardian ceremony acceptance for ceremony {}",
                                proposal.ceremony_id
                            );
                        }
                    } else if let Ok(enrollment_request) =
                        from_slice::<DeviceEnrollmentRequest>(&envelope.payload)
                    {
                        // Handle device enrollment choreography request
                        tracing::info!(
                            "{name} received device enrollment request for ceremony {}",
                            enrollment_request.ceremony_id
                        );

                        // Create acceptance response
                        let accept_response = DeviceEnrollmentAccept {
                            invitation_id: enrollment_request.invitation_id,
                            ceremony_id: enrollment_request.ceremony_id,
                            device_id: enrollment_request.device_id,
                        };

                        // Serialize response
                        let payload = match to_vec(&accept_response) {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::warn!(
                                    "{name} failed to serialize device enrollment acceptance: {e}"
                                );
                                continue;
                            }
                        };

                        // Include choreography metadata
                        let mut response_metadata = std::collections::HashMap::new();
                        response_metadata.insert(
                            "content-type".to_string(),
                            "application/aura-choreography".to_string(),
                        );
                        if let Some(session_id) = envelope.metadata.get("session-id") {
                            response_metadata.insert("session-id".to_string(), session_id.clone());
                        }

                        let response = aura_core::effects::TransportEnvelope {
                            destination: envelope.source,
                            source: agent.authority_id(),
                            context: envelope.context,
                            payload,
                            metadata: response_metadata,
                            receipt: None,
                        };

                        if let Err(e) = effects.send_envelope(response).await {
                            tracing::warn!(
                                "{name} failed to send device enrollment acceptance: {e}"
                            );
                        } else {
                            tracing::info!(
                                "{name} sent device enrollment acceptance for ceremony {}",
                                accept_response.ceremony_id
                            );
                        }
                    } else {
                        tracing::debug!(
                            "{name} received choreography message (not recognized), requeuing"
                        );
                        effects.requeue_envelope(envelope);
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    // Auto-accept pending channel invitations for demo peers.
    if let Ok(invitation_service) = agent.invitations() {
        let pending = invitation_service.list_pending().await;
        for invitation in pending {
            if matches!(invitation.invitation_type, InvitationType::Channel { .. }) {
                let context = invitation.context_id;
                accept_channel_invitation(
                    name,
                    agent,
                    &effects,
                    &invitation_service,
                    &invitation,
                    context,
                )
                .await;
            }
        }
    }

    Ok(())
}

/// Peer authority info for echo responses.
#[derive(Clone)]
pub struct EchoPeer {
    /// Authority ID of the peer
    pub authority_id: AuthorityId,
    /// Display name for echo messages
    pub name: String,
}

/// Spawn a background listener that processes AMP inbox traffic for Bob.
///
/// This reads AMP envelopes from Bob's transport inbox, validates/decrypts them,
/// and commits a `ChatFact::MessageSentSealed` into Bob's journal so the UI
/// updates through the normal reactive pipeline.
pub fn spawn_amp_inbox_listener(
    effects: Arc<AuraEffectSystem>,
    bob_authority: AuthorityId,
    peers: Vec<EchoPeer>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut seen_payloads: HashSet<(aura_core::Hash32, AuthorityId)> = HashSet::new();
        let mut tick = interval(Duration::from_millis(50));

        tracing::debug!("Demo AMP inbox: listener started");

        loop {
            tick.tick().await;

            loop {
                let envelope = match effects.receive_envelope().await {
                    Ok(env) => env,
                    Err(aura_core::effects::TransportError::NoMessage) => break,
                    Err(e) => {
                        tracing::warn!("Demo AMP inbox receive error: {e}");
                        break;
                    }
                };

                let Some(content_type) = envelope.metadata.get("content-type").cloned() else {
                    effects.requeue_envelope(envelope);
                    break;
                };

                if content_type.as_str() != "application/aura-amp" {
                    effects.requeue_envelope(envelope);
                    break;
                }

                // Ignore messages sent by Bob (self)
                if envelope.source == bob_authority {
                    continue;
                }

                let payload_hash = aura_core::Hash32::from_bytes(&envelope.payload);
                if !seen_payloads.insert((payload_hash, envelope.source)) {
                    continue;
                }

                // Validate/decrypt to ensure the channel state is correct.
                if let Err(err) = aura_protocol::amp::amp_recv(
                    effects.as_ref(),
                    envelope.context,
                    envelope.payload.clone(),
                )
                .await
                {
                    tracing::warn!("Demo AMP inbox decrypt failed: {err}");
                    continue;
                }

                let wire = match aura_protocol::amp::deserialize_amp_message(&envelope.payload) {
                    Ok(wire) => wire,
                    Err(err) => {
                        tracing::warn!("Demo AMP inbox decode failed: {err}");
                        continue;
                    }
                };

                let sender_name = peers
                    .iter()
                    .find(|peer| peer.authority_id == envelope.source)
                    .map(|peer| peer.name.clone())
                    .unwrap_or_else(|| envelope.source.to_string());

                let timestamp_ms = effects.current_timestamp_ms().await;
                let message_id = format!("amp-{}-{}", payload_hash.to_hex(), envelope.source);

                let fact = ChatFact::message_sent_sealed_ms(
                    envelope.context,
                    wire.header.channel,
                    message_id,
                    envelope.source,
                    sender_name,
                    envelope.payload.clone(),
                    timestamp_ms,
                    None,
                    Some(wire.header.chan_epoch as u32),
                )
                .to_generic();

                if let Err(err) = effects.commit_relational_facts(vec![fact]).await {
                    tracing::warn!("Demo AMP inbox failed to commit chat fact: {err}");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn demo_simulator_builds_peers() {
        let dir = std::env::temp_dir().join("aura-demo-sim-test");
        let bob_authority = ids::authority_id("demo:test:bob:authority");
        let bob_context = ids::context_id("demo:test:bob:context");
        let mut sim = DemoSimulator::new(2024, dir, bob_authority, bob_context)
            .await
            .unwrap();
        sim.start().await.unwrap();
        assert_ne!(sim.alice_authority(), sim.carol_authority());
        assert_ne!(sim.mobile_authority(), sim.alice_authority());
        sim.stop().await.unwrap();
    }
}

//! # Demo Simulator (Real Runtime Peers)
//!
//! Demo mode should exercise the same runtime assembly path as production.
//! This simulator instantiates real `AuraAgent` runtimes for Alice, Carol, and a
//! Mobile device peer and runs a small automation loop on their behalf (e.g.,
//! auto-accept guardian setup).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use async_lock::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::handlers::InvitationType;
use aura_agent::{AuraAgent, AuraEffectSystem, EffectContext, SharedTransport};
use aura_app::AppCore;
use aura_core::effects::{AmpChannelEffects, ChannelJoinParams, ExecutionMode, TransportEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_effects::time::PhysicalTimeHandler;
use aura_effects::ReactiveEffects;
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
        _bob_authority: AuthorityId,
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
        self.event_loop_handle = Some(tokio::spawn(async move {
            let mut tick = interval(Duration::from_millis(100));
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => break,
                    _ = tick.tick() => {
                        let _ = process_peer_transport_messages("Alice", &alice).await;
                        let _ = process_peer_transport_messages("Carol", &carol).await;

                        // Mobile runs ceremony processing for device enrollment participation.
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
async fn process_peer_transport_messages(name: &str, agent: &AuraAgent) -> TerminalResult<()> {
    let effects = agent.runtime().effects();

    loop {
        let envelope = match effects.receive_envelope().await {
            Ok(env) => env,
            Err(aura_core::effects::TransportError::NoMessage) => break,
            Err(e) => {
                tracing::warn!("{name} transport receive error: {e}");
                break;
            }
        };

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

                    if let InvitationType::Channel { home_id, .. } = invitation.invitation_type {
                        if let Err(err) = invitation_service.accept(&invitation.invitation_id).await
                        {
                            tracing::warn!(
                                "{name} failed to accept channel invitation {}: {err}",
                                invitation.invitation_id
                            );
                            continue;
                        }

                        let channel_id = ChannelId::from_str(&home_id)
                            .unwrap_or_else(|_| ChannelId::from_bytes(hash(home_id.as_bytes())));

                        let params = ChannelJoinParams {
                            context: envelope.context,
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
                if let Err(e) = invitation_service.accept(&invitation.invitation_id).await {
                    tracing::warn!(
                        "{name} failed to auto-accept channel invitation {}: {e}",
                        invitation.invitation_id
                    );
                } else {
                    tracing::info!(
                        "{name} auto-accepted channel invitation {}",
                        invitation.invitation_id
                    );
                }
            }
        }
    }

    Ok(())
}

/// Spawn a background listener that echoes messages from Bob back through demo peers.
///
/// This listens to chat state changes and when Bob sends a message, Alice and Carol
/// will respond with an echo message through the AMP channel.
pub fn spawn_amp_echo_listener(
    _shared_transport: SharedTransport,
    bob_authority: AuthorityId,
    _bob_device_id: String,
    app_core: Arc<RwLock<AppCore>>,
    effects: Arc<AuraEffectSystem>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        use aura_app::ui::signals::{CHAT_SIGNAL, HOMES_SIGNAL};
        use aura_core::effects::amp::ChannelSendParams;

        let mut chat_stream = {
            let core = app_core.read().await;
            core.subscribe(&*CHAT_SIGNAL)
        };

        let mut seen_messages: HashSet<String> = HashSet::new();

        loop {
            let chat_state = match chat_stream.recv().await {
                Ok(state) => state,
                Err(_) => break,
            };

            for msg in &chat_state.messages {
                // Only echo Bob's messages
                if msg.sender_id != bob_authority {
                    continue;
                }
                // Don't echo the same message twice
                if !seen_messages.insert(msg.id.clone()) {
                    continue;
                }

                let context_id = {
                    let core = app_core.read().await;
                    let homes = core.read(&*HOMES_SIGNAL).await.unwrap_or_default();
                    homes
                        .home_state(&msg.channel_id)
                        .and_then(|home| home.context_id)
                        .unwrap_or_else(|| {
                            EffectContext::with_authority(bob_authority).context_id()
                        })
                };

                let reply = format!("received: {}", msg.content);

                // Send echo reply through AMP effects
                let params = ChannelSendParams {
                    context: context_id,
                    channel: msg.channel_id,
                    sender: bob_authority, // Echo as if from Bob's perspective
                    plaintext: reply.as_bytes().to_vec(),
                    reply_to: None,
                };

                if let Err(err) = effects.send_message(params).await {
                    tracing::warn!(
                        error = %err,
                        "Demo echo send failed"
                    );
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

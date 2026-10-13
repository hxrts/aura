//! Relay transport: the relay-first baseline of `docs/113_rendezvous.md` §3.2.
//!
//! Two roles share this module.
//!
//! A peer behind a NAT keeps one outbound TCP connection to a relay-capable
//! node open ([`RelaySessionService`]). It registers its authority and device
//! over that connection, keeps it alive, and reads the frames the relay pushes
//! down it into the ordinary runtime inbox. While registered it advertises a
//! `WebSocketRelay` transport hint naming the relay authority, so peers that
//! cannot reach it directly dial the relay instead. The relay's own descriptor
//! travels in the registration acknowledgment; no address enters a hint.
//!
//! A relay-capable node ([`RelayForwarderActor`]) accepts those registrations on
//! its LAN listener and forwards every frame addressed to a registered
//! authority down that authority's session connection. Frames stay opaque:
//! the relay reads only the routing header, never the payload, and the
//! guard-chain receipt the sender attached travels with the frame, so the
//! receiver's LAN integrity check is unchanged.
//!
//! Registration binds an authority id to a connection without proving the
//! registrant holds that authority's keys; a false registration can only
//! divert or drop frames whose payloads are end-to-end protected. The relay
//! forwards only to registered destinations and only from accepted
//! connections.

use super::services::lan_transport::LanTransportMetrics;
use super::services::service_actor::{validate_actor_transition, ActorLifecyclePhase};
use super::services::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use super::services::ServiceRegistry;
use super::system::{handle_inbound_transport_envelope, publish_lan_descriptor_with};
use super::AuraEffectSystem;
use crate::core::default_context_id_for_authority;
use crate::task_registry::TaskGroup;
use async_trait::async_trait;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::types::identifiers::{AuthorityId, DeviceId};
use aura_protocol::handlers::BackoffWindow;
use aura_rendezvous::TransportHint;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, RwLock};

/// Content type of relay control frames. They never enter the runtime inbox.
pub const RELAY_CONTROL_CONTENT_TYPE: &str = "application/aura-relay-control";
/// Metadata key naming the control operation.
pub const RELAY_OP_METADATA_KEY: &str = "aura-relay-op";
/// Peer → relay: bind this connection to the frame's source authority/device.
pub const RELAY_OP_REGISTER: &str = "register";
/// Relay → peer: registration accepted; the frame's source is the relay.
pub const RELAY_OP_REGISTERED: &str = "registered";
/// Peer → relay: keep the NAT mapping and the registration alive.
pub const RELAY_OP_KEEPALIVE: &str = "keepalive";
/// Interval between keepalive frames on a registered session.
pub const RELAY_KEEPALIVE_INTERVAL_MS: u64 = 15_000;
/// Bound on frames queued toward one registered session before the relay drops.
const RELAY_SESSION_QUEUE_CAPACITY: usize = 64;
/// Largest frame the relay reads or writes (matches the LAN listener bound).
const MAX_RELAY_FRAME_BYTES: usize = 128 * 1024;
/// Connect timeout for a session toward the relay.
const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RELAY_RECONNECT_BASE_BACKOFF: Duration = Duration::from_secs(1);
const RELAY_RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
const DEVICE_METADATA_KEY: &str = "aura-source-device-id";
const DESTINATION_DEVICE_METADATA_KEY: &str = "aura-destination-device-id";

/// Whether `envelope` is a relay control frame.
pub fn is_relay_control(envelope: &TransportEnvelope) -> bool {
    envelope
        .metadata
        .get("content-type")
        .is_some_and(|value| value == RELAY_CONTROL_CONTENT_TYPE)
}

fn relay_op(envelope: &TransportEnvelope) -> Option<&str> {
    envelope
        .metadata
        .get(RELAY_OP_METADATA_KEY)
        .map(String::as_str)
}

fn control_frame(
    source: AuthorityId,
    destination: AuthorityId,
    device_id: DeviceId,
    op: &str,
) -> TransportEnvelope {
    let mut metadata = HashMap::new();
    metadata.insert(
        "content-type".to_string(),
        RELAY_CONTROL_CONTENT_TYPE.to_string(),
    );
    metadata.insert(RELAY_OP_METADATA_KEY.to_string(), op.to_string());
    metadata.insert(DEVICE_METADATA_KEY.to_string(), device_id.to_string());
    TransportEnvelope {
        destination,
        source,
        context: default_context_id_for_authority(source),
        payload: Vec::new(),
        metadata,
        receipt: None,
    }
}

/// Bound `f` by `timeout` on the runtime's selected physical clock. The relay
/// session owns these deadlines: a connect or acknowledgment that outlives its
/// budget fails the attempt, and the session loop's bounded backoff decides
/// when to try again.
async fn relay_timeout<F, Fut, T>(
    time: &(impl PhysicalTimeEffects + ?Sized),
    timeout: Duration,
    what: &str,
    f: F,
) -> Result<T, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let started_at = time
        .physical_time()
        .await
        .map_err(|error| format!("{what}: clock unavailable: {error}"))?;
    let budget = aura_core::TimeoutBudget::from_start_and_timeout(&started_at, timeout)
        .map_err(|error| format!("{what}: {error}"))?;
    aura_core::execute_with_timeout_budget(time, &budget, f)
        .await
        .map_err(|error| match error {
            aura_core::TimeoutRunError::Timeout(_) => format!("{what} timed out"),
            aura_core::TimeoutRunError::Operation(error) => error,
        })
}

/// Write one length-prefixed frame.
pub(crate) async fn write_frame(
    stream: &mut TcpStream,
    envelope: &TransportEnvelope,
) -> Result<(), String> {
    let payload = aura_core::util::serialization::to_vec(envelope)
        .map_err(|error| format!("frame encode failed: {error}"))?;
    if payload.len() > MAX_RELAY_FRAME_BYTES {
        return Err(format!(
            "frame of {} bytes exceeds relay bound",
            payload.len()
        ));
    }
    let len = (payload.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .map_err(|error| format!("frame length write failed: {error}"))?;
    stream
        .write_all(&payload)
        .await
        .map_err(|error| format!("frame write failed: {error}"))?;
    stream
        .flush()
        .await
        .map_err(|error| format!("frame flush failed: {error}"))
}

/// Read one length-prefixed frame. `Ok(None)` is a clean end of stream.
pub(crate) async fn read_frame(
    stream: &mut TcpStream,
) -> Result<Option<TransportEnvelope>, String> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("frame length read failed: {error}")),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_RELAY_FRAME_BYTES {
        return Err(format!("invalid relay frame size {len}"));
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|error| format!("frame read failed: {error}"))?;
    aura_core::util::serialization::from_slice(&payload)
        .map(Some)
        .map_err(|error| format!("frame decode failed: {error}"))
}

// =============================================================================
// Relay side
// =============================================================================

/// Why a frame was not forwarded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelayForwardError {
    /// No registered session for the destination authority (and device).
    #[error("no registered relay session for {destination}")]
    NotRegistered { destination: AuthorityId },
    /// Every matching session's queue is full or closed.
    #[error("relay session queue for {destination} is full or closed")]
    SessionUnavailable { destination: AuthorityId },
}

struct RegisteredSession {
    sender: mpsc::Sender<TransportEnvelope>,
    generation: u64,
}

#[allow(dead_code)] // Declaration-layer ingress inventory; connection tasks call methods directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelayForwarderCommand {
    Register,
    Unregister,
    Forward,
}

/// Relay-side registry of connected peers and the forwarding decision. The
/// relay node's LAN connection tasks are its only callers; each registration
/// is owned by the connection task that created it and removed when that task
/// ends.
#[derive(Default)]
#[aura_macros::actor_owned(
    owner = "relay_forwarder",
    domain = "relay_transport",
    gate = "relay_forwarder_command_ingress",
    command = RelayForwarderCommand,
    capacity = 64,
    category = "actor_owned"
)]
pub struct RelayForwarderActor {
    sessions: parking_lot::RwLock<HashMap<AuthorityId, HashMap<String, RegisteredSession>>>,
    next_generation: AtomicU64,
    forwarded: AtomicU64,
    dropped: AtomicU64,
}

impl RelayForwarderActor {
    /// Bind `(authority, device)` to `sender`; a later registration for the
    /// same pair replaces an earlier one. Returns the registration generation
    /// the owner must present to unregister.
    pub fn register(
        &self,
        authority: AuthorityId,
        device: String,
        sender: mpsc::Sender<TransportEnvelope>,
    ) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.sessions
            .write()
            .entry(authority)
            .or_default()
            .insert(device, RegisteredSession { sender, generation });
        generation
    }

    /// Remove the registration if it is still the one created with `generation`.
    pub fn unregister(&self, authority: AuthorityId, device: &str, generation: u64) {
        let mut sessions = self.sessions.write();
        let Some(devices) = sessions.get_mut(&authority) else {
            return;
        };
        if devices
            .get(device)
            .is_some_and(|session| session.generation == generation)
        {
            devices.remove(device);
        }
        if devices.is_empty() {
            sessions.remove(&authority);
        }
    }

    /// Number of registered sessions.
    pub fn registered_sessions(&self) -> usize {
        self.sessions.read().values().map(HashMap::len).sum()
    }

    /// Frames forwarded and frames dropped since start.
    pub fn counters(&self) -> (u64, u64) {
        (
            self.forwarded.load(Ordering::SeqCst),
            self.dropped.load(Ordering::SeqCst),
        )
    }

    /// Queue `envelope` toward its destination's registered session(s). A
    /// device-addressed frame goes to that device only; an authority-addressed
    /// frame goes to every registered device of the authority.
    pub fn forward(&self, envelope: TransportEnvelope) -> Result<(), RelayForwardError> {
        let destination = envelope.destination;
        let device_filter = envelope
            .metadata
            .get(DESTINATION_DEVICE_METADATA_KEY)
            .cloned();
        let sessions = self.sessions.read();
        let Some(devices) = sessions.get(&destination) else {
            self.dropped.fetch_add(1, Ordering::SeqCst);
            return Err(RelayForwardError::NotRegistered { destination });
        };
        let targets: Vec<&RegisteredSession> = devices
            .iter()
            .filter(|(device, _)| {
                device_filter
                    .as_ref()
                    .is_none_or(|wanted| *device == wanted)
            })
            .map(|(_, session)| session)
            .collect();
        if targets.is_empty() {
            self.dropped.fetch_add(1, Ordering::SeqCst);
            return Err(RelayForwardError::NotRegistered { destination });
        }
        let mut delivered = 0usize;
        for session in targets {
            if session.sender.try_send(envelope.clone()).is_ok() {
                delivered += 1;
            }
        }
        if delivered == 0 {
            self.dropped.fetch_add(1, Ordering::SeqCst);
            return Err(RelayForwardError::SessionUnavailable { destination });
        }
        self.forwarded.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Own an accepted LAN connection whose first frame was a relay registration:
/// acknowledge it, keep forwarding frames to the registrant until the
/// connection ends, and forward frames the registrant sends for other
/// registered peers.
pub(crate) async fn serve_relay_connection(
    mut stream: TcpStream,
    first: TransportEnvelope,
    effects: Arc<AuraEffectSystem>,
    forwarder: Arc<RelayForwarderActor>,
) {
    let relay_authority = effects.runtime_authority_id();
    let relay_device = effects.device_id();
    if relay_op(&first) != Some(RELAY_OP_REGISTER) {
        tracing::debug!(
            op = ?relay_op(&first),
            "relay connection opened without a registration frame"
        );
        return;
    }
    let registrant = first.source;
    let device = first
        .metadata
        .get(DEVICE_METADATA_KEY)
        .cloned()
        .unwrap_or_default();
    let (sender, mut receiver) = mpsc::channel(RELAY_SESSION_QUEUE_CAPACITY);
    let generation = forwarder.register(registrant, device.clone(), sender);
    tracing::info!(
        registrant = %registrant,
        device = %device,
        sessions = forwarder.registered_sessions(),
        "relay session registered"
    );
    // The acknowledgment carries the relay's own descriptor so the registrant
    // can resolve the relay authority to a dialable address; no address ever
    // enters a relay hint.
    let mut ack = control_frame(
        relay_authority,
        registrant,
        relay_device,
        RELAY_OP_REGISTERED,
    );
    if let Some(descriptor) = relay_own_descriptor(&effects).await {
        match aura_core::util::serialization::to_vec(&descriptor) {
            Ok(payload) => ack.payload = payload,
            Err(error) => tracing::debug!(
                error = %error,
                "relay descriptor could not be encoded into the registration ack"
            ),
        }
    }
    if let Err(error) = write_frame(&mut stream, &ack).await {
        tracing::debug!(registrant = %registrant, error = %error, "relay registration ack failed");
        forwarder.unregister(registrant, &device, generation);
        return;
    }
    loop {
        tokio::select! {
            frame = read_frame(&mut stream) => match frame {
                Ok(Some(envelope)) => {
                    if is_relay_control(&envelope) {
                        if let Err(error) = write_frame(&mut stream, &ack).await {
                            tracing::debug!(registrant = %registrant, error = %error, "relay keepalive ack failed");
                            break;
                        }
                    } else if envelope.destination == relay_authority {
                        tracing::debug!(registrant = %registrant, "relay session frame addressed to the relay itself was dropped");
                    } else if let Err(error) = forwarder.forward(envelope) {
                        tracing::debug!(registrant = %registrant, error = %error, "relay session frame not forwarded");
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::debug!(registrant = %registrant, error = %error, "relay session read ended");
                    break;
                }
            },
            queued = receiver.recv() => match queued {
                Some(envelope) => {
                    if let Err(error) = write_frame(&mut stream, &envelope).await {
                        tracing::debug!(registrant = %registrant, error = %error, "relay forward write failed");
                        break;
                    }
                }
                None => break,
            },
        }
    }
    forwarder.unregister(registrant, &device, generation);
    tracing::info!(registrant = %registrant, device = %device, "relay session ended");
}

/// The relay's own published descriptor, cached under its authority context
/// when the LAN descriptor was installed.
async fn relay_own_descriptor(
    effects: &AuraEffectSystem,
) -> Option<aura_rendezvous::RendezvousDescriptor> {
    let manager = effects.rendezvous_manager()?;
    let authority = effects.runtime_authority_id();
    manager
        .get_descriptor(default_context_id_for_authority(authority), authority)
        .await
}

/// Cache the relay descriptor an acknowledgment carried, if any.
async fn adopt_relay_descriptor(effects: &AuraEffectSystem, ack: &TransportEnvelope) {
    if ack.payload.is_empty() {
        return;
    }
    let Some(manager) = effects.rendezvous_manager() else {
        return;
    };
    match aura_core::util::serialization::from_slice::<aura_rendezvous::RendezvousDescriptor>(
        &ack.payload,
    ) {
        Ok(descriptor) if descriptor.authority_id == ack.source => {
            if let Err(error) = manager.cache_descriptor(descriptor).await {
                tracing::debug!(error = %error, "relay descriptor from the registration ack was not cached");
            }
        }
        Ok(descriptor) => tracing::debug!(
            claimed = %descriptor.authority_id,
            relay = %ack.source,
            "relay ack carried a descriptor for another authority; ignored"
        ),
        Err(error) => tracing::debug!(error = %error, "relay ack descriptor did not decode"),
    }
}

// =============================================================================
// Peer side
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelaySessionState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

impl RelaySessionState {
    fn phase(self) -> ActorLifecyclePhase {
        match self {
            Self::Stopped => ActorLifecyclePhase::Stopped,
            Self::Starting => ActorLifecyclePhase::Starting,
            Self::Running => ActorLifecyclePhase::Running,
            Self::Stopping => ActorLifecyclePhase::Stopping,
            Self::Failed => ActorLifecyclePhase::Failed,
        }
    }
}

struct RelaySessionShared {
    relay_authority: parking_lot::RwLock<Option<AuthorityId>>,
    connections: AtomicU64,
    tasks: RwLock<Option<TaskGroup>>,
    state: RwLock<RelaySessionState>,
    lifecycle: Mutex<()>,
}

/// The peer's registered session with its configured relay.
#[derive(Clone)]
#[aura_macros::actor_root(
    owner = "relay_session_service",
    domain = "relay_transport",
    supervision = "relay_session_task_root",
    category = "actor_owned"
)]
pub struct RelaySessionService {
    effects: Arc<AuraEffectSystem>,
    registry: Arc<ServiceRegistry>,
    relay_addr: String,
    authority_id: AuthorityId,
    device_id: DeviceId,
    shared: Arc<RelaySessionShared>,
}

impl RelaySessionService {
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        registry: Arc<ServiceRegistry>,
        relay_addr: String,
        authority_id: AuthorityId,
        device_id: DeviceId,
    ) -> Self {
        Self {
            effects,
            registry,
            relay_addr,
            authority_id,
            device_id,
            shared: Arc::new(RelaySessionShared {
                relay_authority: parking_lot::RwLock::new(None),
                connections: AtomicU64::new(0),
                tasks: RwLock::new(None),
                state: RwLock::new(RelaySessionState::Stopped),
                lifecycle: Mutex::new(()),
            }),
        }
    }

    /// Address this peer dials to reach its relay.
    pub fn relay_addr(&self) -> &str {
        &self.relay_addr
    }

    /// The relay's authority, learned from its registration acknowledgment.
    pub fn relay_authority(&self) -> Option<AuthorityId> {
        *self.shared.relay_authority.read()
    }

    /// Sessions established so far (the first is not a network change).
    pub fn connections(&self) -> u64 {
        self.shared.connections.load(Ordering::SeqCst)
    }

    /// Transport hint to advertise while registered: reach me through this
    /// relay authority. The hint names no address; senders resolve the relay
    /// from its own descriptor. `None` until the relay has acknowledged.
    pub fn advertised_hint(&self) -> Option<TransportHint> {
        self.relay_authority().map(TransportHint::websocket_relay)
    }

    async fn mark_state(&self, next: RelaySessionState) {
        *self.shared.state.write().await = next;
    }

    async fn connect_and_register(&self) -> Result<(TcpStream, AuthorityId), String> {
        let socket_addr: std::net::SocketAddr = self
            .relay_addr
            .parse()
            .map_err(|error| format!("invalid relay address '{}': {error}", self.relay_addr))?;
        let mut stream = relay_timeout(
            self.effects.time_effects(),
            RELAY_CONNECT_TIMEOUT,
            "relay connect",
            || async {
                TcpStream::connect(socket_addr)
                    .await
                    .map_err(|error| format!("relay connect failed: {error}"))
            },
        )
        .await?;
        let register = control_frame(
            self.authority_id,
            self.authority_id,
            self.device_id,
            RELAY_OP_REGISTER,
        );
        write_frame(&mut stream, &register).await?;
        let ack = relay_timeout(
            self.effects.time_effects(),
            RELAY_CONNECT_TIMEOUT,
            "relay registration ack",
            || read_frame(&mut stream),
        )
        .await?
        .ok_or_else(|| "relay closed before acknowledging registration".to_string())?;
        if !is_relay_control(&ack) || relay_op(&ack) != Some(RELAY_OP_REGISTERED) {
            return Err("relay answered registration with an unexpected frame".to_string());
        }
        adopt_relay_descriptor(&self.effects, &ack).await;
        Ok((stream, ack.source))
    }

    /// Republish the LAN descriptor so it carries (or drops) the relay hint.
    async fn republish_descriptor(&self) {
        let (Some(rendezvous_manager), Some(lan_transport)) = (
            self.effects.rendezvous_manager(),
            self.effects.lan_transport(),
        ) else {
            return;
        };
        if let Err(error) = publish_lan_descriptor_with(
            self.effects.clone(),
            self.authority_id,
            self.device_id,
            &rendezvous_manager,
            lan_transport.as_ref(),
        )
        .await
        {
            // Before account bootstrap there is no identity to sign with; the
            // bootstrap path publishes the descriptor itself.
            tracing::debug!(error = %error, "relay session could not republish the LAN descriptor");
        }
    }

    async fn run_registered(&self, mut stream: TcpStream) {
        let metrics: Arc<RwLock<LanTransportMetrics>> = self
            .effects
            .lan_transport()
            .map(|lan| lan.metrics_handle())
            .unwrap_or_default();
        let time = self.effects.time_effects().clone();
        let keepalive = control_frame(
            self.authority_id,
            self.authority_id,
            self.device_id,
            RELAY_OP_KEEPALIVE,
        );
        // A keepalive the relay never acknowledges means the path is dead even
        // when the socket has not noticed (a flushed NAT mapping, for one).
        let mut awaiting_ack = false;
        loop {
            tokio::select! {
                frame = read_frame(&mut stream) => match frame {
                    Ok(Some(envelope)) => {
                        if is_relay_control(&envelope) {
                            if relay_op(&envelope) == Some(RELAY_OP_REGISTERED) {
                                *self.shared.relay_authority.write() = Some(envelope.source);
                                adopt_relay_descriptor(&self.effects, &envelope).await;
                                awaiting_ack = false;
                            }
                            continue;
                        }
                        let _ = handle_inbound_transport_envelope(
                            self.effects.clone(),
                            metrics.clone(),
                            envelope,
                        )
                        .await;
                    }
                    Ok(None) => {
                        tracing::info!(relay_addr = %self.relay_addr, "relay closed the session");
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(relay_addr = %self.relay_addr, error = %error, "relay session read failed");
                        return;
                    }
                },
                _ = time.sleep_ms(RELAY_KEEPALIVE_INTERVAL_MS) => {
                    if awaiting_ack {
                        tracing::warn!(relay_addr = %self.relay_addr, "relay did not acknowledge the last keepalive; re-establishing the session");
                        return;
                    }
                    if let Err(error) = write_frame(&mut stream, &keepalive).await {
                        tracing::warn!(relay_addr = %self.relay_addr, error = %error, "relay keepalive failed");
                        return;
                    }
                    awaiting_ack = true;
                }
            }
        }
    }

    async fn session_loop(self) {
        let backoff = BackoffWindow::new(RELAY_RECONNECT_BASE_BACKOFF, RELAY_RECONNECT_MAX_BACKOFF);
        let time = self.effects.time_effects().clone();
        let mut failures: u8 = 0;
        loop {
            match self.connect_and_register().await {
                Ok((stream, relay_authority)) => {
                    failures = 0;
                    *self.shared.relay_authority.write() = Some(relay_authority);
                    let connections = self.shared.connections.fetch_add(1, Ordering::SeqCst) + 1;
                    if connections > 1 {
                        let generation = self.registry.bump_network_generation();
                        tracing::info!(
                            relay = %relay_authority,
                            network_generation = generation,
                            "relay session re-established; network generation advanced"
                        );
                    } else {
                        tracing::info!(relay = %relay_authority, relay_addr = %self.relay_addr, "relay session established");
                    }
                    self.republish_descriptor().await;
                    self.run_registered(stream).await;
                }
                Err(error) => {
                    tracing::warn!(relay_addr = %self.relay_addr, error = %error, "relay session attempt failed");
                }
            }
            let delay = backoff.jittered_delay(failures);
            failures = failures.saturating_add(1).min(8);
            if time.sleep_ms(delay.as_millis() as u64).await.is_err() {
                // The clock refused to wait: do not spin.
                return;
            }
        }
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == RelaySessionState::Running {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Starting)?;
        self.mark_state(RelaySessionState::Starting).await;
        let tasks = context.tasks().group(self.name());
        let _session_task =
            tasks.spawn_cancellable_named("relay_session", self.clone().session_loop());
        *self.shared.tasks.write().await = Some(tasks);
        self.mark_state(RelaySessionState::Running).await;
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _guard = self.shared.lifecycle.lock().await;
        let current = *self.shared.state.read().await;
        if current == RelaySessionState::Stopped {
            return Ok(());
        }
        validate_actor_transition(self.name(), current.phase(), ActorLifecyclePhase::Stopping)?;
        self.mark_state(RelaySessionState::Stopping).await;
        let shutdown_error = if let Some(tasks) = self.shared.tasks.write().await.take() {
            tasks
                .shutdown_with_timeout(Duration::from_secs(2))
                .await
                .err()
                .map(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("failed to stop relay session task group: {error}"),
                    )
                })
        } else {
            None
        };
        match shutdown_error {
            Some(error) => {
                self.mark_state(RelaySessionState::Failed).await;
                Err(error)
            }
            None => {
                self.mark_state(RelaySessionState::Stopped).await;
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RuntimeService for RelaySessionService {
    fn name(&self) -> &'static str {
        "relay_session"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["rendezvous_manager"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        match *self.shared.state.read().await {
            RelaySessionState::Stopped => ServiceHealth::Stopped,
            RelaySessionState::Starting => ServiceHealth::Starting,
            RelaySessionState::Running => ServiceHealth::Healthy,
            RelaySessionState::Stopping => ServiceHealth::Stopping,
            RelaySessionState::Failed => ServiceHealth::Unhealthy {
                reason: "relay session entered failed lifecycle state".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(destination: AuthorityId, device: Option<&str>) -> TransportEnvelope {
        let mut metadata = HashMap::new();
        metadata.insert("content-type".to_string(), "application/test".to_string());
        if let Some(device) = device {
            metadata.insert(
                DESTINATION_DEVICE_METADATA_KEY.to_string(),
                device.to_string(),
            );
        }
        TransportEnvelope {
            destination,
            source: AuthorityId::new_from_entropy([9u8; 32]),
            context: default_context_id_for_authority(destination),
            payload: vec![1, 2, 3],
            metadata,
            receipt: None,
        }
    }

    #[test]
    fn forwarder_routes_by_destination_and_device() {
        let forwarder = RelayForwarderActor::default();
        let bob = AuthorityId::new_from_entropy([1u8; 32]);
        let (phone_tx, mut phone_rx) = mpsc::channel(4);
        let (laptop_tx, mut laptop_rx) = mpsc::channel(4);
        forwarder.register(bob, "phone".to_string(), phone_tx);
        forwarder.register(bob, "laptop".to_string(), laptop_tx);
        assert_eq!(forwarder.registered_sessions(), 2);

        forwarder
            .forward(envelope(bob, None))
            .expect("authority-addressed frame forwards");
        assert!(phone_rx.try_recv().is_ok());
        assert!(laptop_rx.try_recv().is_ok());

        forwarder
            .forward(envelope(bob, Some("phone")))
            .expect("device-addressed frame forwards");
        assert!(phone_rx.try_recv().is_ok());
        assert!(laptop_rx.try_recv().is_err());

        let stranger = AuthorityId::new_from_entropy([2u8; 32]);
        assert_eq!(
            forwarder.forward(envelope(stranger, None)),
            Err(RelayForwardError::NotRegistered {
                destination: stranger
            })
        );
        assert_eq!(forwarder.counters(), (2, 1));
    }

    #[test]
    fn stale_unregister_does_not_remove_a_replacement_registration() {
        let forwarder = RelayForwarderActor::default();
        let bob = AuthorityId::new_from_entropy([1u8; 32]);
        let (first_tx, _first_rx) = mpsc::channel(1);
        let first = forwarder.register(bob, "phone".to_string(), first_tx);
        let (second_tx, mut second_rx) = mpsc::channel(1);
        let second = forwarder.register(bob, "phone".to_string(), second_tx);
        assert_eq!(forwarder.registered_sessions(), 1);

        forwarder.unregister(bob, "phone", first);
        forwarder
            .forward(envelope(bob, None))
            .expect("replacement session still registered");
        assert!(second_rx.try_recv().is_ok());

        forwarder.unregister(bob, "phone", second);
        assert_eq!(forwarder.registered_sessions(), 0);
    }

    #[test]
    fn full_session_queue_counts_as_a_drop() {
        let forwarder = RelayForwarderActor::default();
        let bob = AuthorityId::new_from_entropy([1u8; 32]);
        let (tx, _rx) = mpsc::channel(1);
        forwarder.register(bob, "phone".to_string(), tx);
        forwarder.forward(envelope(bob, None)).expect("first fits");
        assert_eq!(
            forwarder.forward(envelope(bob, None)),
            Err(RelayForwardError::SessionUnavailable { destination: bob })
        );
    }

    #[tokio::test]
    async fn frames_round_trip_over_a_loopback_stream() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bob = AuthorityId::new_from_entropy([1u8; 32]);
        let device = DeviceId::new_from_entropy([3u8; 32]);
        let frame = control_frame(bob, bob, device, RELAY_OP_REGISTER);
        let writer = async {
            let mut stream = TcpStream::connect(addr).await.expect("connect");
            write_frame(&mut stream, &frame).await.expect("write");
        };
        let reader = async {
            let (mut stream, _) = listener.accept().await.expect("accept");
            read_frame(&mut stream).await.expect("read")
        };
        let (_, received) = tokio::join!(writer, reader);
        let received = received.expect("one frame");
        assert!(is_relay_control(&received));
        assert_eq!(relay_op(&received), Some(RELAY_OP_REGISTER));
        assert_eq!(received.source, bob);
        assert_eq!(
            received.metadata.get(DEVICE_METADATA_KEY),
            Some(&device.to_string())
        );
    }
}

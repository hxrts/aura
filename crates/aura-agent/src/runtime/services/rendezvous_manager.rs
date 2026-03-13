//! Rendezvous Service Manager
//!
//! Wraps `aura_rendezvous::RendezvousService` for integration with the agent runtime.
//! Provides lifecycle management, descriptor caching, and channel establishment.
//!
//! ## LAN Discovery
//!
//! Supports local network peer discovery via UDP broadcast. When enabled, the manager
//! will announce presence and discover peers on the local network.

use super::runtime_tasks::TaskGroup;
use super::traits::{RuntimeService, RuntimeServiceContext, ServiceError, ServiceHealth};
use async_trait::async_trait;
use aura_core::crypto::single_signer::SingleSignerKeyPackage;
#[cfg(target_arch = "wasm32")]
use aura_core::effects::network::NetworkError;
use aura_core::effects::network::{UdpEffects, UdpEndpoint, UdpEndpointEffects};
use aura_core::effects::secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{CryptoEffects, NoiseEffects};
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_core::AuraError;
use aura_rendezvous::{
    DiscoveredPeer, LanDiscoveryConfig, LocalInterfaces, RendezvousConfig, RendezvousDescriptor,
    RendezvousFact, RendezvousService, TransportHint,
};
use cfg_if::cfg_if;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};

use super::lan_discovery::{LanDiscoveryMetrics, LanDiscoveryService};
use super::service_actor::{validate_actor_transition, ActorLifecyclePhase, ServiceActorHandle};

/// Configuration for the rendezvous service manager
#[derive(Debug, Clone)]
pub struct RendezvousManagerConfig {
    /// Enable automatic descriptor refresh
    pub auto_refresh_enabled: bool,

    /// Refresh window - refresh descriptors this long before expiry (default: 5 min)
    pub refresh_window: Duration,

    /// Default descriptor validity duration (default: 1 hour)
    pub descriptor_validity: Duration,

    /// Enable periodic cleanup of expired descriptors
    pub auto_cleanup_enabled: bool,

    /// Cleanup interval (default: 60s)
    pub cleanup_interval: Duration,

    /// Default transport hints for this node
    pub default_transport_hints: Vec<TransportHint>,

    /// LAN discovery configuration
    pub lan_discovery: LanDiscoveryConfig,
}

impl Default for RendezvousManagerConfig {
    fn default() -> Self {
        Self {
            auto_refresh_enabled: true,
            refresh_window: Duration::from_secs(300), // 5 minutes
            descriptor_validity: Duration::from_secs(3600), // 1 hour
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(60),
            default_transport_hints: Vec::new(),
            lan_discovery: LanDiscoveryConfig::default(),
        }
    }
}

impl RendezvousManagerConfig {
    /// Create config for testing (shorter intervals)
    pub fn for_testing() -> Self {
        Self {
            auto_refresh_enabled: true,
            refresh_window: Duration::from_secs(30),
            descriptor_validity: Duration::from_secs(300),
            auto_cleanup_enabled: true,
            cleanup_interval: Duration::from_secs(10),
            default_transport_hints: vec![
                TransportHint::quic_direct("127.0.0.1:0").expect("valid loopback address")
            ],
            lan_discovery: LanDiscoveryConfig {
                enabled: false, // Disabled by default in tests
                ..Default::default()
            },
        }
    }

    /// Create config with auto features disabled
    pub fn manual_only() -> Self {
        Self {
            auto_refresh_enabled: false,
            auto_cleanup_enabled: false,
            ..Default::default()
        }
    }

    /// Set default transport hints
    pub fn with_transport_hints(mut self, hints: Vec<TransportHint>) -> Self {
        self.default_transport_hints = hints;
        self
    }

    /// Set LAN discovery configuration
    pub fn with_lan_discovery(mut self, config: LanDiscoveryConfig) -> Self {
        self.lan_discovery = config;
        self
    }

    /// Enable or disable LAN discovery
    pub fn lan_discovery_enabled(mut self, enabled: bool) -> Self {
        self.lan_discovery.enabled = enabled;
        self
    }
}

/// State of the rendezvous service manager
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendezvousManagerState {
    /// Service not yet started
    Stopped,
    /// Service starting up
    Starting,
    /// Service running
    Running,
    /// Service shutting down
    Stopping,
    /// Service hit an observable lifecycle failure.
    Failed,
}

#[derive(Debug, Error)]
pub enum RendezvousManagerError {
    #[error(transparent)]
    Service(#[from] ServiceError),
    #[error("rendezvous manager not started")]
    NotStarted,
    #[error("peer descriptor not found in cache for authority {peer}")]
    PeerDescriptorNotFound { peer: AuthorityId },
    #[error("rendezvous channel preparation failed")]
    ChannelPreparation(#[source] AuraError),
    #[error("rendezvous manager is not running under supervised task ownership")]
    MissingSupervisedTasks,
    #[error("invalid peer address")]
    InvalidPeerAddress(#[source] std::net::AddrParseError),
    #[error("failed to send LAN invitation")]
    LanInvitationSend(#[source] aura_effects::NetworkError),
    #[error("LAN discovery not running")]
    LanDiscoveryUnavailable,
}

struct RendezvousState {
    status: RendezvousManagerState,
    service: Option<Arc<RendezvousService>>,
    lan_discovery: Option<Arc<LanDiscoveryService>>,
    lan_discovered_peers: HashMap<AuthorityId, DiscoveredPeer>,
    descriptor_cache: HashMap<(ContextId, AuthorityId), RendezvousDescriptor>,
}

#[derive(Clone)]
struct RendezvousSnapshot {
    status: RendezvousManagerState,
    service: Option<Arc<RendezvousService>>,
    lan_discovery: Option<Arc<LanDiscoveryService>>,
}

enum RendezvousCommand {
    Snapshot {
        reply: oneshot::Sender<RendezvousSnapshot>,
    },
    CacheDescriptor {
        descriptor: RendezvousDescriptor,
        reply: oneshot::Sender<()>,
    },
    GetDescriptor {
        context_id: ContextId,
        peer: AuthorityId,
        reply: oneshot::Sender<Option<RendezvousDescriptor>>,
    },
    GetAnyDescriptorForAuthority {
        peer: AuthorityId,
        reply: oneshot::Sender<Option<RendezvousDescriptor>>,
    },
    NeedsRefresh {
        context_id: ContextId,
        owner: AuthorityId,
        refresh_window_ms: u64,
        now_ms: u64,
        reply: oneshot::Sender<bool>,
    },
    ContextsNeedingRefresh {
        owner: AuthorityId,
        refresh_window_ms: u64,
        now_ms: u64,
        reply: oneshot::Sender<Vec<ContextId>>,
    },
    ListCachedPeers {
        owner: AuthorityId,
        reply: oneshot::Sender<Vec<AuthorityId>>,
    },
    ListCachedPeerDevices {
        owner: AuthorityId,
        reply: oneshot::Sender<Vec<DeviceId>>,
    },
    ListCachedPeersForContext {
        context_id: ContextId,
        owner: AuthorityId,
        reply: oneshot::Sender<Vec<AuthorityId>>,
    },
    CacheDiscoveredPeer {
        local_authority_id: AuthorityId,
        peer: DiscoveredPeer,
        reply: oneshot::Sender<()>,
    },
    InstallLanDiscovery {
        service: Arc<LanDiscoveryService>,
        reply: oneshot::Sender<()>,
    },
    ClearLanDiscovery {
        reply: oneshot::Sender<()>,
    },
    PruneLanPeers {
        now_ms: u64,
        max_age_ms: u64,
        reply: oneshot::Sender<()>,
    },
    CleanupExpiredDescriptors {
        now_ms: u64,
        reply: oneshot::Sender<()>,
    },
    ListLanDiscoveredPeers {
        reply: oneshot::Sender<Vec<DiscoveredPeer>>,
    },
    ListLanDiscoveredPeerDevices {
        owner: AuthorityId,
        reply: oneshot::Sender<Vec<DeviceId>>,
    },
    ListReachablePeerDevices {
        owner: AuthorityId,
        reply: oneshot::Sender<Vec<DeviceId>>,
    },
    ListReachablePeerDevicesForAuthority {
        authority_id: AuthorityId,
        reply: oneshot::Sender<Vec<DeviceId>>,
    },
    GetLanDiscoveredPeer {
        authority_id: AuthorityId,
        reply: oneshot::Sender<Option<DiscoveredPeer>>,
    },
    IsLanDiscoveryRunning {
        reply: oneshot::Sender<bool>,
    },
}

impl RendezvousState {
    fn new_running(service: Arc<RendezvousService>) -> Self {
        Self {
            status: RendezvousManagerState::Running,
            service: Some(service),
            lan_discovery: None,
            lan_discovered_peers: HashMap::new(),
            descriptor_cache: HashMap::new(),
        }
    }
}

impl RendezvousManagerState {
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

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
struct WasmUnsupportedUdpEffects;

#[cfg(target_arch = "wasm32")]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl UdpEffects for WasmUnsupportedUdpEffects {
    async fn udp_bind(
        &self,
        _addr: UdpEndpoint,
    ) -> Result<Arc<dyn UdpEndpointEffects>, NetworkError> {
        Err(NetworkError::NotImplemented)
    }
}

fn default_udp_effects() -> Arc<dyn UdpEffects> {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            Arc::new(WasmUnsupportedUdpEffects)
        } else {
            Arc::new(aura_effects::RealUdpEffectsHandler::new())
        }
    }
}

/// Manager for rendezvous operations
///
/// Integrates `aura_rendezvous::RendezvousService` into the agent runtime lifecycle.
/// Handles descriptor publication, caching, and channel establishment.
///
/// ## LAN Discovery
///
/// When `config.lan_discovery.enabled` is true, the manager will:
/// - Broadcast presence on the local network periodically
/// - Listen for other Aura nodes on the LAN
/// - Cache discovered peer descriptors for connection
#[derive(Clone)]
pub struct RendezvousManager {
    /// Configuration
    config: RendezvousManagerConfig,

    /// Lifecycle state outside the actor for startup/shutdown/health fallback.
    lifecycle_state: Arc<Mutex<RendezvousManagerState>>,

    /// Authority ID
    authority_id: AuthorityId,

    /// Time effects (simulator-controllable)
    time: Arc<dyn PhysicalTimeEffects>,

    /// UDP effects for LAN discovery sockets
    udp: Arc<dyn UdpEffects>,

    /// Owned task group for service-local background work.
    tasks: Arc<Mutex<Option<TaskGroup>>>,

    /// Typed command ingress for rendezvous-owned mutable state.
    commands: Arc<Mutex<Option<ServiceActorHandle<RendezvousCommand>>>>,

    /// Serializes lifecycle transitions.
    lifecycle: Arc<Mutex<()>>,
}

impl RendezvousManager {
    const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

    /// Create a new rendezvous manager
    pub fn new(
        authority_id: AuthorityId,
        config: RendezvousManagerConfig,
        time: Arc<dyn PhysicalTimeEffects>,
        udp: Arc<dyn UdpEffects>,
    ) -> Self {
        Self {
            config,
            lifecycle_state: Arc::new(Mutex::new(RendezvousManagerState::Stopped)),
            authority_id,
            time,
            udp,
            tasks: Arc::new(Mutex::new(None)),
            commands: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(Mutex::new(())),
        }
    }

    /// Create a new rendezvous manager with the default UDP effect backend for this target.
    pub fn new_with_default_udp(
        authority_id: AuthorityId,
        config: RendezvousManagerConfig,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Self {
        Self::new(authority_id, config, time, default_udp_effects())
    }

    /// Create with default configuration
    pub fn with_defaults(authority_id: AuthorityId, time: Arc<dyn PhysicalTimeEffects>) -> Self {
        Self::new(
            authority_id,
            RendezvousManagerConfig::default(),
            time,
            default_udp_effects(),
        )
    }

    /// Get the current state
    pub async fn state(&self) -> RendezvousManagerState {
        match self.snapshot().await {
            Ok(snapshot) => snapshot.status,
            Err(_) => *self.lifecycle_state.lock().await,
        }
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        self.state().await == RendezvousManagerState::Running
    }

    async fn command_handle(&self) -> Result<ServiceActorHandle<RendezvousCommand>, ServiceError> {
        self.commands.lock().await.clone().ok_or_else(|| {
            ServiceError::unavailable(
                self.name(),
                "rendezvous command actor unavailable; service is not fully started",
            )
        })
    }

    fn spawn_command_actor(
        &self,
        tasks: &TaskGroup,
        mut state: RendezvousState,
    ) -> ServiceActorHandle<RendezvousCommand> {
        let (cmd_tx, mut cmd_rx) = mpsc::channel(64);

        tasks.spawn_named("command_actor", async move {
            while let Some(command) = cmd_rx.recv().await {
                match command {
                    RendezvousCommand::Snapshot { reply } => {
                        let _ = reply.send(RendezvousSnapshot {
                            status: state.status,
                            service: state.service.clone(),
                            lan_discovery: state.lan_discovery.clone(),
                        });
                    }
                    RendezvousCommand::CacheDescriptor { descriptor, reply } => {
                        let key = (descriptor.context_id, descriptor.authority_id);
                        let descriptor = if let Some(existing) = state.descriptor_cache.get(&key) {
                            let mut merged = descriptor.clone();
                            if merged.device_id.is_none() {
                                merged.device_id = existing.device_id;
                            }
                            merged
                        } else {
                            descriptor
                        };
                        state.descriptor_cache.insert(key, descriptor);
                        let _ = reply.send(());
                    }
                    RendezvousCommand::GetDescriptor {
                        context_id,
                        peer,
                        reply,
                    } => {
                        let value = state.descriptor_cache.get(&(context_id, peer)).cloned();
                        let _ = reply.send(value);
                    }
                    RendezvousCommand::GetAnyDescriptorForAuthority { peer, reply } => {
                        let value = state
                            .descriptor_cache
                            .values()
                            .find(|descriptor| descriptor.authority_id == peer)
                            .cloned()
                            .or_else(|| {
                                state
                                    .lan_discovered_peers
                                    .get(&peer)
                                    .map(|discovered| discovered.descriptor.clone())
                            });
                        let _ = reply.send(value);
                    }
                    RendezvousCommand::NeedsRefresh {
                        context_id,
                        owner,
                        refresh_window_ms,
                        now_ms,
                        reply,
                    } => {
                        let value = state
                            .descriptor_cache
                            .get(&(context_id, owner))
                            .map(|desc| {
                                let refresh_threshold =
                                    desc.valid_until.saturating_sub(refresh_window_ms);
                                now_ms >= refresh_threshold
                            })
                            .unwrap_or(true);
                        let _ = reply.send(value);
                    }
                    RendezvousCommand::ContextsNeedingRefresh {
                        owner,
                        refresh_window_ms,
                        now_ms,
                        reply,
                    } => {
                        let contexts = state
                            .descriptor_cache
                            .iter()
                            .filter(|((_, auth), desc)| {
                                *auth == owner && {
                                    let refresh_threshold =
                                        desc.valid_until.saturating_sub(refresh_window_ms);
                                    now_ms >= refresh_threshold
                                }
                            })
                            .map(|((ctx, _), _)| *ctx)
                            .collect();
                        let _ = reply.send(contexts);
                    }
                    RendezvousCommand::ListCachedPeers { owner, reply } => {
                        let peers = state
                            .descriptor_cache
                            .keys()
                            .filter(|(_, auth)| *auth != owner)
                            .map(|(_, auth)| *auth)
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        let _ = reply.send(peers);
                    }
                    RendezvousCommand::ListCachedPeerDevices { owner, reply } => {
                        let devices = state
                            .descriptor_cache
                            .values()
                            .filter_map(|descriptor| {
                                (descriptor.authority_id != owner)
                                    .then_some(descriptor.device_id)
                                    .flatten()
                            })
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        let _ = reply.send(devices);
                    }
                    RendezvousCommand::ListCachedPeersForContext {
                        context_id,
                        owner,
                        reply,
                    } => {
                        let peers = state
                            .descriptor_cache
                            .keys()
                            .filter(|(ctx, auth)| *ctx == context_id && *auth != owner)
                            .map(|(_, auth)| *auth)
                            .collect();
                        let _ = reply.send(peers);
                    }
                    RendezvousCommand::CacheDiscoveredPeer {
                        local_authority_id,
                        peer,
                        reply,
                    } => {
                        state
                            .lan_discovered_peers
                            .insert(peer.authority_id, peer.clone());
                        state.descriptor_cache.insert(
                            (peer.descriptor.context_id, peer.descriptor.authority_id),
                            peer.descriptor.clone(),
                        );
                        let local_context =
                            aura_core::context::EffectContext::with_authority(local_authority_id)
                                .context_id();
                        state.descriptor_cache.insert(
                            (local_context, peer.descriptor.authority_id),
                            peer.descriptor.clone(),
                        );
                        let _ = reply.send(());
                    }
                    RendezvousCommand::InstallLanDiscovery { service, reply } => {
                        state.lan_discovery = Some(service);
                        let _ = reply.send(());
                    }
                    RendezvousCommand::ClearLanDiscovery { reply } => {
                        state.lan_discovery = None;
                        let _ = reply.send(());
                    }
                    RendezvousCommand::PruneLanPeers {
                        now_ms,
                        max_age_ms,
                        reply,
                    } => {
                        state.lan_discovered_peers.retain(|_, peer| {
                            now_ms.saturating_sub(peer.discovered_at_ms) < max_age_ms
                        });
                        let _ = reply.send(());
                    }
                    RendezvousCommand::CleanupExpiredDescriptors { now_ms, reply } => {
                        state
                            .descriptor_cache
                            .retain(|_, descriptor| descriptor.is_valid(now_ms));
                        let _ = reply.send(());
                    }
                    RendezvousCommand::ListLanDiscoveredPeers { reply } => {
                        let peers = state.lan_discovered_peers.values().cloned().collect();
                        let _ = reply.send(peers);
                    }
                    RendezvousCommand::ListLanDiscoveredPeerDevices { owner, reply } => {
                        let devices = state
                            .lan_discovered_peers
                            .values()
                            .filter_map(|peer| {
                                (peer.authority_id != owner)
                                    .then_some(peer.descriptor.device_id)
                                    .flatten()
                            })
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect();
                        let _ = reply.send(devices);
                    }
                    RendezvousCommand::ListReachablePeerDevices { owner, reply } => {
                        let mut devices = state
                            .descriptor_cache
                            .values()
                            .filter_map(|descriptor| {
                                (descriptor.authority_id != owner)
                                    .then_some(descriptor.device_id)
                                    .flatten()
                            })
                            .chain(state.lan_discovered_peers.values().filter_map(|peer| {
                                (peer.authority_id != owner)
                                    .then_some(peer.descriptor.device_id)
                                    .flatten()
                            }))
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect::<Vec<_>>();
                        devices.sort();
                        let _ = reply.send(devices);
                    }
                    RendezvousCommand::ListReachablePeerDevicesForAuthority {
                        authority_id,
                        reply,
                    } => {
                        let mut devices = state
                            .descriptor_cache
                            .values()
                            .filter_map(|descriptor| {
                                (descriptor.authority_id == authority_id)
                                    .then_some(descriptor.device_id)
                                    .flatten()
                            })
                            .chain(state.lan_discovered_peers.values().filter_map(|peer| {
                                (peer.authority_id == authority_id)
                                    .then_some(peer.descriptor.device_id)
                                    .flatten()
                            }))
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect::<Vec<_>>();
                        devices.sort();
                        let _ = reply.send(devices);
                    }
                    RendezvousCommand::GetLanDiscoveredPeer {
                        authority_id,
                        reply,
                    } => {
                        let peer = state.lan_discovered_peers.get(&authority_id).cloned();
                        let _ = reply.send(peer);
                    }
                    RendezvousCommand::IsLanDiscoveryRunning { reply } => {
                        let running = state.lan_discovery.is_some();
                        let _ = reply.send(running);
                    }
                }
            }
        });

        ServiceActorHandle::new(self.name(), cmd_tx)
    }

    async fn start_managed(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        let current_state = *self.lifecycle_state.lock().await;
        if current_state == RendezvousManagerState::Running {
            return Ok(());
        }
        validate_actor_transition(
            self.name(),
            current_state.phase(),
            ActorLifecyclePhase::Starting,
        )?;

        *self.lifecycle_state.lock().await = RendezvousManagerState::Starting;

        let rendezvous_config = RendezvousConfig {
            descriptor_validity_ms: self.config.descriptor_validity.as_millis() as u64,
            probe_timeout_ms: 5000,
            stun_server: None,
            max_relay_hops: 3,
        };

        let service = Arc::new(RendezvousService::new(self.authority_id, rendezvous_config));
        let service_tasks = context.tasks().group(self.name());
        let command_handle =
            self.spawn_command_actor(&service_tasks, RendezvousState::new_running(service));
        *self.commands.lock().await = Some(command_handle);
        *self.lifecycle_state.lock().await = RendezvousManagerState::Running;

        if self.config.auto_cleanup_enabled {
            self.start_cleanup_task(service_tasks.clone());
        }

        #[cfg(not(target_arch = "wasm32"))]
        if self.config.lan_discovery.enabled {
            if let Err(error) = self.start_lan_discovery(service_tasks.clone()).await {
                self.commands.lock().await.take();
                *self.lifecycle_state.lock().await = RendezvousManagerState::Failed;
                let _ = service_tasks
                    .shutdown_with_timeout(Self::SHUTDOWN_TIMEOUT)
                    .await;
                return Err(error);
            }
        }

        *self.tasks.lock().await = Some(service_tasks);

        tracing::info!(
            event = "runtime.service.rendezvous.started",
            service = self.name(),
            "Rendezvous manager started for authority {}",
            self.authority_id
        );
        Ok(())
    }

    async fn stop_managed(&self) -> Result<(), ServiceError> {
        let _lifecycle_guard = self.lifecycle.lock().await;
        let current_state = *self.lifecycle_state.lock().await;
        if current_state == RendezvousManagerState::Stopped {
            return Ok(());
        }
        validate_actor_transition(
            self.name(),
            current_state.phase(),
            ActorLifecyclePhase::Stopping,
        )?;

        *self.lifecycle_state.lock().await = RendezvousManagerState::Stopping;

        self.stop_lan_discovery().await;
        self.commands.lock().await.take();

        let task_shutdown_error = if let Some(tasks) = self.tasks.lock().await.take() {
            tasks
                .shutdown_with_timeout(Self::SHUTDOWN_TIMEOUT)
                .await
                .err()
                .map(|error| {
                    ServiceError::shutdown_failed(
                        self.name(),
                        format!("failed to quiesce rendezvous tasks: {error}"),
                    )
                })
        } else {
            None
        };

        *self.lifecycle_state.lock().await = RendezvousManagerState::Stopped;

        tracing::info!(
            event = "runtime.service.rendezvous.stopped",
            service = self.name(),
            "Rendezvous manager stopped"
        );
        match task_shutdown_error {
            Some(error) => {
                *self.lifecycle_state.lock().await = RendezvousManagerState::Failed;
                Err(error)
            }
            None => Ok(()),
        }
    }

    fn start_cleanup_task(&self, tasks: TaskGroup) {
        let interval = self.config.cleanup_interval;
        let time = self.time.clone();
        let commands = self.commands.clone();

        tasks.spawn_periodic("descriptor_cleanup", time.clone(), interval, move || {
            let time = time.clone();
            let commands = commands.clone();
            async move {
                let now_ms = match time.physical_time().await {
                    Ok(t) => t.ts_ms,
                    Err(error) => {
                        tracing::debug!(
                            event = "runtime.service.rendezvous.cleanup.skipped",
                            reason = %error,
                            "Skipping rendezvous descriptor cleanup"
                        );
                        return true;
                    }
                };

                if let Some(commands) = commands.lock().await.clone() {
                    let _ = commands
                        .request(|reply| RendezvousCommand::CleanupExpiredDescriptors {
                            now_ms,
                            reply,
                        })
                        .await;
                }
                true
            }
        });
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

    async fn snapshot(&self) -> Result<RendezvousSnapshot, ServiceError> {
        self.command_handle()
            .await?
            .request(|reply| RendezvousCommand::Snapshot { reply })
            .await
    }

    /// Publish a transport descriptor for a context
    ///
    /// Returns the guard outcome with the descriptor fact.
    pub async fn publish_descriptor<E: SecureStorageEffects>(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
        effects: &E,
    ) -> Result<aura_rendezvous::GuardOutcome, RendezvousManagerError> {
        let service = self
            .snapshot()
            .await?
            .service
            .ok_or(RendezvousManagerError::NotStarted)?;

        let hints = Self::relay_first_order(
            transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone()),
        );

        // Retrieve identity keys to get public key
        let keys = retrieve_identity_keys(effects, &self.authority_id).await;
        let public_key = keys.map(|(_, pub_key)| pub_key).unwrap_or([0u8; 32]);

        Ok(service.prepare_publish_descriptor(snapshot, context_id, hints, public_key, now_ms))
    }

    /// Refresh a descriptor for a context
    ///
    /// Returns the guard outcome with the new descriptor fact.
    pub async fn refresh_descriptor<E: SecureStorageEffects>(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
        effects: &E,
    ) -> Result<aura_rendezvous::GuardOutcome, RendezvousManagerError> {
        let service = self
            .snapshot()
            .await?
            .service
            .ok_or(RendezvousManagerError::NotStarted)?;

        let hints = Self::relay_first_order(
            transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone()),
        );

        // Retrieve identity keys to get public key
        let keys = retrieve_identity_keys(effects, &self.authority_id).await;
        let public_key = keys.map(|(_, pub_key)| pub_key).unwrap_or([0u8; 32]);

        Ok(service.prepare_refresh_descriptor(snapshot, context_id, hints, public_key, now_ms))
    }

    /// Cache a peer's descriptor
    pub async fn cache_descriptor(
        &self,
        descriptor: RendezvousDescriptor,
    ) -> Result<(), RendezvousManagerError> {
        self.command_handle()
            .await?
            .request(|reply| RendezvousCommand::CacheDescriptor { descriptor, reply })
            .await
            .map_err(RendezvousManagerError::from)
    }

    /// Get a cached descriptor for a peer
    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.command_handle()
            .await
            .ok()?
            .request(|reply| RendezvousCommand::GetDescriptor {
                context_id,
                peer,
                reply,
            })
            .await
            .ok()?
    }

    /// Get any cached descriptor for a peer authority regardless of context.
    ///
    /// Transport address resolution is authority-scoped even when higher-level
    /// protocols are operating inside a more specific relational context. When a
    /// context-specific descriptor has not been cached yet, this provides a
    /// bounded authority-level fallback instead of treating the peer as absent.
    pub async fn get_any_descriptor_for_authority(
        &self,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.command_handle()
            .await
            .ok()?
            .request(|reply| RendezvousCommand::GetAnyDescriptorForAuthority { peer, reply })
            .await
            .ok()?
    }

    /// Check if our descriptor needs refresh in a context
    pub async fn needs_refresh(&self, context_id: ContextId, now_ms: u64) -> bool {
        let refresh_window_ms = self.config.refresh_window.as_millis() as u64;
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::NeedsRefresh {
                    context_id,
                    owner: self.authority_id,
                    refresh_window_ms,
                    now_ms,
                    reply,
                })
                .await
                .unwrap_or(true),
            Err(_) => true,
        }
    }

    /// Get contexts needing descriptor refresh
    pub async fn contexts_needing_refresh(&self, now_ms: u64) -> Vec<ContextId> {
        let refresh_window_ms = self.config.refresh_window.as_millis() as u64;
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ContextsNeedingRefresh {
                    owner: self.authority_id,
                    refresh_window_ms,
                    now_ms,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Prepare to establish a channel with a peer
    pub async fn prepare_establish_channel<
        E: NoiseEffects + CryptoEffects + SecureStorageEffects,
    >(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        psk: &[u8; 32],
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
        effects: &E,
    ) -> Result<aura_rendezvous::GuardOutcome, RendezvousManagerError> {
        let service = self
            .snapshot()
            .await?
            .service
            .ok_or(RendezvousManagerError::NotStarted)?;
        let descriptor = self.get_descriptor(context_id, peer).await;
        let descriptor = match descriptor {
            Some(value) => value,
            None => self
                .get_any_descriptor_for_authority(peer)
                .await
                .ok_or(RendezvousManagerError::PeerDescriptorNotFound { peer })?,
        };
        let establish_descriptor = Self::relay_first_initial_descriptor(&descriptor);

        // Retrieve identity keys
        let keys = retrieve_identity_keys(effects, &self.authority_id).await;
        let (local_private_key, _) = keys.unwrap_or(([0u8; 32], [0u8; 32]));

        let remote_public_key = descriptor.public_key;

        service
            .prepare_establish_channel(
                snapshot,
                context_id,
                peer,
                psk,
                &local_private_key,
                &remote_public_key,
                now_ms,
                &establish_descriptor,
                effects,
            )
            .await
            .map_err(RendezvousManagerError::ChannelPreparation)
    }

    /// Create a channel established fact
    pub async fn create_channel_fact(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> Result<RendezvousFact, RendezvousManagerError> {
        let service = self
            .snapshot()
            .await?
            .service
            .ok_or(RendezvousManagerError::NotStarted)?;

        Ok(service.create_channel_established_fact(context_id, peer, channel_id, epoch))
    }

    // ========================================================================
    // Relay Operations
    // ========================================================================

    /// Prepare a relay request
    pub async fn prepare_relay_request(
        &self,
        context_id: ContextId,
        relay: AuthorityId,
        target: AuthorityId,
        snapshot: &aura_rendezvous::GuardSnapshot,
    ) -> Result<aura_rendezvous::GuardOutcome, RendezvousManagerError> {
        let service = self
            .snapshot()
            .await?
            .service
            .ok_or(RendezvousManagerError::NotStarted)?;

        Ok(service.prepare_relay_request(context_id, relay, target, snapshot))
    }

    // ========================================================================
    // Peer Discovery
    // ========================================================================

    /// List all cached peer authorities (excluding self)
    ///
    /// Returns unique AuthorityIds for all peers with cached descriptors.
    /// Useful for peer discovery integration with sync.
    pub async fn list_cached_peers(&self) -> Vec<AuthorityId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListCachedPeers {
                    owner: self.authority_id,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// List cached peer devices with explicit device identity.
    pub async fn list_cached_peer_devices(&self) -> Vec<DeviceId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListCachedPeerDevices {
                    owner: self.authority_id,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// List all cached peers for a specific context (excluding self)
    pub async fn list_cached_peers_for_context(&self, context_id: ContextId) -> Vec<AuthorityId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListCachedPeersForContext {
                    context_id,
                    owner: self.authority_id,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// Return recoverable direct candidates for background direct-upgrade attempts.
    ///
    /// This intentionally excludes relay hints because relay is handled as the
    /// initial path selection strategy.
    pub async fn direct_upgrade_candidates(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        interfaces: &LocalInterfaces,
    ) -> Vec<TransportHint> {
        let descriptor = self.get_descriptor(context_id, peer).await;
        let Some(descriptor) = descriptor else {
            return Vec::new();
        };

        descriptor
            .transport_hints
            .into_iter()
            .filter(|hint| {
                !matches!(hint, TransportHint::WebSocketRelay { .. })
                    && hint.is_recoverable(interfaces)
            })
            .collect()
    }

    // ========================================================================
    // LAN Discovery Operations
    // ========================================================================

    /// Start LAN discovery
    ///
    /// Creates a LAN discovery service and starts the announcer and listener.
    /// Discovered peers are cached and their descriptors are stored.
    async fn start_lan_discovery(&self, tasks: TaskGroup) -> Result<(), ServiceError> {
        if !self.config.lan_discovery.enabled {
            return Ok(());
        }

        if self.is_lan_discovery_running().await {
            return Ok(());
        }

        let time: Arc<dyn PhysicalTimeEffects> = self.time.clone();
        let lan_service = LanDiscoveryService::new(
            self.config.lan_discovery.clone(),
            self.authority_id,
            self.udp.clone(),
            time,
        )
        .await
        .map_err(|error| {
            ServiceError::startup_failed(
                self.name(),
                format!("failed to create LAN discovery service: {error}"),
            )
        })?;

        let local_authority_id = self.authority_id;
        let lan_cache_tasks = tasks.group("lan_peer_cache");
        let commands = self.command_handle().await?;

        let lan_tasks = tasks.group("lan_discovery");
        lan_service.start(lan_tasks, move |peer: DiscoveredPeer| {
            let peer_clone = peer.clone();
            let lan_cache_tasks = lan_cache_tasks.clone();
            let commands = commands.clone();

            lan_cache_tasks.spawn_named("cache_discovered_peer", async move {
                let _ = commands
                    .request(|reply| RendezvousCommand::CacheDiscoveredPeer {
                        local_authority_id,
                        peer: peer_clone.clone(),
                        reply,
                    })
                    .await;

                tracing::info!(
                    event = "runtime.service.rendezvous.lan_peer_cached",
                    authority = %peer.authority_id,
                    addr = %peer.source_addr,
                    "Cached LAN-discovered peer"
                );
            });
        });

        let lan_service = Arc::new(lan_service);
        self.command_handle()
            .await?
            .request(|reply| RendezvousCommand::InstallLanDiscovery {
                service: lan_service,
                reply,
            })
            .await?;

        tracing::info!(
            event = "runtime.service.rendezvous.lan_started",
            component = "rendezvous_manager",
            port = self.config.lan_discovery.port,
            "LAN discovery started"
        );
        Ok(())
    }

    /// Stop LAN discovery
    async fn stop_lan_discovery(&self) {
        if let Ok(commands) = self.command_handle().await {
            let _ = commands
                .request(|reply| RendezvousCommand::ClearLanDiscovery { reply })
                .await;
        }
        tracing::info!(
            event = "runtime.service.rendezvous.lan_stopped",
            component = "rendezvous_manager",
            port = self.config.lan_discovery.port,
            "LAN discovery stopped"
        );
    }

    /// Set the descriptor to announce on LAN
    ///
    /// Should be called after publishing a descriptor to start announcing on LAN.
    pub async fn set_lan_descriptor(&self, descriptor: RendezvousDescriptor) {
        let service = self
            .snapshot()
            .await
            .ok()
            .and_then(|snapshot| snapshot.lan_discovery);
        if let Some(service) = service.as_ref() {
            service.set_descriptor(descriptor).await;
        }
    }

    /// Get LAN discovery metrics, if LAN discovery is enabled and running.
    pub async fn lan_metrics(&self) -> Option<LanDiscoveryMetrics> {
        let service = self
            .snapshot()
            .await
            .ok()
            .and_then(|snapshot| snapshot.lan_discovery);
        match service {
            Some(service) => Some(service.metrics().await),
            None => None,
        }
    }

    /// Get LAN-discovered peers
    ///
    /// Returns a copy of all peers discovered via LAN broadcast.
    pub async fn list_lan_discovered_peers(&self) -> Vec<DiscoveredPeer> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListLanDiscoveredPeers { reply })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// List LAN-discovered peer devices with explicit device identity.
    pub async fn list_lan_discovered_peer_devices(&self) -> Vec<DeviceId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListLanDiscoveredPeerDevices {
                    owner: self.authority_id,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// List all reachable peer devices known to rendezvous.
    ///
    /// This is the authoritative device-level view for transport/sync wiring.
    /// Higher-level social semantics remain authority-based, but endpoint
    /// selection must operate only on explicit device identities.
    pub async fn list_reachable_peer_devices(&self) -> Vec<DeviceId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::ListReachablePeerDevices {
                    owner: self.authority_id,
                    reply,
                })
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// List reachable device ids for a specific peer authority.
    ///
    /// Social semantics remain authority-based, but transport routing must target
    /// explicit devices. This returns the currently known reachable device set for
    /// that authority from descriptor and LAN-discovery state.
    pub async fn list_reachable_peer_devices_for_authority(
        &self,
        authority_id: AuthorityId,
    ) -> Vec<DeviceId> {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(
                    |reply| RendezvousCommand::ListReachablePeerDevicesForAuthority {
                        authority_id,
                        reply,
                    },
                )
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    /// Get a specific LAN-discovered peer
    pub async fn get_lan_discovered_peer(
        &self,
        authority_id: AuthorityId,
    ) -> Option<DiscoveredPeer> {
        self.command_handle()
            .await
            .ok()?
            .request(|reply| RendezvousCommand::GetLanDiscoveredPeer {
                authority_id,
                reply,
            })
            .await
            .ok()?
    }

    /// Check if LAN discovery is enabled and running
    pub async fn is_lan_discovery_running(&self) -> bool {
        match self.command_handle().await {
            Ok(commands) => commands
                .request(|reply| RendezvousCommand::IsLanDiscoveryRunning { reply })
                .await
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Clear expired LAN-discovered peers
    ///
    /// Removes peers that haven't been seen for longer than the specified duration.
    pub async fn prune_lan_peers(&self, max_age_ms: u64) {
        let now_ms = match self.time.physical_time().await {
            Ok(t) => t.ts_ms,
            Err(_) => return,
        };

        if let Ok(commands) = self.command_handle().await {
            let _ = commands
                .request(|reply| RendezvousCommand::PruneLanPeers {
                    now_ms,
                    max_age_ms,
                    reply,
                })
                .await;
        }
    }

    /// Trigger an on-demand discovery refresh
    ///
    /// For LAN discovery, this starts the LAN discovery service if not already running.
    pub async fn trigger_discovery(&self) -> Result<(), RendezvousManagerError> {
        if self.config.lan_discovery.enabled && !self.is_lan_discovery_running().await {
            let tasks = self
                .tasks
                .lock()
                .await
                .clone()
                .ok_or(RendezvousManagerError::MissingSupervisedTasks)?;
            self.start_lan_discovery(tasks).await?;
        }

        Ok(())
    }

    /// Send an invitation to a LAN peer
    ///
    /// This method uses the LAN transport to send an invite code directly
    /// to a peer discovered on the local network.
    pub async fn send_lan_invitation(
        &self,
        _peer_authority: &AuthorityId,
        peer_address: &str,
        invitation_code: &str,
    ) -> Result<(), RendezvousManagerError> {
        // Parse the peer address
        let addr = peer_address
            .parse::<std::net::SocketAddr>()
            .map_err(RendezvousManagerError::InvalidPeerAddress)?;
        let addr = UdpEndpoint::new(addr.to_string());

        // Get the LAN discovery service for sending
        let lan = self
            .snapshot()
            .await
            .ok()
            .and_then(|snapshot| snapshot.lan_discovery);
        if let Some(lan) = lan.as_ref() {
            // Use the LAN socket to send the invite code
            // The invitation is sent as a simple UDP packet
            let message = format!("AURA_INV:{}", invitation_code);
            let socket = lan.socket();
            socket
                .send_to(message.as_bytes(), &addr)
                .await
                .map_err(RendezvousManagerError::LanInvitationSend)?;

            tracing::info!(
                address = %addr,
                "Sent LAN invitation"
            );
            Ok(())
        } else {
            Err(RendezvousManagerError::LanDiscoveryUnavailable)
        }
    }

    // ========================================================================
    // Accessors
    // ========================================================================

    /// Get the configuration
    pub fn config(&self) -> &RendezvousManagerConfig {
        &self.config
    }

    fn relay_first_initial_descriptor(descriptor: &RendezvousDescriptor) -> RendezvousDescriptor {
        let relay_hints: Vec<_> = descriptor
            .transport_hints
            .iter()
            .filter(|hint| matches!(hint, TransportHint::WebSocketRelay { .. }))
            .cloned()
            .collect();
        if relay_hints.is_empty() {
            return descriptor.clone();
        }

        let mut relay_first = descriptor.clone();
        relay_first.transport_hints = relay_hints;
        relay_first
    }

    fn relay_first_order(mut hints: Vec<TransportHint>) -> Vec<TransportHint> {
        hints.sort_by_key(|hint| match hint {
            TransportHint::WebSocketRelay { .. } => 0u8,
            _ => 1u8,
        });
        hints
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl RuntimeService for RendezvousManager {
    fn name(&self) -> &'static str {
        "rendezvous_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["transport"]
    }

    async fn start(&self, context: &RuntimeServiceContext) -> Result<(), ServiceError> {
        self.start_managed(context).await
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop_managed().await
    }

    async fn health(&self) -> ServiceHealth {
        self.health_async().await
    }
}

impl RendezvousManager {
    /// Get the service health status asynchronously
    pub async fn health_async(&self) -> ServiceHealth {
        let (status, has_service, has_lan_discovery) = match self.snapshot().await {
            Ok(snapshot) => (
                snapshot.status,
                snapshot.service.is_some(),
                snapshot.lan_discovery.is_some(),
            ),
            Err(_) => (*self.lifecycle_state.lock().await, false, false),
        };

        match status {
            RendezvousManagerState::Stopped => ServiceHealth::Stopped,
            RendezvousManagerState::Starting => ServiceHealth::Starting,
            RendezvousManagerState::Stopping => ServiceHealth::Stopping,
            RendezvousManagerState::Failed => ServiceHealth::Unhealthy {
                reason: "service entered failed lifecycle state".to_string(),
            },
            RendezvousManagerState::Running => {
                if !has_service {
                    return ServiceHealth::Unhealthy {
                        reason: "underlying service not available".to_string(),
                    };
                }
                if self.tasks.lock().await.is_none() {
                    return ServiceHealth::Unhealthy {
                        reason: "service task group missing".to_string(),
                    };
                }
                if self.commands.lock().await.is_none() {
                    return ServiceHealth::Unhealthy {
                        reason: "service command actor missing".to_string(),
                    };
                }
                if self.config.lan_discovery.enabled {
                    if !has_lan_discovery {
                        return ServiceHealth::Unhealthy {
                            reason: "lan discovery enabled but service missing".to_string(),
                        };
                    }
                }
                ServiceHealth::Healthy
            }
        }
    }
}

async fn retrieve_identity_keys<E: SecureStorageEffects>(
    effects: &E,
    authority: &AuthorityId,
) -> Option<([u8; 32], [u8; 32])> {
    // Try to retrieve key from epoch 1 (bootstrap epoch)
    let location = SecureStorageLocation::new("signing_keys", format!("{}/1/1", authority));
    let caps = vec![SecureStorageCapability::Read];

    match effects.secure_retrieve(&location, &caps).await {
        Ok(bytes) => {
            if let Ok(pkg) = SingleSignerKeyPackage::from_bytes(&bytes) {
                let signing_key = pkg.signing_key().try_into().unwrap_or([0u8; 32]);
                let verifying_key = pkg.verifying_key().try_into().unwrap_or([0u8; 32]);
                Some((signing_key, verifying_key))
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::services::RuntimeTaskRegistry;
    use async_trait::async_trait;
    use aura_core::effects::noise::{
        HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState,
    };
    use aura_core::effects::{
        CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
        SecureStorageError,
    };
    use aura_core::time::PhysicalTime;
    use aura_core::FlowCost;
    use aura_effects::time::PhysicalTimeHandler;
    use aura_guards::types::CapabilityId;
    use aura_rendezvous::GuardSnapshot;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_peer() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([3u8; 32])
    }

    fn test_snapshot(authority: AuthorityId, context: ContextId) -> GuardSnapshot {
        GuardSnapshot {
            authority_id: authority,
            context_id: context,
            flow_budget_remaining: FlowCost::new(1000),
            capabilities: vec![
                CapabilityId::from("rendezvous:publish"),
                CapabilityId::from("rendezvous:connect"),
            ],
            epoch: 1,
        }
    }

    fn test_time() -> Arc<dyn PhysicalTimeEffects> {
        Arc::new(PhysicalTimeHandler::new())
    }

    fn test_udp() -> Arc<dyn UdpEffects> {
        default_udp_effects()
    }

    fn test_service_context() -> RuntimeServiceContext {
        RuntimeServiceContext::new(Arc::new(RuntimeTaskRegistry::new()), test_time())
    }

    // Mock for tests
    struct MockEffects;
    #[async_trait]
    impl SecureStorageEffects for MockEffects {
        async fn secure_store(
            &self,
            _: &SecureStorageLocation,
            _: &[u8],
            _: &[SecureStorageCapability],
        ) -> Result<(), SecureStorageError> {
            Ok(())
        }
        async fn secure_retrieve(
            &self,
            _: &SecureStorageLocation,
            _: &[SecureStorageCapability],
        ) -> Result<Vec<u8>, SecureStorageError> {
            Ok(vec![])
        }
        async fn secure_delete(
            &self,
            _: &SecureStorageLocation,
            _: &[SecureStorageCapability],
        ) -> Result<(), SecureStorageError> {
            Ok(())
        }
        async fn secure_exists(
            &self,
            _: &SecureStorageLocation,
        ) -> Result<bool, SecureStorageError> {
            Ok(false)
        }
        async fn secure_list_keys(
            &self,
            _: &str,
            _: &[SecureStorageCapability],
        ) -> Result<Vec<String>, SecureStorageError> {
            Ok(vec![])
        }
        async fn secure_generate_key(
            &self,
            _: &SecureStorageLocation,
            _: &str,
            _: &[SecureStorageCapability],
        ) -> Result<Option<Vec<u8>>, SecureStorageError> {
            Ok(None)
        }
        async fn secure_create_time_bound_token(
            &self,
            _: &SecureStorageLocation,
            _: &[SecureStorageCapability],
            _: &PhysicalTime,
        ) -> Result<Vec<u8>, SecureStorageError> {
            Ok(vec![])
        }
        async fn secure_access_with_token(
            &self,
            _: &[u8],
            _: &SecureStorageLocation,
        ) -> Result<Vec<u8>, SecureStorageError> {
            Ok(vec![])
        }
        async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError> {
            Ok(vec![])
        }
        async fn is_secure_storage_available(&self) -> bool {
            false
        }
        fn get_secure_storage_capabilities(&self) -> Vec<String> {
            vec![]
        }
    }
    #[async_trait]
    impl NoiseEffects for MockEffects {
        async fn create_handshake_state(
            &self,
            _: NoiseParams,
        ) -> Result<HandshakeState, NoiseError> {
            Ok(HandshakeState(Box::new(())))
        }
        async fn write_message(
            &self,
            _: HandshakeState,
            _: &[u8],
        ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![], HandshakeState(Box::new(()))))
        }
        async fn read_message(
            &self,
            _: HandshakeState,
            _: &[u8],
        ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![], HandshakeState(Box::new(()))))
        }
        async fn into_transport_mode(
            &self,
            _: HandshakeState,
        ) -> Result<TransportState, NoiseError> {
            Ok(TransportState(Box::new(())))
        }
        async fn encrypt_transport_message(
            &self,
            _: &mut TransportState,
            _: &[u8],
        ) -> Result<Vec<u8>, NoiseError> {
            Ok(vec![])
        }
        async fn decrypt_transport_message(
            &self,
            _: &mut TransportState,
            _: &[u8],
        ) -> Result<Vec<u8>, NoiseError> {
            Ok(vec![])
        }
    }
    // Minimal implementations for other traits needed by E
    #[async_trait]
    impl RandomCoreEffects for MockEffects {
        async fn random_bytes(&self, _: usize) -> Vec<u8> {
            vec![]
        }
        async fn random_bytes_32(&self) -> [u8; 32] {
            [0u8; 32]
        }
        async fn random_u64(&self) -> u64 {
            0
        }
    }
    #[async_trait]
    impl CryptoCoreEffects for MockEffects {
        async fn hkdf_derive(
            &self,
            _: &[u8],
            _: &[u8],
            _: &[u8],
            _: u32,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn derive_key(
            &self,
            _: &[u8],
            _: &aura_core::effects::crypto::KeyDerivationContext,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            Ok((vec![], vec![]))
        }
        async fn ed25519_sign(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn ed25519_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> {
            Ok(true)
        }
        fn is_simulated(&self) -> bool {
            false
        }
        fn crypto_capabilities(&self) -> Vec<String> {
            vec![]
        }
        fn constant_time_eq(&self, _: &[u8], _: &[u8]) -> bool {
            true
        }
        fn secure_zero(&self, _: &mut [u8]) {}
    }
    #[async_trait]
    impl CryptoExtendedEffects for MockEffects {
        async fn convert_ed25519_to_x25519_public(
            &self,
            _: &[u8],
        ) -> Result<[u8; 32], CryptoError> {
            Ok([0u8; 32])
        }
        async fn convert_ed25519_to_x25519_private(
            &self,
            _: &[u8],
        ) -> Result<[u8; 32], CryptoError> {
            Ok([0u8; 32])
        }
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());

        assert_eq!(manager.state().await, RendezvousManagerState::Stopped);
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();

        RuntimeService::start(&manager, &context).await.unwrap();
        assert!(manager.is_running().await);

        RuntimeService::stop(&manager).await.unwrap();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_manager_concurrent_lifecycle_transitions_are_idempotent() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();

        let start_a = RuntimeService::start(&manager, &context);
        let start_b = RuntimeService::start(&manager, &context);
        let (start_a, start_b) = tokio::join!(start_a, start_b);
        start_a.expect("first concurrent start should succeed");
        start_b.expect("second concurrent start should be idempotent");
        assert!(manager.is_running().await);

        let stop_a = RuntimeService::stop(&manager);
        let stop_b = RuntimeService::stop(&manager);
        let (stop_a, stop_b) = tokio::join!(stop_a, stop_b);
        stop_a.expect("first concurrent stop should succeed");
        stop_b.expect("second concurrent stop should be idempotent");
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_manager_stop_drains_owned_tasks() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();

        RuntimeService::start(&manager, &context).await.unwrap();
        let task_group = manager
            .tasks
            .lock()
            .await
            .clone()
            .expect("running rendezvous service should own a task group");

        RuntimeService::stop(&manager).await.unwrap();
        task_group
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect("rendezvous task group should drain on stop");
        assert!(
            task_group.active_tasks().is_empty(),
            "rendezvous task group should not leak owned tasks after stop"
        );
    }

    #[tokio::test]
    async fn test_descriptor_caching() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();
        RuntimeService::start(&manager, &context).await.unwrap();

        let descriptor = RendezvousDescriptor {
            authority_id: test_peer(),
            device_id: None,
            context_id: test_context(),
            transport_hints: vec![TransportHint::quic_direct("127.0.0.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

        manager.cache_descriptor(descriptor.clone()).await.unwrap();

        let cached = manager
            .get_descriptor(test_context(), test_peer())
            .await
            .unwrap();
        assert_eq!(cached.authority_id, test_peer());

        RuntimeService::stop(&manager).await.unwrap();
    }

    #[tokio::test]
    async fn test_publish_descriptor() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();
        RuntimeService::start(&manager, &context).await.unwrap();

        let snapshot = test_snapshot(test_authority(), test_context());
        let mock_effects = MockEffects;
        let outcome = manager
            .publish_descriptor(
                test_context(),
                Some(vec![TransportHint::quic_direct("127.0.0.1:8443").unwrap()]),
                1000,
                &snapshot,
                &mock_effects,
            )
            .await
            .unwrap();

        assert!(outcome.decision.is_allowed());

        RuntimeService::stop(&manager).await.unwrap();
    }

    #[tokio::test]
    async fn test_needs_refresh() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();
        RuntimeService::start(&manager, &context).await.unwrap();

        // No descriptor cached - should need refresh
        assert!(manager.needs_refresh(test_context(), 1000).await);

        RuntimeService::stop(&manager).await.unwrap();
    }

    #[test]
    fn test_relay_first_ordering_places_relay_before_direct() {
        let relay = TransportHint::websocket_relay(test_peer());
        let direct = TransportHint::quic_direct("127.0.0.1:8443").unwrap();
        let ordered = RendezvousManager::relay_first_order(vec![direct.clone(), relay.clone()]);

        assert!(matches!(
            ordered.first(),
            Some(TransportHint::WebSocketRelay { .. })
        ));
        assert!(ordered.iter().any(|hint| hint == &direct));
    }

    #[tokio::test]
    async fn test_direct_upgrade_candidates_filter_recoverable_direct_hints() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        let context = test_service_context();
        RuntimeService::start(&manager, &context).await.unwrap();

        let descriptor = RendezvousDescriptor {
            authority_id: test_peer(),
            device_id: None,
            context_id: test_context(),
            transport_hints: vec![
                TransportHint::websocket_relay(test_authority()),
                TransportHint::quic_direct("10.0.0.42:8443").unwrap(),
            ],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };
        manager.cache_descriptor(descriptor).await.unwrap();

        let mut interfaces = LocalInterfaces::new();
        interfaces.insert("10.0.0.42");
        let candidates = manager
            .direct_upgrade_candidates(test_context(), test_peer(), &interfaces)
            .await;
        assert_eq!(candidates.len(), 1);
        assert!(matches!(candidates[0], TransportHint::QuicDirect { .. }));

        let interfaces = LocalInterfaces::new();
        let none = manager
            .direct_upgrade_candidates(test_context(), test_peer(), &interfaces)
            .await;
        assert!(none.is_empty());

        RuntimeService::stop(&manager).await.unwrap();
    }
}

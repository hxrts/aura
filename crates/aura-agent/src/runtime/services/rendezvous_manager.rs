//! Rendezvous Service Manager
//!
//! Wraps `aura_rendezvous::RendezvousService` for integration with the agent runtime.
//! Provides lifecycle management, descriptor caching, and channel establishment.
//!
//! ## LAN Discovery
//!
//! Supports local network peer discovery via UDP broadcast. When enabled, the manager
//! will announce presence and discover peers on the local network.

use aura_core::crypto::single_signer::SingleSignerKeyPackage;
use aura_core::effects::network::{UdpEffects, UdpEndpoint};
use aura_core::effects::secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{CryptoEffects, NoiseEffects};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_rendezvous::{
    DiscoveredPeer, LanDiscoveryConfig, RendezvousConfig, RendezvousDescriptor, RendezvousFact,
    RendezvousService, TransportHint,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::sync::RwLock;

use super::lan_discovery::LanDiscoveryService;
use super::state::{with_state_mut, with_state_mut_validated};

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
}

struct RendezvousState {
    status: RendezvousManagerState,
    service: Option<Arc<RendezvousService>>,
    cleanup_task: Option<tokio::task::JoinHandle<()>>,
    lan_discovery: Option<Arc<LanDiscoveryService>>,
    lan_tasks: LanTaskHandles,
    lan_discovered_peers: HashMap<AuthorityId, DiscoveredPeer>,
    descriptor_cache: HashMap<(ContextId, AuthorityId), RendezvousDescriptor>,
}

impl RendezvousState {
    fn new() -> Self {
        Self {
            status: RendezvousManagerState::Stopped,
            service: None,
            cleanup_task: None,
            lan_discovery: None,
            lan_tasks: None,
            lan_discovered_peers: HashMap::new(),
            descriptor_cache: HashMap::new(),
        }
    }

    fn validate(&self) -> Result<(), String> {
        // Running requires service; Stopping still has service while shutting down
        if self.status == RendezvousManagerState::Running && self.service.is_none() {
            return Err("rendezvous running without service".to_string());
        }
        // Service must be gone when fully stopped
        if self.status == RendezvousManagerState::Stopped && self.service.is_some() {
            return Err("rendezvous stopped with active service".to_string());
        }
        // cleanup_task can exist during Running (active) and Stopping (being shut down)
        if self.cleanup_task.is_some()
            && !matches!(
                self.status,
                RendezvousManagerState::Running | RendezvousManagerState::Stopping
            )
        {
            return Err("cleanup task active while not running or stopping".to_string());
        }
        // lan_tasks can exist during Running and Stopping
        if self.lan_tasks.is_some()
            && self.lan_discovery.is_none()
            && !matches!(self.status, RendezvousManagerState::Stopping)
        {
            return Err("lan tasks active without discovery service".to_string());
        }
        Ok(())
    }
}

/// Task handles for LAN discovery announcer and listener.
type LanTaskHandles = Option<(tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)>;

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
pub struct RendezvousManager {
    /// Configuration
    config: RendezvousManagerConfig,

    /// Owned state (service, caches, lifecycle)
    state: Arc<RwLock<RendezvousState>>,

    /// Authority ID
    authority_id: AuthorityId,

    /// Time effects (simulator-controllable)
    time: Arc<dyn PhysicalTimeEffects>,

    /// UDP effects for LAN discovery sockets
    udp: Arc<dyn UdpEffects>,

    /// Shutdown signal for background tasks
    shutdown_tx: watch::Sender<bool>,
}

impl RendezvousManager {
    /// Create a new rendezvous manager
    pub fn new(
        authority_id: AuthorityId,
        config: RendezvousManagerConfig,
        time: Arc<dyn PhysicalTimeEffects>,
        udp: Arc<dyn UdpEffects>,
    ) -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            config,
            state: Arc::new(RwLock::new(RendezvousState::new())),
            authority_id,
            time,
            udp,
            shutdown_tx,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(authority_id: AuthorityId, time: Arc<dyn PhysicalTimeEffects>) -> Self {
        Self::new(
            authority_id,
            RendezvousManagerConfig::default(),
            time,
            Arc::new(aura_effects::RealUdpEffectsHandler::new()),
        )
    }

    /// Get the current state
    pub async fn state(&self) -> RendezvousManagerState {
        self.state.read().await.status
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        self.state.read().await.status == RendezvousManagerState::Running
    }

    /// Start the rendezvous manager
    pub async fn start(&self) -> Result<(), String> {
        let current_state = self.state.read().await.status;
        if current_state == RendezvousManagerState::Running {
            return Ok(()); // Already running
        }

        let _ = self.shutdown_tx.send(false);
        with_state_mut_validated(
            &self.state,
            |state| state.status = RendezvousManagerState::Starting,
            |state| state.validate(),
        )
        .await;

        // Create rendezvous config from manager config
        let rendezvous_config = RendezvousConfig {
            descriptor_validity_ms: self.config.descriptor_validity.as_millis() as u64,
            probe_timeout_ms: 5000,
            stun_server: None,
            max_relay_hops: 3,
        };

        // Create the underlying rendezvous service
        let service = RendezvousService::new(self.authority_id, rendezvous_config);

        with_state_mut_validated(
            &self.state,
            |state| {
                state.service = Some(Arc::new(service));
                state.status = RendezvousManagerState::Running;
            },
            |state| state.validate(),
        )
        .await;

        // Start background cleanup task if enabled
        if self.config.auto_cleanup_enabled {
            self.start_cleanup_task().await;
        }

        // Start LAN discovery if enabled
        if self.config.lan_discovery.enabled {
            if let Err(e) = self.start_lan_discovery().await {
                tracing::warn!("Failed to start LAN discovery: {}", e);
            }
        }

        tracing::info!(
            "Rendezvous manager started for authority {}",
            self.authority_id
        );
        Ok(())
    }

    /// Stop the rendezvous manager
    pub async fn stop(&self) -> Result<(), String> {
        let current_state = self.state.read().await.status;
        if current_state == RendezvousManagerState::Stopped {
            return Ok(()); // Already stopped
        }

        with_state_mut_validated(
            &self.state,
            |state| state.status = RendezvousManagerState::Stopping,
            |state| state.validate(),
        )
        .await;
        let _ = self.shutdown_tx.send(true);

        // Stop LAN discovery
        self.stop_lan_discovery().await;

        // Cancel cleanup task if running
        let cleanup_task = with_state_mut(&self.state, |state| state.cleanup_task.take()).await;
        if let Some(handle) = cleanup_task {
            let _ = handle.await;
        }

        with_state_mut_validated(
            &self.state,
            |state| {
                state.service = None;
                state.status = RendezvousManagerState::Stopped;
            },
            |state| state.validate(),
        )
        .await;

        tracing::info!("Rendezvous manager stopped");
        Ok(())
    }

    /// Start the background cleanup task
    async fn start_cleanup_task(&self) {
        let interval = self.config.cleanup_interval;
        let state = self.state.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let time = self.time.clone();

        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                    }
                    _ = time.sleep_ms(interval.as_millis() as u64) => {
                        // Check if still running
                        if state.read().await.status != RendezvousManagerState::Running {
                            break;
                        }

                        // Perform cleanup
                        let now_ms = match time.physical_time().await {
                            Ok(t) => t.ts_ms,
                            Err(_) => continue,
                        };
                        with_state_mut(&state, |state| {
                            state
                                .descriptor_cache
                                .retain(|_, descriptor| descriptor.is_valid(now_ms));
                        })
                        .await;
                    }
                }
            }
        });

        with_state_mut_validated(
            &self.state,
            |state| state.cleanup_task = Some(handle),
            |state| state.validate(),
        )
        .await;
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

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
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self
            .state
            .read()
            .await
            .service
            .clone()
            .ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());

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
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self
            .state
            .read()
            .await
            .service
            .clone()
            .ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());

        // Retrieve identity keys to get public key
        let keys = retrieve_identity_keys(effects, &self.authority_id).await;
        let public_key = keys.map(|(_, pub_key)| pub_key).unwrap_or([0u8; 32]);

        Ok(service.prepare_refresh_descriptor(snapshot, context_id, hints, public_key, now_ms))
    }

    /// Cache a peer's descriptor
    pub async fn cache_descriptor(&self, descriptor: RendezvousDescriptor) -> Result<(), String> {
        with_state_mut(&self.state, |state| {
            state
                .descriptor_cache
                .insert((descriptor.context_id, descriptor.authority_id), descriptor);
        })
        .await;
        Ok(())
    }

    /// Get a cached descriptor for a peer
    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.state
            .read()
            .await
            .descriptor_cache
            .get(&(context_id, peer))
            .cloned()
    }

    /// Check if our descriptor needs refresh in a context
    pub async fn needs_refresh(&self, context_id: ContextId, now_ms: u64) -> bool {
        self.state
            .read()
            .await
            .descriptor_cache
            .get(&(context_id, self.authority_id))
            .map(|desc| {
                let refresh_threshold = desc
                    .valid_until
                    .saturating_sub(self.config.refresh_window.as_millis() as u64);
                now_ms >= refresh_threshold
            })
            .unwrap_or(true)
    }

    /// Get contexts needing descriptor refresh
    pub async fn contexts_needing_refresh(&self, now_ms: u64) -> Vec<ContextId> {
        let refresh_window_ms = self.config.refresh_window.as_millis() as u64;
        self.state
            .read()
            .await
            .descriptor_cache
            .iter()
            .filter(|((_, auth), desc)| {
                *auth == self.authority_id && {
                    let refresh_threshold = desc.valid_until.saturating_sub(refresh_window_ms);
                    now_ms >= refresh_threshold
                }
            })
            .map(|((ctx, _), _)| *ctx)
            .collect()
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Prepare to establish a channel with a peer
    pub async fn prepare_establish_channel<E: NoiseEffects + CryptoEffects + SecureStorageEffects>(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        psk: &[u8; 32],
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
        effects: &E,
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self
            .state
            .read()
            .await
            .service
            .clone()
            .ok_or("Rendezvous manager not started")?;
        let descriptor = self
            .state
            .read()
            .await
            .descriptor_cache
            .get(&(context_id, peer))
            .cloned()
            .ok_or("Peer descriptor not found in cache")?;

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
                &descriptor,
                effects
            )
            .await
            .map_err(|e| format!("Failed to prepare channel: {e}"))
    }

    /// Create a channel established fact
    pub async fn create_channel_fact(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> Result<RendezvousFact, String> {
        let service = self
            .state
            .read()
            .await
            .service
            .clone()
            .ok_or("Rendezvous manager not started")?;

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
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self
            .state
            .read()
            .await
            .service
            .clone()
            .ok_or("Rendezvous manager not started")?;

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
        self.state
            .read()
            .await
            .descriptor_cache
            .keys()
            .filter(|(_, auth)| *auth != self.authority_id)
            .map(|(_, auth)| *auth)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// List all cached peers for a specific context (excluding self)
    pub async fn list_cached_peers_for_context(&self, context_id: ContextId) -> Vec<AuthorityId> {
        self.state
            .read()
            .await
            .descriptor_cache
            .keys()
            .filter(|(ctx, auth)| *ctx == context_id && *auth != self.authority_id)
            .map(|(_, auth)| *auth)
            .collect()
    }

    // ========================================================================
    // LAN Discovery Operations
    // ========================================================================

    /// Start LAN discovery
    ///
    /// Creates a LAN discovery service and starts the announcer and listener.
    /// Discovered peers are cached and their descriptors are stored.
    pub async fn start_lan_discovery(&self) -> Result<(), String> {
        if !self.config.lan_discovery.enabled {
            return Ok(());
        }

        // Create LAN discovery service
        let time: Arc<dyn PhysicalTimeEffects> = self.time.clone();
        let lan_service = LanDiscoveryService::new(
            self.config.lan_discovery.clone(),
            self.authority_id,
            self.udp.clone(),
            time,
        )
        .await
        .map_err(|e| format!("Failed to create LAN discovery service: {}", e))?;

        // Set up callback to cache discovered peers
        let state = self.state.clone();

        let (announcer_handle, listener_handle) = lan_service.start(move |peer: DiscoveredPeer| {
            let state = state.clone();
            let peer_clone = peer.clone();

            tokio::spawn(async move {
                with_state_mut(&state, |state| {
                    state
                        .lan_discovered_peers
                        .insert(peer_clone.authority_id, peer_clone.clone());
                    state.descriptor_cache.insert(
                        (
                            peer_clone.descriptor.context_id,
                            peer_clone.descriptor.authority_id,
                        ),
                        peer_clone.descriptor,
                    );
                })
                .await;

                tracing::info!(
                    authority = %peer.authority_id,
                    addr = %peer.source_addr,
                    "Cached LAN-discovered peer"
                );
            });
        });

        let lan_service = Arc::new(lan_service);
        with_state_mut_validated(
            &self.state,
            |state| {
                state.lan_discovery = Some(lan_service);
                state.lan_tasks = Some((announcer_handle, listener_handle));
            },
            |state| state.validate(),
        )
        .await;

        tracing::info!(
            "LAN discovery started on port {}",
            self.config.lan_discovery.port
        );
        Ok(())
    }

    /// Stop LAN discovery
    pub async fn stop_lan_discovery(&self) {
        // Signal shutdown
        if let Some(service) = self.state.read().await.lan_discovery.as_ref() {
            service.stop();
        }

        // Abort tasks
        let tasks = with_state_mut_validated(
            &self.state,
            |state| state.lan_tasks.take(),
            |state| state.validate(),
        )
        .await;
        if let Some((announcer, listener)) = tasks {
            announcer.abort();
            listener.abort();
        }

        with_state_mut_validated(
            &self.state,
            |state| state.lan_discovery = None,
            |state| state.validate(),
        )
        .await;
        tracing::info!("LAN discovery stopped");
    }

    /// Set the descriptor to announce on LAN
    ///
    /// Should be called after publishing a descriptor to start announcing on LAN.
    pub async fn set_lan_descriptor(&self, descriptor: RendezvousDescriptor) {
        let service = self.state.read().await.lan_discovery.clone();
        if let Some(service) = service.as_ref() {
            service.set_descriptor(descriptor).await;
        }
    }

    /// Get LAN-discovered peers
    ///
    /// Returns a copy of all peers discovered via LAN broadcast.
    pub async fn list_lan_discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.state
            .read()
            .await
            .lan_discovered_peers
            .values()
            .cloned()
            .collect()
    }

    /// Get a specific LAN-discovered peer
    pub async fn get_lan_discovered_peer(
        &self,
        authority_id: AuthorityId,
    ) -> Option<DiscoveredPeer> {
        self.state
            .read()
            .await
            .lan_discovered_peers
            .get(&authority_id)
            .cloned()
    }

    /// Check if LAN discovery is enabled and running
    pub async fn is_lan_discovery_running(&self) -> bool {
        self.state.read().await.lan_discovery.is_some()
    }

    /// Clear expired LAN-discovered peers
    ///
    /// Removes peers that haven't been seen for longer than the specified duration.
    pub async fn prune_lan_peers(&self, max_age_ms: u64) {
        let now_ms = match self.time.physical_time().await {
            Ok(t) => t.ts_ms,
            Err(_) => return,
        };

        with_state_mut(&self.state, |state| {
            state
                .lan_discovered_peers
                .retain(|_, peer| now_ms.saturating_sub(peer.discovered_at_ms) < max_age_ms);
        })
        .await;
    }

    /// Trigger an on-demand discovery refresh
    ///
    /// For LAN discovery, this starts the LAN discovery service if not already running.
    pub async fn trigger_discovery(&self) -> Result<(), String> {
        // Start LAN discovery if not running and enabled
        if self.config.lan_discovery.enabled && !self.is_lan_discovery_running().await {
            self.start_lan_discovery().await?;
        }

        Ok(())
    }

    /// Send an invitation to a LAN peer
    ///
    /// This method uses the LAN transport to send an invitation code directly
    /// to a peer discovered on the local network.
    pub async fn send_lan_invitation(
        &self,
        _peer_authority: &AuthorityId,
        peer_address: &str,
        invitation_code: &str,
    ) -> Result<(), String> {
        // Parse the peer address
        let addr: std::net::SocketAddr = peer_address
            .parse()
            .map_err(|e| format!("Invalid peer address: {}", e))?;
        let addr = UdpEndpoint::new(addr.to_string());

        // Get the LAN discovery service for sending
        let lan = self.state.read().await.lan_discovery.clone();
        if let Some(lan) = lan.as_ref() {
            // Use the LAN socket to send the invitation code
            // The invitation is sent as a simple UDP packet
            let message = format!("AURA_INV:{}", invitation_code);
            let socket = lan.socket();
            socket
                .send_to(message.as_bytes(), &addr)
                .await
                .map_err(|e| format!("Failed to send invitation: {}", e))?;

            tracing::info!(
                address = %addr,
                "Sent LAN invitation"
            );
            Ok(())
        } else {
            Err("LAN discovery not running".to_string())
        }
    }

    // ========================================================================
    // Accessors
    // ========================================================================

    /// Get the configuration
    pub fn config(&self) -> &RendezvousManagerConfig {
        &self.config
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

// =============================================================================
// RuntimeService Implementation
// =============================================================================

use super::traits::{RuntimeService, ServiceError, ServiceHealth};
use super::RuntimeTaskRegistry;
use async_trait::async_trait;

#[async_trait]
impl RuntimeService for RendezvousManager {
    fn name(&self) -> &'static str {
        "rendezvous_manager"
    }

    fn dependencies(&self) -> &[&'static str] {
        &["transport"]
    }

    async fn start(&self, _tasks: Arc<RuntimeTaskRegistry>) -> Result<(), ServiceError> {
        self.start()
            .await
            .map_err(|e| ServiceError::startup_failed("rendezvous_manager", e))
    }

    async fn stop(&self) -> Result<(), ServiceError> {
        self.stop()
            .await
            .map_err(|e| ServiceError::shutdown_failed("rendezvous_manager", e))
    }

    fn health(&self) -> ServiceHealth {
        // Synchronous approximation - for full health use async method
        ServiceHealth::Healthy
    }
}

impl RendezvousManager {
    /// Get the service health status asynchronously
    pub async fn health_async(&self) -> ServiceHealth {
        let state = self.state().await;
        match state {
            RendezvousManagerState::Stopped => ServiceHealth::Stopped,
            RendezvousManagerState::Starting => ServiceHealth::Starting,
            RendezvousManagerState::Stopping => ServiceHealth::Stopping,
            RendezvousManagerState::Running => {
                // Check if underlying service is available
                if self.state.read().await.service.is_some() {
                    ServiceHealth::Healthy
                } else {
                    ServiceHealth::Unhealthy {
                        reason: "underlying service not available".to_string(),
                    }
                }
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
    use aura_core::FlowCost;
    use aura_effects::time::PhysicalTimeHandler;
    use aura_guards::types::CapabilityId;
    use aura_rendezvous::GuardSnapshot;
    use aura_core::effects::noise::{HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState};
    use aura_core::effects::{CryptoCoreEffects, CryptoExtendedEffects, CryptoError, RandomCoreEffects};
    use async_trait::async_trait;

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
        Arc::new(aura_effects::RealUdpEffectsHandler::new())
    }
    
    // Mock for tests
    struct MockEffects;
    #[async_trait]
    impl SecureStorageEffects for MockEffects {
        async fn secure_store(&self, _: &SecureStorageLocation, _: &[u8], _: &[SecureStorageCapability]) -> Result<(), AuraError> { Ok(()) }
        async fn secure_retrieve(&self, _: &SecureStorageLocation, _: &[SecureStorageCapability]) -> Result<Vec<u8>, AuraError> { Ok(vec![]) }
        async fn secure_delete(&self, _: &SecureStorageLocation, _: &[SecureStorageCapability]) -> Result<(), AuraError> { Ok(()) }
        async fn list_keys(&self, _: &str, _: &[SecureStorageCapability]) -> Result<Vec<String>, AuraError> { Ok(vec![]) }
    }
    #[async_trait]
    impl NoiseEffects for MockEffects {
        async fn create_handshake_state(&self, _: NoiseParams) -> Result<HandshakeState, NoiseError> { Ok(HandshakeState(Box::new(()))) }
        async fn write_message(&self, _: HandshakeState, _: &[u8]) -> Result<(Vec<u8>, HandshakeState), NoiseError> { Ok((vec![], HandshakeState(Box::new(())))) }
        async fn read_message(&self, _: HandshakeState, _: &[u8]) -> Result<(Vec<u8>, HandshakeState), NoiseError> { Ok((vec![], HandshakeState(Box::new(())))) }
        async fn into_transport_mode(&self, _: HandshakeState) -> Result<TransportState, NoiseError> { Ok(TransportState(Box::new(()))) }
        async fn encrypt_transport_message(&self, _: &mut TransportState, _: &[u8]) -> Result<Vec<u8>, NoiseError> { Ok(vec![]) }
        async fn decrypt_transport_message(&self, _: &mut TransportState, _: &[u8]) -> Result<Vec<u8>, NoiseError> { Ok(vec![]) }
    }
    // Stub other traits needed by E
    #[async_trait]
    impl RandomCoreEffects for MockEffects {
        async fn random_bytes(&self, _: usize) -> Vec<u8> { vec![] }
        async fn random_bytes_32(&self) -> [u8; 32] { [0u8; 32] }
        async fn random_u64(&self) -> u64 { 0 }
        async fn random_range(&self, _: u64, _: u64) -> u64 { 0 }
        async fn random_uuid(&self) -> uuid::Uuid { uuid::Uuid::nil() }
    }
    #[async_trait]
    impl CryptoCoreEffects for MockEffects {
        async fn hkdf_derive(&self, _: &[u8], _: &[u8], _: &[u8], _: u32) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn derive_key(&self, _: &[u8], _: &aura_core::effects::crypto::KeyDerivationContext) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> { Ok((vec![], vec![])) }
        async fn ed25519_sign(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn ed25519_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> { Ok(true) }
        fn is_simulated(&self) -> bool { false }
        fn crypto_capabilities(&self) -> Vec<String> { vec![] }
        fn constant_time_eq(&self, _: &[u8], _: &[u8]) -> bool { true }
        fn secure_zero(&self, _: &mut [u8]) {}
    }
    #[async_trait]
    impl CryptoExtendedEffects for MockEffects {
        async fn convert_ed25519_to_x25519_public(&self, _: &[u8]) -> Result<[u8; 32], CryptoError> { Ok([0u8; 32]) }
        async fn convert_ed25519_to_x25519_private(&self, _: &[u8]) -> Result<[u8; 32], CryptoError> { Ok([0u8; 32]) }
    }
    impl CryptoEffects for MockEffects {}

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

        // Start
        manager.start().await.unwrap();
        assert!(manager.is_running().await);

        // Stop
        manager.stop().await.unwrap();
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_descriptor_caching() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        manager.start().await.unwrap();

        let descriptor = RendezvousDescriptor {
            authority_id: test_peer(),
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

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_publish_descriptor() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        manager.start().await.unwrap();

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

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_needs_refresh() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config, test_time(), test_udp());
        manager.start().await.unwrap();

        // No descriptor cached - should need refresh
        assert!(manager.needs_refresh(test_context(), 1000).await);

        manager.stop().await.unwrap();
    }
}
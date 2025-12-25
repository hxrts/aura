//! Rendezvous Service Manager
//!
//! Wraps `aura_rendezvous::RendezvousService` for integration with the agent runtime.
//! Provides lifecycle management, descriptor caching, and channel establishment.
//!
//! ## LAN Discovery
//!
//! Supports local network peer discovery via UDP broadcast. When enabled, the manager
//! will announce presence and discover peers on the local network.

use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_effects::time::PhysicalTimeHandler;
use aura_rendezvous::{
    DiscoveredPeer, LanDiscoveryConfig, RendezvousConfig, RendezvousDescriptor, RendezvousFact,
    RendezvousService, TransportHint,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::lan_discovery::LanDiscoveryService;

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
            default_transport_hints: vec![TransportHint::QuicDirect {
                addr: "127.0.0.1:0".to_string(),
            }],
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
    /// Inner rendezvous service
    service: Arc<RwLock<Option<RendezvousService>>>,

    /// Configuration
    config: RendezvousManagerConfig,

    /// Current state
    state: Arc<RwLock<RendezvousManagerState>>,

    /// Authority ID
    authority_id: AuthorityId,

    /// Background cleanup task handle
    cleanup_task: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,

    /// LAN discovery service (if enabled)
    lan_discovery: Arc<RwLock<Option<LanDiscoveryService>>>,

    /// LAN discovery task handles (announcer, listener)
    lan_tasks: Arc<RwLock<LanTaskHandles>>,

    /// LAN-discovered peers (authority_id -> DiscoveredPeer)
    lan_discovered_peers: Arc<RwLock<HashMap<AuthorityId, DiscoveredPeer>>>,

    /// Cached peer descriptors by (context, authority)
    descriptor_cache: Arc<RwLock<HashMap<(ContextId, AuthorityId), RendezvousDescriptor>>>,
}

impl RendezvousManager {
    /// Create a new rendezvous manager
    pub fn new(authority_id: AuthorityId, config: RendezvousManagerConfig) -> Self {
        Self {
            service: Arc::new(RwLock::new(None)),
            config,
            state: Arc::new(RwLock::new(RendezvousManagerState::Stopped)),
            authority_id,
            cleanup_task: Arc::new(RwLock::new(None)),
            lan_discovery: Arc::new(RwLock::new(None)),
            lan_tasks: Arc::new(RwLock::new(None)),
            lan_discovered_peers: Arc::new(RwLock::new(HashMap::new())),
            descriptor_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(authority_id: AuthorityId) -> Self {
        Self::new(authority_id, RendezvousManagerConfig::default())
    }

    /// Get the current state
    pub async fn state(&self) -> RendezvousManagerState {
        *self.state.read().await
    }

    /// Check if the service is running
    pub async fn is_running(&self) -> bool {
        *self.state.read().await == RendezvousManagerState::Running
    }

    /// Start the rendezvous manager
    pub async fn start(&self) -> Result<(), String> {
        let current_state = *self.state.read().await;
        if current_state == RendezvousManagerState::Running {
            return Ok(()); // Already running
        }

        *self.state.write().await = RendezvousManagerState::Starting;

        // Create rendezvous config from manager config
        let rendezvous_config = RendezvousConfig {
            descriptor_validity_ms: self.config.descriptor_validity.as_millis() as u64,
            probe_timeout_ms: 5000,
            stun_server: None,
            max_relay_hops: 3,
        };

        // Create the underlying rendezvous service
        let service = RendezvousService::new(self.authority_id, rendezvous_config);

        *self.service.write().await = Some(service);
        *self.state.write().await = RendezvousManagerState::Running;

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
        let current_state = *self.state.read().await;
        if current_state == RendezvousManagerState::Stopped {
            return Ok(()); // Already stopped
        }

        *self.state.write().await = RendezvousManagerState::Stopping;

        // Stop LAN discovery
        self.stop_lan_discovery().await;

        // Cancel cleanup task if running
        if let Some(handle) = self.cleanup_task.write().await.take() {
            handle.abort();
        }

        *self.service.write().await = None;
        *self.state.write().await = RendezvousManagerState::Stopped;

        tracing::info!("Rendezvous manager stopped");
        Ok(())
    }

    /// Start the background cleanup task
    async fn start_cleanup_task(&self) {
        let interval = self.config.cleanup_interval;
        let state = self.state.clone();
        let descriptor_cache = self.descriptor_cache.clone();
        let clock = PhysicalTimeHandler::new();

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            let clock = clock;
            loop {
                ticker.tick().await;

                // Check if still running
                if *state.read().await != RendezvousManagerState::Running {
                    break;
                }

                // Perform cleanup
                let now_ms = clock.physical_time_now_ms();
                descriptor_cache
                    .write()
                    .await
                    .retain(|_, descriptor| descriptor.is_valid(now_ms));
            }
        });

        *self.cleanup_task.write().await = Some(handle);
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

    /// Publish a transport descriptor for a context
    ///
    /// Returns the guard outcome with the descriptor fact.
    pub async fn publish_descriptor(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());

        Ok(service.prepare_publish_descriptor(snapshot, context_id, hints, now_ms))
    }

    /// Refresh a descriptor for a context
    ///
    /// Returns the guard outcome with the new descriptor fact.
    pub async fn refresh_descriptor(
        &self,
        context_id: ContextId,
        transport_hints: Option<Vec<TransportHint>>,
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

        let hints = transport_hints.unwrap_or_else(|| self.config.default_transport_hints.clone());

        Ok(service.prepare_refresh_descriptor(snapshot, context_id, hints, now_ms))
    }

    /// Cache a peer's descriptor
    pub async fn cache_descriptor(&self, descriptor: RendezvousDescriptor) -> Result<(), String> {
        self.descriptor_cache
            .write()
            .await
            .insert((descriptor.context_id, descriptor.authority_id), descriptor);
        Ok(())
    }

    /// Get a cached descriptor for a peer
    pub async fn get_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.descriptor_cache
            .read()
            .await
            .get(&(context_id, peer))
            .cloned()
    }

    /// Check if our descriptor needs refresh in a context
    pub async fn needs_refresh(&self, context_id: ContextId, now_ms: u64) -> bool {
        self.descriptor_cache
            .read()
            .await
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
        self.descriptor_cache
            .read()
            .await
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
    pub async fn prepare_establish_channel(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        psk: &[u8; 32],
        now_ms: u64,
        snapshot: &aura_rendezvous::GuardSnapshot,
    ) -> Result<aura_rendezvous::GuardOutcome, String> {
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;
        let descriptor = self
            .descriptor_cache
            .read()
            .await
            .get(&(context_id, peer))
            .cloned()
            .ok_or("Peer descriptor not found in cache")?;

        service
            .prepare_establish_channel(snapshot, context_id, peer, psk, now_ms, &descriptor)
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
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

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
        let service = self.service.read().await;
        let service = service.as_ref().ok_or("Rendezvous manager not started")?;

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
        self.descriptor_cache
            .read()
            .await
            .keys()
            .filter(|(_, auth)| *auth != self.authority_id)
            .map(|(_, auth)| *auth)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// List all cached peers for a specific context (excluding self)
    pub async fn list_cached_peers_for_context(&self, context_id: ContextId) -> Vec<AuthorityId> {
        self.descriptor_cache
            .read()
            .await
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
        let time: Arc<dyn PhysicalTimeEffects> = Arc::new(PhysicalTimeHandler::new());
        let lan_service =
            LanDiscoveryService::new(self.config.lan_discovery.clone(), self.authority_id, time)
                .await
                .map_err(|e| format!("Failed to create LAN discovery service: {}", e))?;

        // Set up callback to cache discovered peers
        let discovered_peers = self.lan_discovered_peers.clone();
        let descriptor_cache = self.descriptor_cache.clone();

        let (announcer_handle, listener_handle) = lan_service.start(move |peer: DiscoveredPeer| {
            let discovered_peers = discovered_peers.clone();
            let service = service.clone();
            let peer_clone = peer.clone();

            // Spawn a task to handle the discovery
            tokio::spawn(async move {
                // Cache the peer
                {
                    let mut peers = discovered_peers.write().await;
                    peers.insert(peer_clone.authority_id, peer_clone.clone());
                }

                // Also cache the descriptor locally for rendezvous resolution.
                descriptor_cache
                    .write()
                    .await
                    .insert(
                        (peer_clone.descriptor.context_id, peer_clone.descriptor.authority_id),
                        peer_clone.descriptor,
                    );

                tracing::info!(
                    authority = %peer.authority_id,
                    addr = %peer.source_addr,
                    "Cached LAN-discovered peer"
                );
            });
        });

        *self.lan_discovery.write().await = Some(lan_service);
        *self.lan_tasks.write().await = Some((announcer_handle, listener_handle));

        tracing::info!(
            "LAN discovery started on port {}",
            self.config.lan_discovery.port
        );
        Ok(())
    }

    /// Stop LAN discovery
    pub async fn stop_lan_discovery(&self) {
        // Signal shutdown
        if let Some(service) = self.lan_discovery.read().await.as_ref() {
            service.stop();
        }

        // Abort tasks
        if let Some((announcer, listener)) = self.lan_tasks.write().await.take() {
            announcer.abort();
            listener.abort();
        }

        *self.lan_discovery.write().await = None;
        tracing::info!("LAN discovery stopped");
    }

    /// Set the descriptor to announce on LAN
    ///
    /// Should be called after publishing a descriptor to start announcing on LAN.
    pub async fn set_lan_descriptor(&self, descriptor: RendezvousDescriptor) {
        if let Some(service) = self.lan_discovery.read().await.as_ref() {
            service.set_descriptor(descriptor).await;
        }
    }

    /// Get LAN-discovered peers
    ///
    /// Returns a copy of all peers discovered via LAN broadcast.
    pub async fn list_lan_discovered_peers(&self) -> Vec<DiscoveredPeer> {
        self.lan_discovered_peers
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    /// Get a specific LAN-discovered peer
    pub async fn get_lan_discovered_peer(
        &self,
        authority_id: AuthorityId,
    ) -> Option<DiscoveredPeer> {
        self.lan_discovered_peers
            .read()
            .await
            .get(&authority_id)
            .cloned()
    }

    /// Check if LAN discovery is enabled and running
    pub async fn is_lan_discovery_running(&self) -> bool {
        self.lan_discovery.read().await.is_some()
    }

    /// Clear expired LAN-discovered peers
    ///
    /// Removes peers that haven't been seen for longer than the specified duration.
    pub async fn prune_lan_peers(&self, max_age_ms: u64) {
        let now_ms = PhysicalTimeHandler::new().physical_time_now_ms();

        let mut peers = self.lan_discovered_peers.write().await;
        peers.retain(|_, peer| now_ms.saturating_sub(peer.discovered_at_ms) < max_age_ms);
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

        // Get the LAN discovery service for sending
        let lan_guard = self.lan_discovery.read().await;
        if let Some(lan) = lan_guard.as_ref() {
            // Use the LAN socket to send the invitation code
            // The invitation is sent as a simple UDP packet
            let message = format!("AURA_INV:{}", invitation_code);
            let socket = lan.socket();
            socket
                .send_to(message.as_bytes(), addr)
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

#[cfg(test)]
mod tests {
    use super::*;
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
            flow_budget_remaining: 1000,
            capabilities: vec![
                "rendezvous:publish".to_string(),
                "rendezvous:connect".to_string(),
            ],
            epoch: 1,
        }
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config);

        assert_eq!(manager.state().await, RendezvousManagerState::Stopped);
        assert!(!manager.is_running().await);
    }

    #[tokio::test]
    async fn test_manager_lifecycle() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config);

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
        let manager = RendezvousManager::new(test_authority(), config);
        manager.start().await.unwrap();

        let descriptor = RendezvousDescriptor {
            authority_id: test_peer(),
            context_id: test_context(),
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "127.0.0.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            display_name: None,
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
        let manager = RendezvousManager::new(test_authority(), config);
        manager.start().await.unwrap();

        let snapshot = test_snapshot(test_authority(), test_context());
        let outcome = manager
            .publish_descriptor(
                test_context(),
                Some(vec![TransportHint::QuicDirect {
                    addr: "127.0.0.1:8443".to_string(),
                }]),
                1000,
                &snapshot,
            )
            .await
            .unwrap();

        assert!(outcome.decision.is_allowed());

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_needs_refresh() {
        let config = RendezvousManagerConfig::for_testing();
        let manager = RendezvousManager::new(test_authority(), config);
        manager.start().await.unwrap();

        // No descriptor cached - should need refresh
        assert!(manager.needs_refresh(test_context(), 1000).await);

        manager.stop().await.unwrap();
    }
}

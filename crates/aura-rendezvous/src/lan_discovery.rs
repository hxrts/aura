//! LAN Discovery via UDP Broadcast
//!
//! Enables peer discovery on local networks without requiring prior relationships.
//! Uses UDP broadcast to announce presence and discover other Aura nodes.
//!
//! ## Design
//!
//! - **Announcer**: Periodically broadcasts presence with transport descriptor
//! - **Listener**: Receives broadcasts and notifies on peer discovery
//! - **Service**: Combines announcer and listener for easy management
//!
//! ## Security Considerations
//!
//! LAN discovery is inherently less secure than journal-based rendezvous:
//! - Anyone on the local network can see announcements
//! - Descriptors are not authenticated until invitation redemption
//! - Use for initial discovery only; establish trust via invitations

use crate::facts::{RendezvousDescriptor, TransportHint};
use aura_core::effects::time::TimeEffects;
use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{watch, RwLock};
use tracing::{debug, error, info, trace, warn};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Default UDP port for LAN discovery
pub const DEFAULT_LAN_PORT: u16 = 19433;

/// Default broadcast interval in milliseconds
pub const DEFAULT_ANNOUNCE_INTERVAL_MS: u64 = 5000;

/// Maximum packet size for UDP broadcast
pub const MAX_PACKET_SIZE: usize = 1400;

/// Protocol magic bytes to identify Aura LAN discovery packets
pub const MAGIC_BYTES: &[u8; 4] = b"AURA";

/// Protocol version
pub const PROTOCOL_VERSION: u8 = 1;

// =============================================================================
// CONFIGURATION
// =============================================================================

/// Configuration for LAN discovery
#[derive(Debug, Clone)]
pub struct LanDiscoveryConfig {
    /// UDP port for discovery
    pub port: u16,
    /// Interval between announcements in milliseconds
    pub announce_interval_ms: u64,
    /// Whether LAN discovery is enabled
    pub enabled: bool,
    /// Bind address (typically 0.0.0.0 for all interfaces)
    pub bind_addr: Ipv4Addr,
    /// Broadcast address (typically 255.255.255.255)
    pub broadcast_addr: Ipv4Addr,
}

impl Default for LanDiscoveryConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_LAN_PORT,
            announce_interval_ms: DEFAULT_ANNOUNCE_INTERVAL_MS,
            enabled: true,
            bind_addr: Ipv4Addr::UNSPECIFIED,
            broadcast_addr: Ipv4Addr::BROADCAST,
        }
    }
}

// =============================================================================
// PACKET TYPES
// =============================================================================

/// LAN discovery packet sent via UDP broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanDiscoveryPacket {
    /// Protocol version
    pub version: u8,
    /// Authority announcing presence
    pub authority_id: AuthorityId,
    /// Transport descriptor for connecting
    pub descriptor: RendezvousDescriptor,
    /// Timestamp (ms since epoch)
    pub timestamp_ms: u64,
}

impl LanDiscoveryPacket {
    /// Create a new discovery packet
    pub fn new(
        authority_id: AuthorityId,
        descriptor: RendezvousDescriptor,
        timestamp_ms: u64,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            authority_id,
            descriptor,
            timestamp_ms,
        }
    }

    /// Serialize packet with magic header
    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        let json = serde_json::to_vec(self).ok()?;
        let mut bytes = Vec::with_capacity(MAGIC_BYTES.len() + json.len());
        bytes.extend_from_slice(MAGIC_BYTES);
        bytes.extend(json);
        Some(bytes)
    }

    /// Deserialize packet, validating magic header
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < MAGIC_BYTES.len() {
            return None;
        }
        if &bytes[..MAGIC_BYTES.len()] != MAGIC_BYTES {
            return None;
        }
        serde_json::from_slice(&bytes[MAGIC_BYTES.len()..]).ok()
    }
}

// =============================================================================
// LAN ANNOUNCER
// =============================================================================

/// Announces presence on the local network via UDP broadcast
pub struct LanAnnouncer {
    socket: Arc<UdpSocket>,
    authority_id: AuthorityId,
    descriptor: Arc<RwLock<Option<RendezvousDescriptor>>>,
    interval_ms: u64,
    time: Arc<dyn TimeEffects>,
    broadcast_addr: SocketAddrV4,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl LanAnnouncer {
    /// Create a new LAN announcer
    pub async fn new(
        config: &LanDiscoveryConfig,
        authority_id: AuthorityId,
        time: Arc<dyn TimeEffects>,
    ) -> std::io::Result<Self> {
        let bind_addr = SocketAddrV4::new(config.bind_addr, config.port);
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.set_broadcast(true)?;

        let broadcast_addr = SocketAddrV4::new(config.broadcast_addr, config.port);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            socket: Arc::new(socket),
            authority_id,
            descriptor: Arc::new(RwLock::new(None)),
            interval_ms: config.announce_interval_ms,
            time,
            broadcast_addr,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Set the descriptor to announce
    pub async fn set_descriptor(&self, descriptor: RendezvousDescriptor) {
        let mut guard = self.descriptor.write().await;
        *guard = Some(descriptor);
    }

    /// Clear the descriptor (stop announcing)
    pub async fn clear_descriptor(&self) {
        let mut guard = self.descriptor.write().await;
        *guard = None;
    }

    /// Start the announcer task
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let socket = self.socket.clone();
        let authority_id = self.authority_id;
        let descriptor = self.descriptor.clone();
        let interval_ms = self.interval_ms;
        let time = self.time.clone();
        let broadcast_addr = self.broadcast_addr;
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let desc_guard = descriptor.read().await;
                        if let Some(desc) = desc_guard.as_ref() {
                            // Get current timestamp
                            let timestamp_ms = time.current_timestamp_ms().await;

                            let packet = LanDiscoveryPacket::new(
                                authority_id,
                                desc.clone(),
                                timestamp_ms,
                            );

                            if let Some(bytes) = packet.to_bytes() {
                                if bytes.len() <= MAX_PACKET_SIZE {
                                    match socket.send_to(&bytes, broadcast_addr).await {
                                        Ok(n) => trace!(
                                            authority = %authority_id,
                                            bytes = n,
                                            "LAN announcement sent"
                                        ),
                                        Err(e) => warn!(
                                            error = %e,
                                            "Failed to send LAN announcement"
                                        ),
                                    }
                                } else {
                                    warn!(
                                        size = bytes.len(),
                                        max = MAX_PACKET_SIZE,
                                        "LAN announcement packet too large"
                                    );
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN announcer shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Signal shutdown
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// =============================================================================
// LAN LISTENER
// =============================================================================

/// Discovered peer information
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// Authority that was discovered
    pub authority_id: AuthorityId,
    /// Transport descriptor
    pub descriptor: RendezvousDescriptor,
    /// Source address of the announcement
    pub source_addr: SocketAddr,
    /// Timestamp when discovered
    pub discovered_at_ms: u64,
}

/// Listens for LAN discovery announcements
pub struct LanListener {
    socket: Arc<UdpSocket>,
    local_authority: AuthorityId,
    time: Arc<dyn TimeEffects>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl LanListener {
    /// Create a new LAN listener
    pub async fn new(
        config: &LanDiscoveryConfig,
        local_authority: AuthorityId,
        time: Arc<dyn TimeEffects>,
    ) -> std::io::Result<Self> {
        let bind_addr = SocketAddrV4::new(config.bind_addr, config.port);
        let socket = UdpSocket::bind(bind_addr).await?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            socket: Arc::new(socket),
            local_authority,
            time,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Create listener using an existing socket (for combined announcer/listener)
    pub fn with_socket(
        socket: Arc<UdpSocket>,
        local_authority: AuthorityId,
        time: Arc<dyn TimeEffects>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            socket,
            local_authority,
            time,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Start listening for announcements
    pub fn start<F>(&self, on_discovered: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        let socket = self.socket.clone();
        let local_authority = self.local_authority;
        let mut shutdown_rx = self.shutdown_rx.clone();
        let time = self.time.clone();
        let on_discovered = Arc::new(on_discovered);

        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_PACKET_SIZE];

            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, src_addr)) => {
                                if let Some(packet) = LanDiscoveryPacket::from_bytes(&buf[..len]) {
                                    // Ignore our own announcements
                                    if packet.authority_id == local_authority {
                                        trace!("Ignoring own LAN announcement");
                                        continue;
                                    }

                                    // Validate protocol version
                                    if packet.version != PROTOCOL_VERSION {
                                        debug!(
                                            version = packet.version,
                                            expected = PROTOCOL_VERSION,
                                            "Ignoring packet with different protocol version"
                                        );
                                        continue;
                                    }

                                    let discovered_at_ms = time.current_timestamp_ms().await;

                                    let peer = DiscoveredPeer {
                                        authority_id: packet.authority_id,
                                        descriptor: packet.descriptor,
                                        source_addr: src_addr,
                                        discovered_at_ms,
                                    };

                                    info!(
                                        authority = %peer.authority_id,
                                        addr = %src_addr,
                                        "LAN peer discovered"
                                    );

                                    on_discovered(peer);
                                } else {
                                    trace!(
                                        addr = %src_addr,
                                        len = len,
                                        "Received non-Aura packet"
                                    );
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Error receiving LAN packet");
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN listener shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Signal shutdown
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// =============================================================================
// LAN DISCOVERY SERVICE
// =============================================================================

/// Combined LAN discovery service (announcer + listener)
pub struct LanDiscoveryService {
    config: LanDiscoveryConfig,
    authority_id: AuthorityId,
    socket: Arc<UdpSocket>,
    descriptor: Arc<RwLock<Option<RendezvousDescriptor>>>,
    time: Arc<dyn TimeEffects>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl LanDiscoveryService {
    /// Create a new LAN discovery service
    pub async fn new(
        config: LanDiscoveryConfig,
        authority_id: AuthorityId,
        time: Arc<dyn TimeEffects>,
    ) -> std::io::Result<Self> {
        let bind_addr = SocketAddrV4::new(config.bind_addr, config.port);
        let socket = UdpSocket::bind(bind_addr).await?;
        socket.set_broadcast(true)?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            config,
            authority_id,
            socket: Arc::new(socket),
            descriptor: Arc::new(RwLock::new(None)),
            time,
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Set the descriptor to announce
    pub async fn set_descriptor(&self, descriptor: RendezvousDescriptor) {
        let mut guard = self.descriptor.write().await;
        *guard = Some(descriptor);
    }

    /// Clear the descriptor
    pub async fn clear_descriptor(&self) {
        let mut guard = self.descriptor.write().await;
        *guard = None;
    }

    /// Get a reference to the UDP socket for direct sending
    pub fn socket(&self) -> &Arc<UdpSocket> {
        &self.socket
    }

    /// Start both announcer and listener
    ///
    /// Returns handles for both tasks
    pub fn start<F>(
        &self,
        on_discovered: F,
    ) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        let announcer_handle = self.start_announcer();
        let listener_handle = self.start_listener(on_discovered);
        (announcer_handle, listener_handle)
    }

    /// Start only the announcer
    fn start_announcer(&self) -> tokio::task::JoinHandle<()> {
        let socket = self.socket.clone();
        let authority_id = self.authority_id;
        let descriptor = self.descriptor.clone();
        let interval_ms = self.config.announce_interval_ms;
        let time = self.time.clone();
        let broadcast_addr = SocketAddrV4::new(self.config.broadcast_addr, self.config.port);
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_millis(interval_ms));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let desc_guard = descriptor.read().await;
                        if let Some(desc) = desc_guard.as_ref() {
                            let timestamp_ms = time.current_timestamp_ms().await;

                            let packet = LanDiscoveryPacket::new(
                                authority_id,
                                desc.clone(),
                                timestamp_ms,
                            );

                            if let Some(bytes) = packet.to_bytes() {
                                if bytes.len() <= MAX_PACKET_SIZE {
                                    match socket.send_to(&bytes, broadcast_addr).await {
                                        Ok(n) => trace!(
                                            authority = %authority_id,
                                            bytes = n,
                                            "LAN announcement sent"
                                        ),
                                        Err(e) => warn!(
                                            error = %e,
                                            "Failed to send LAN announcement"
                                        ),
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN announcer shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Start only the listener
    fn start_listener<F>(&self, on_discovered: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        let socket = self.socket.clone();
        let local_authority = self.authority_id;
        let time = self.time.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();
        let on_discovered = Arc::new(on_discovered);

        tokio::spawn(async move {
            let mut buf = vec![0u8; MAX_PACKET_SIZE];

            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, src_addr)) => {
                                if let Some(packet) = LanDiscoveryPacket::from_bytes(&buf[..len]) {
                                    if packet.authority_id == local_authority {
                                        continue;
                                    }

                                    if packet.version != PROTOCOL_VERSION {
                                        continue;
                                    }

                                    let discovered_at_ms = time.current_timestamp_ms().await;

                                    let peer = DiscoveredPeer {
                                        authority_id: packet.authority_id,
                                        descriptor: packet.descriptor,
                                        source_addr: src_addr,
                                        discovered_at_ms,
                                    };

                                    info!(
                                        authority = %peer.authority_id,
                                        addr = %src_addr,
                                        "LAN peer discovered"
                                    );

                                    on_discovered(peer);
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Error receiving LAN packet");
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN listener shutting down");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Signal shutdown for both announcer and listener
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Create a transport hint for the LAN discovery source address
pub fn lan_transport_hint(addr: SocketAddr) -> TransportHint {
    TransportHint::TcpDirect {
        addr: addr.to_string(),
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::identifiers::ContextId;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Minimal mock time effects for testing without instantiating production handlers
    #[derive(Debug, Default)]
    struct MockTime {
        now_ms: AtomicU64,
    }

    impl MockTime {
        fn new(ts_ms: u64) -> Self {
            Self {
                now_ms: AtomicU64::new(ts_ms),
            }
        }
    }

    #[async_trait]
    impl aura_core::effects::time::PhysicalTimeEffects for MockTime {
        async fn physical_time(
            &self,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::time::TimeError> {
            Ok(aura_core::time::PhysicalTime {
                ts_ms: self.now_ms.load(Ordering::Relaxed),
                uncertainty: None,
            })
        }

        async fn sleep_ms(&self, _ms: u64) -> Result<(), aura_core::effects::time::TimeError> {
            // MockTime doesn't actually sleep - just returns immediately
            Ok(())
        }
    }

    #[async_trait]
    impl aura_core::effects::time::TimeEffects for MockTime {}

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([99u8; 32])
    }

    fn test_descriptor(authority_id: AuthorityId) -> RendezvousDescriptor {
        RendezvousDescriptor {
            authority_id,
            context_id: test_context(),
            transport_hints: vec![TransportHint::TcpDirect {
                addr: "127.0.0.1:8080".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [42u8; 32],
            display_name: None,
        }
    }

    #[test]
    fn test_packet_serialization() {
        let authority = test_authority(1);
        let descriptor = test_descriptor(authority);

        let packet = LanDiscoveryPacket::new(authority, descriptor.clone(), 12345);
        let bytes = packet
            .to_bytes()
            .unwrap_or_else(|| panic!("serialization should succeed"));

        assert!(bytes.starts_with(MAGIC_BYTES));
        assert!(bytes.len() < MAX_PACKET_SIZE);

        let restored = LanDiscoveryPacket::from_bytes(&bytes)
            .unwrap_or_else(|| panic!("deserialization should succeed"));
        assert_eq!(restored.authority_id, authority);
        assert_eq!(restored.version, PROTOCOL_VERSION);
        assert_eq!(restored.timestamp_ms, 12345);
    }

    #[test]
    fn test_packet_rejects_invalid_magic() {
        let mut bytes = vec![0u8; 100];
        bytes[0..4].copy_from_slice(b"XXXX");

        let result = LanDiscoveryPacket::from_bytes(&bytes);
        assert!(result.is_none());
    }

    #[test]
    fn test_packet_rejects_short_bytes() {
        let bytes = vec![0u8; 2];
        let result = LanDiscoveryPacket::from_bytes(&bytes);
        assert!(result.is_none());
    }

    #[test]
    fn test_config_default() {
        let config = LanDiscoveryConfig::default();
        assert_eq!(config.port, DEFAULT_LAN_PORT);
        assert_eq!(config.announce_interval_ms, DEFAULT_ANNOUNCE_INTERVAL_MS);
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_announcer_creation() {
        let config = LanDiscoveryConfig {
            port: 0, // Let OS assign port
            ..Default::default()
        };
        let authority = test_authority(10);
        let time: Arc<dyn TimeEffects> = Arc::new(MockTime::new(0));

        let result = LanAnnouncer::new(&config, authority, time).await;
        // May fail if broadcast not supported, that's okay for this test
        if let Ok(announcer) = result {
            announcer.stop();
        }
    }

    #[tokio::test]
    async fn test_service_creation() {
        let config = LanDiscoveryConfig {
            port: 0,
            ..Default::default()
        };
        let authority = test_authority(20);
        let time: Arc<dyn TimeEffects> = Arc::new(MockTime::new(0));

        let result = LanDiscoveryService::new(config, authority, time).await;
        if let Ok(service) = result {
            let descriptor = test_descriptor(authority);
            service.set_descriptor(descriptor).await;
            service.stop();
        }
    }
}

//! LAN Discovery (Layer 6 runtime)
//!
//! UDP broadcast/listen is platform/runtime-specific, so the implementation is provided
//! via `UdpEffects` (Layer 3) and wired by the runtime. The packet/config types live in
//! `aura-rendezvous` as pure data.

use aura_core::effects::network::{UdpEffects, UdpEndpoint, UdpEndpointEffects};
use aura_core::effects::time::{PhysicalTimeEffects, TimeError};
use aura_core::identifiers::AuthorityId;
use aura_rendezvous::{
    DiscoveredPeer, LanDiscoveryConfig, LanDiscoveryPacket, RendezvousDescriptor,
};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::sync::{watch, RwLock};
use tracing::{debug, error, info, trace, warn};

/// LAN discovery service combining announcer and listener tasks.
pub struct LanDiscoveryService {
    config: LanDiscoveryConfig,
    authority_id: AuthorityId,
    time: Arc<dyn PhysicalTimeEffects>,
    socket: Arc<dyn UdpEndpointEffects>,
    descriptor: Arc<RwLock<Option<RendezvousDescriptor>>>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
}

impl LanDiscoveryService {
    /// Create a new LAN discovery service.
    pub async fn new(
        config: LanDiscoveryConfig,
        authority_id: AuthorityId,
        udp: Arc<dyn UdpEffects>,
        time: Arc<dyn PhysicalTimeEffects>,
    ) -> Result<Self, String> {
        let bind_ip: Ipv4Addr = config
            .bind_addr
            .parse()
            .map_err(|e| format!("Invalid bind_addr '{}': {e}", config.bind_addr))?;
        let broadcast_ip: Ipv4Addr = config
            .broadcast_addr
            .parse()
            .map_err(|e| format!("Invalid broadcast_addr '{}': {e}", config.broadcast_addr))?;

        let bind_addr = UdpEndpoint::new(SocketAddrV4::new(bind_ip, config.port).to_string());
        let socket = udp
            .udp_bind(bind_addr.clone())
            .await
            .map_err(|e| format!("UDP bind failed ({bind_addr}): {e}"))?;
        socket
            .set_broadcast(true)
            .await
            .map_err(|e| format!("set_broadcast failed: {e}"))?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Validate broadcast addr early for better error messages (used at send time).
        let _ = SocketAddrV4::new(broadcast_ip, config.port);

        Ok(Self {
            config,
            authority_id,
            time,
            socket,
            descriptor: Arc::new(RwLock::new(None)),
            shutdown_tx,
            shutdown_rx,
        })
    }

    /// Start announcer + listener tasks.
    pub fn start<F>(
        &self,
        on_discovered: F,
    ) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>)
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        let announcer = self.start_announcer();
        let listener = self.start_listener(on_discovered);
        (announcer, listener)
    }

    /// Signal shutdown.
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    /// Set the descriptor to announce.
    pub async fn set_descriptor(&self, descriptor: RendezvousDescriptor) {
        *self.descriptor.write().await = Some(descriptor);
    }

    /// Clear the descriptor (stop announcing).
    #[allow(dead_code)]
    pub async fn clear_descriptor(&self) {
        *self.descriptor.write().await = None;
    }

    /// Expose the underlying UDP socket (used for ad-hoc LAN invitation sends).
    pub fn socket(&self) -> &Arc<dyn UdpEndpointEffects> {
        &self.socket
    }

    fn start_announcer(&self) -> tokio::task::JoinHandle<()> {
        let socket = self.socket.clone();
        let authority_id = self.authority_id;
        let descriptor = self.descriptor.clone();
        let interval_ms = self.config.announce_interval_ms;
        let time = self.time.clone();
        let broadcast_ip: Ipv4Addr = self
            .config
            .broadcast_addr
            .parse()
            .unwrap_or(Ipv4Addr::BROADCAST);
        let broadcast_addr = UdpEndpoint::new(SocketAddrV4::new(broadcast_ip, self.config.port).to_string());
        let mut shutdown_rx = self.shutdown_rx.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN announcer shutting down");
                            break;
                        }
                    }
                    sleep_res = time.sleep_ms(interval_ms) => {
                        if let Err(TimeError::Timeout { .. }) = sleep_res {
                            // Unlikely for sleep_ms, but treat as retryable.
                            continue;
                        }

                        let desc_guard = descriptor.read().await;
                        let Some(desc) = desc_guard.as_ref() else {
                            continue;
                        };

                        let timestamp_ms = match time.physical_time().await {
                            Ok(t) => t.ts_ms,
                            Err(err) => {
                                warn!(error = %err, "LAN announcer: failed to read physical time");
                                continue;
                            }
                        };

                        let packet = LanDiscoveryPacket::new(authority_id, desc.clone(), timestamp_ms);
                        let Some(bytes) = packet.to_bytes() else {
                            warn!("LAN announcer: failed to serialize packet");
                            continue;
                        };
                        if bytes.len() > aura_rendezvous::MAX_PACKET_SIZE {
                            warn!(size = bytes.len(), "LAN announcer: packet too large");
                            continue;
                        }

                        match socket.send_to(&bytes, &broadcast_addr).await {
                            Ok(n) => trace!(authority = %authority_id, bytes = n, "LAN announcement sent"),
                            Err(e) => warn!(error = %e, "Failed to send LAN announcement"),
                        }
                    }
                }
            }
        })
    }

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
            let mut buf = vec![0u8; aura_rendezvous::MAX_PACKET_SIZE];

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("LAN listener shutting down");
                            break;
                        }
                    }
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((len, src_addr)) => {
                                let Some(packet) = LanDiscoveryPacket::from_bytes(&buf[..len]) else {
                                    trace!(addr = %src_addr, len = len, "Received non-Aura LAN packet");
                                    continue;
                                };

                                if packet.authority_id == local_authority {
                                    continue;
                                }

                                let discovered_at_ms = match time.physical_time().await {
                                    Ok(t) => t.ts_ms,
                                    Err(err) => {
                                        debug!(error = %err, "LAN listener: failed to read physical time");
                                        0
                                    }
                                };

                                let peer = DiscoveredPeer::new(
                                    packet.authority_id,
                                    packet.descriptor,
                                    src_addr.to_string(),
                                    discovered_at_ms,
                                );

                                info!(authority = %peer.authority_id, addr = %peer.source_addr, discovered_at_ms = discovered_at_ms, "LAN peer discovered");
                                on_discovered(peer);
                            }
                            Err(e) => {
                                error!(error = %e, "Error receiving LAN packet");
                            }
                        }
                    }
                }
            }
        })
    }
}

//! LAN Discovery (Layer 6 runtime)
//!
//! UDP broadcast/listen is platform/runtime-specific, so the implementation is provided
//! via `UdpEffects` (Layer 3) and wired by the runtime. The packet/config types live in
//! `aura-rendezvous` as pure data.

use crate::runtime::TaskGroup;
use aura_core::effects::network::{UdpEffects, UdpEndpoint, UdpEndpointEffects};
use aura_core::effects::time::{PhysicalTimeEffects, TimeError};
use aura_core::types::identifiers::AuthorityId;
use aura_rendezvous::{
    DiscoveredPeer, LanDiscoveryConfig, LanDiscoveryPacket, RendezvousDescriptor,
};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info, trace, warn};

struct LanDiscoveryShared {
    state: Mutex<LanDiscoveryState>,
    metrics: Mutex<LanDiscoveryMetrics>,
}

/// LAN discovery service combining announcer and listener tasks.
pub struct LanDiscoveryService {
    config: LanDiscoveryConfig,
    authority_id: AuthorityId,
    time: Arc<dyn PhysicalTimeEffects>,
    socket: Arc<dyn UdpEndpointEffects>,
    shared: Arc<LanDiscoveryShared>,
}

#[derive(Debug, Default)]
struct LanDiscoveryState {
    descriptor: Option<RendezvousDescriptor>,
}

/// Runtime metrics for LAN discovery.
#[derive(Debug, Default, Clone)]
pub struct LanDiscoveryMetrics {
    pub announcements_sent: u64,
    pub announcement_errors: u64,
    pub packets_received: u64,
    pub packets_invalid: u64,
    pub peers_discovered: u64,
    pub receive_errors: u64,
    pub last_announce_ms: u64,
    pub last_packet_ms: u64,
    pub last_discovered_ms: u64,
    pub last_error_ms: u64,
}

impl LanDiscoveryState {
    #[allow(dead_code)] // For use with with_state_mut_validated
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        Ok(())
    }
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

        // Validate broadcast addr early for better error messages (used at send time).
        let _ = SocketAddrV4::new(broadcast_ip, config.port);

        Ok(Self {
            config,
            authority_id,
            time,
            socket,
            shared: Arc::new(LanDiscoveryShared {
                state: Mutex::new(LanDiscoveryState::default()),
                metrics: Mutex::new(LanDiscoveryMetrics::default()),
            }),
        })
    }

    async fn with_state_mut<R>(&self, op: impl FnOnce(&mut LanDiscoveryState) -> R) -> R {
        let mut guard = self.shared.state.lock().await;
        let result = op(&mut guard);
        #[cfg(debug_assertions)]
        {
            if let Err(violation) = guard.validate() {
                tracing::error!(
                    component = violation.component,
                    description = %violation.description,
                    "LanDiscoveryService state invariant violated"
                );
                debug_assert!(
                    false,
                    "LanDiscoveryService invariant violated: {}",
                    violation
                );
            }
        }
        result
    }

    /// Start announcer + listener tasks under an owning task group.
    pub fn start<F>(&self, tasks: TaskGroup, on_discovered: F)
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        self.start_announcer(tasks.group("announcer"));
        self.start_listener(tasks.group("listener"), on_discovered);
    }

    /// Set the descriptor to announce.
    pub async fn set_descriptor(&self, descriptor: RendezvousDescriptor) {
        self.with_state_mut(|state| {
            state.descriptor = Some(descriptor);
        })
        .await;
    }

    /// Clear the descriptor (stop announcing).
    #[allow(dead_code)]
    pub async fn clear_descriptor(&self) {
        self.with_state_mut(|state| {
            state.descriptor = None;
        })
        .await;
    }

    /// Expose the underlying UDP socket (used for ad-hoc LAN invitation sends).
    pub fn socket(&self) -> &Arc<dyn UdpEndpointEffects> {
        &self.socket
    }

    /// Get a snapshot of LAN discovery metrics.
    pub async fn metrics(&self) -> LanDiscoveryMetrics {
        self.shared.metrics.lock().await.clone()
    }

    fn start_announcer(&self, tasks: TaskGroup) {
        let socket = self.socket.clone();
        let authority_id = self.authority_id;
        let shared = Arc::clone(&self.shared);
        let interval_ms = self.config.announce_interval_ms;
        let time = self.time.clone();
        let broadcast_ip: Ipv4Addr = self
            .config
            .broadcast_addr
            .parse()
            .unwrap_or(Ipv4Addr::BROADCAST);
        let broadcast_addr =
            UdpEndpoint::new(SocketAddrV4::new(broadcast_ip, self.config.port).to_string());
        let fut = async move {
            loop {
                if let Err(TimeError::Timeout { .. }) = time.sleep_ms(interval_ms).await {
                    continue;
                }

                let descriptor = {
                    let guard = shared.state.lock().await;
                    guard.descriptor.clone()
                };
                let Some(desc) = descriptor.as_ref() else {
                    continue;
                };

                let timestamp_ms = match time.physical_time().await {
                    Ok(t) => t.ts_ms,
                    Err(err) => {
                        warn!(error = %err, "LAN announcer: failed to read physical time");
                        let mut metrics = shared.metrics.lock().await;
                        metrics.announcement_errors = metrics.announcement_errors.saturating_add(1);
                        continue;
                    }
                };

                let packet = LanDiscoveryPacket::new(authority_id, desc.clone(), timestamp_ms);
                let Some(bytes) = packet.to_bytes() else {
                    warn!("LAN announcer: failed to serialize packet");
                    let mut metrics = shared.metrics.lock().await;
                    metrics.announcement_errors = metrics.announcement_errors.saturating_add(1);
                    metrics.last_error_ms = timestamp_ms;
                    continue;
                };
                if bytes.len() > aura_rendezvous::MAX_PACKET_SIZE {
                    warn!(size = bytes.len(), "LAN announcer: packet too large");
                    let mut metrics = shared.metrics.lock().await;
                    metrics.announcement_errors = metrics.announcement_errors.saturating_add(1);
                    metrics.last_error_ms = timestamp_ms;
                    continue;
                }

                match socket.send_to(&bytes, &broadcast_addr).await {
                    Ok(n) => {
                        trace!(authority = %authority_id, bytes = n, "LAN announcement sent");
                        let mut metrics = shared.metrics.lock().await;
                        metrics.announcements_sent = metrics.announcements_sent.saturating_add(1);
                        metrics.last_announce_ms = timestamp_ms;
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to send LAN announcement");
                        let mut metrics = shared.metrics.lock().await;
                        metrics.announcement_errors = metrics.announcement_errors.saturating_add(1);
                        metrics.last_error_ms = timestamp_ms;
                    }
                }
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                tasks.spawn_local_named("run", fut)
            } else {
                tasks.spawn_named("run", fut)
            }
        }
    }

    fn start_listener<F>(&self, tasks: TaskGroup, on_discovered: F)
    where
        F: Fn(DiscoveredPeer) + Send + Sync + 'static,
    {
        let socket = self.socket.clone();
        let local_authority = self.authority_id;
        let time = self.time.clone();
        let shared = Arc::clone(&self.shared);
        let on_discovered = Arc::new(on_discovered);

        let fut = async move {
            let mut buf = vec![0u8; aura_rendezvous::MAX_PACKET_SIZE];

            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((len, src_addr)) => {
                        let received_at_ms = match time.physical_time().await {
                            Ok(t) => t.ts_ms,
                            Err(err) => {
                                debug!(error = %err, "LAN listener: failed to read physical time");
                                0
                            }
                        };
                        {
                            let mut metrics = shared.metrics.lock().await;
                            metrics.packets_received = metrics.packets_received.saturating_add(1);
                            if received_at_ms > 0 {
                                metrics.last_packet_ms = received_at_ms;
                            }
                        }

                        let Some(packet) = LanDiscoveryPacket::from_bytes(&buf[..len]) else {
                            trace!(addr = %src_addr, len = len, "Received non-Aura LAN packet");
                            let mut metrics = shared.metrics.lock().await;
                            metrics.packets_invalid = metrics.packets_invalid.saturating_add(1);
                            if received_at_ms > 0 {
                                metrics.last_error_ms = received_at_ms;
                            }
                            continue;
                        };

                        if packet.authority_id == local_authority {
                            continue;
                        }

                        let discovered_at_ms = received_at_ms;

                        let peer = DiscoveredPeer::new(
                            packet.authority_id,
                            packet.descriptor,
                            src_addr.to_string(),
                            discovered_at_ms,
                        );

                        info!(authority = %peer.authority_id, addr = %peer.source_addr, discovered_at_ms = discovered_at_ms, "LAN peer discovered");
                        {
                            let mut metrics = shared.metrics.lock().await;
                            metrics.peers_discovered = metrics.peers_discovered.saturating_add(1);
                            if discovered_at_ms > 0 {
                                metrics.last_discovered_ms = discovered_at_ms;
                            }
                        }
                        on_discovered(peer);
                    }
                    Err(e) => {
                        error!(error = %e, "Error receiving LAN packet");
                        let now_ms = match time.physical_time().await {
                            Ok(t) => t.ts_ms,
                            Err(_) => 0,
                        };
                        let mut metrics = shared.metrics.lock().await;
                        metrics.receive_errors = metrics.receive_errors.saturating_add(1);
                        if now_ms > 0 {
                            metrics.last_error_ms = now_ms;
                        }
                    }
                }
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                tasks.spawn_local_named("run", fut)
            } else {
                tasks.spawn_named("run", fut)
            }
        }
    }
}

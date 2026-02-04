//! LAN Transport Service (Layer 6 runtime)
//!
//! Binds a TCP listener for LAN connections and derives advertised addresses
//! for rendezvous descriptors.

use std::net::IpAddr;

use get_if_addrs::{get_if_addrs, IfAddr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::info;

/// LAN transport service holding the listener and advertised addresses.
#[derive(Debug)]
pub struct LanTransportService {
    listener: Arc<TcpListener>,
    advertised_addrs: Vec<String>,
    metrics: Arc<RwLock<LanTransportMetrics>>,
}

/// Runtime metrics for LAN transport.
#[derive(Debug, Default, Clone)]
pub struct LanTransportMetrics {
    pub connections_accepted: u64,
    pub accept_errors: u64,
    pub frames_received: u64,
    pub bytes_received: u64,
    pub read_errors: u64,
    pub decode_errors: u64,
    pub last_accept_ms: u64,
    pub last_frame_ms: u64,
    pub last_error_ms: u64,
}

impl LanTransportService {
    /// Bind a TCP listener and derive advertised LAN addresses.
    pub async fn bind(bind_addr: &str) -> Result<Self, String> {
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|e| format!("LAN transport bind failed ({bind_addr}): {e}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to read LAN transport local addr: {e}"))?;
        let port = local_addr.port();

        let mut advertised_addrs = Vec::new();
        let mut loopback_addrs = Vec::new();
        match get_if_addrs() {
            Ok(ifaces) => {
                for iface in ifaces {
                    let addr = match iface.addr {
                        IfAddr::V4(v4) => IpAddr::V4(v4.ip),
                        IfAddr::V6(v6) => IpAddr::V6(v6.ip),
                    };
                    if is_advertisable_ip(addr) {
                        advertised_addrs.push(format!("{addr}:{port}"));
                        continue;
                    }
                    if addr.is_loopback() {
                        loopback_addrs.push(format!("{addr}:{port}"));
                        continue;
                    }
                }
            }
            Err(err) => {
                tracing::warn!(error = %err, "Failed to enumerate interfaces for LAN advertise");
            }
        }

        if advertised_addrs.is_empty() {
            if !loopback_addrs.is_empty() {
                advertised_addrs.extend(loopback_addrs);
            } else if local_addr.ip().is_unspecified() {
                advertised_addrs.push(format!("127.0.0.1:{port}"));
            } else {
                // Fallback to the listener address (may be 0.0.0.0). Better than nothing.
                advertised_addrs.push(local_addr.to_string());
            }
        }

        info!(
            component = "lan_transport",
            bind_addr = %bind_addr,
            advertised_count = advertised_addrs.len(),
            "LAN transport listener bound"
        );

        Ok(Self {
            listener: Arc::new(listener),
            advertised_addrs,
            metrics: Arc::new(RwLock::new(LanTransportMetrics::default())),
        })
    }

    /// Get advertised addresses for rendezvous descriptors.
    pub fn advertised_addrs(&self) -> &[String] {
        &self.advertised_addrs
    }

    /// Get a snapshot of LAN transport metrics.
    pub async fn metrics(&self) -> LanTransportMetrics {
        self.metrics.read().await.clone()
    }

    /// Access the metrics handle for live updates.
    pub fn metrics_handle(&self) -> Arc<RwLock<LanTransportMetrics>> {
        self.metrics.clone()
    }

    /// Access the underlying listener.
    pub fn listener(&self) -> Arc<TcpListener> {
        self.listener.clone()
    }
}

fn is_advertisable_ip(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_multicast()
                && !v4.is_unspecified()
                && !v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            !v6.is_loopback()
                && !v6.is_multicast()
                && !v6.is_unspecified()
                && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

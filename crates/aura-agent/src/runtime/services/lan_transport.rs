//! LAN Transport Service (Layer 6 runtime)
//!
//! Binds a TCP listener for LAN connections and derives advertised addresses
//! for rendezvous descriptors.

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use std::sync::Arc;
        use tokio::sync::RwLock;

        const HARNESS_INSTANCE_QUERY_KEY: &str = "__aura_harness_instance";

        fn harness_browser_transport_addr() -> Option<String> {
            let window = web_sys::window()?;
            let search = window.location().search().ok()?;
            let query = search.strip_prefix('?').unwrap_or(&search);
            let harness_mode = query.split('&').any(|pair: &str| {
                pair.split_once('=')
                    .is_some_and(|(key, value)| key == HARNESS_INSTANCE_QUERY_KEY && !value.is_empty())
            });
            if !harness_mode {
                return None;
            }

            let host = window.location().host().ok()?;
            if host.is_empty() {
                return None;
            }
            Some(host)
        }

        #[derive(Debug)]
        struct LanTransportShared {
            metrics: Arc<RwLock<LanTransportMetrics>>,
        }

        /// LAN transport service placeholder for wasm builds.
        #[derive(Debug)]
        #[aura_macros::actor_root(
            owner = "lan_transport_service",
            domain = "lan_transport",
            supervision = "lan_transport_task_root",
            category = "actor_owned"
        )]
        pub struct LanTransportService {
            advertised_addrs: Vec<String>,
            websocket_addrs: Vec<String>,
            shared: Arc<LanTransportShared>,
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
            /// Bind LAN transport.
            ///
            /// WASM runtimes cannot bind TCP listeners directly, so this returns an empty
            /// transport placeholder.
            pub async fn bind(_bind_addr: &str) -> Result<Self, String> {
                let websocket_addrs = harness_browser_transport_addr()
                    .into_iter()
                    .collect::<Vec<_>>();
                Ok(Self {
                    advertised_addrs: Vec::new(),
                    websocket_addrs,
                    shared: Arc::new(LanTransportShared {
                        metrics: Arc::new(RwLock::new(LanTransportMetrics::default())),
                    }),
                })
            }

            /// Get advertised addresses for rendezvous descriptors.
            pub fn advertised_addrs(&self) -> &[String] {
                &self.advertised_addrs
            }

            /// Get advertised WebSocket addresses for browser runtimes.
            pub fn websocket_addrs(&self) -> &[String] {
                &self.websocket_addrs
            }

            /// Get a snapshot of LAN transport metrics.
            pub async fn metrics(&self) -> LanTransportMetrics {
                self.shared.metrics.read().await.clone()
            }
        }
    } else {
        use std::net::{IpAddr, SocketAddr};
        use std::sync::Arc;

        use get_if_addrs::{get_if_addrs, IfAddr};
        use tokio::net::TcpListener;
        use tokio::sync::RwLock;
        use tracing::info;

        #[derive(Debug)]
        struct LanTransportShared {
            metrics: Arc<RwLock<LanTransportMetrics>>,
        }

        /// LAN transport service holding the listener and advertised addresses.
        #[derive(Debug)]
        #[aura_macros::actor_root(
            owner = "lan_transport_service",
            domain = "lan_transport",
            supervision = "lan_transport_task_root",
            category = "actor_owned"
        )]
        pub struct LanTransportService {
            listener: Arc<TcpListener>,
            advertised_addrs: Vec<String>,
            websocket_listener: Arc<TcpListener>,
            websocket_addrs: Vec<String>,
            shared: Arc<LanTransportShared>,
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
                let local_ip = local_addr.ip();
                if !local_ip.is_unspecified() {
                    // If bound to a specific interface, advertise only that exact listener address.
                    advertised_addrs.push(local_addr.to_string());
                } else {
                    let mut loopback_addrs = Vec::new();
                    match get_if_addrs() {
                        Ok(ifaces) => {
                            for iface in ifaces {
                                let addr = match iface.addr {
                                    IfAddr::V4(v4) => IpAddr::V4(v4.ip),
                                    IfAddr::V6(v6) => IpAddr::V6(v6.ip),
                                };
                                if is_advertisable_ip(addr) {
                                    advertised_addrs.push(format_transport_addr(addr, port));
                                    continue;
                                }
                                if addr.is_loopback() {
                                    loopback_addrs.push(format_transport_addr(addr, port));
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
                        } else {
                            advertised_addrs.push(format!("127.0.0.1:{port}"));
                        }
                    }
                }

                let websocket_bind_addr = std::net::SocketAddr::new(local_addr.ip(), 0);
                let websocket_listener = TcpListener::bind(websocket_bind_addr)
                    .await
                    .map_err(|e| format!("LAN websocket bind failed ({websocket_bind_addr}): {e}"))?;
                let websocket_local_addr = websocket_listener
                    .local_addr()
                    .map_err(|e| format!("Failed to read LAN websocket local addr: {e}"))?;
                let websocket_port = websocket_local_addr.port();
                let mut websocket_addrs = Vec::new();
                if !local_ip.is_unspecified() {
                    websocket_addrs.push(websocket_local_addr.to_string());
                } else {
                    let mut loopback_ws_addrs = Vec::new();
                    match get_if_addrs() {
                        Ok(ifaces) => {
                            for iface in ifaces {
                                let addr = match iface.addr {
                                    IfAddr::V4(v4) => IpAddr::V4(v4.ip),
                                    IfAddr::V6(v6) => IpAddr::V6(v6.ip),
                                };
                                if is_advertisable_ip(addr) {
                                    websocket_addrs
                                        .push(format_transport_addr(addr, websocket_port));
                                    continue;
                                }
                                if addr.is_loopback() {
                                    loopback_ws_addrs
                                        .push(format_transport_addr(addr, websocket_port));
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                "Failed to enumerate interfaces for LAN websocket advertise"
                            );
                        }
                    }

                    if websocket_addrs.is_empty() {
                        if !loopback_ws_addrs.is_empty() {
                            websocket_addrs.extend(loopback_ws_addrs);
                        } else {
                            websocket_addrs.push(format!("127.0.0.1:{websocket_port}"));
                        }
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
                    websocket_listener: Arc::new(websocket_listener),
                    websocket_addrs,
                    shared: Arc::new(LanTransportShared {
                        metrics: Arc::new(RwLock::new(LanTransportMetrics::default())),
                    }),
                })
            }

            /// Get advertised addresses for rendezvous descriptors.
            pub fn advertised_addrs(&self) -> &[String] {
                &self.advertised_addrs
            }

            /// Get advertised WebSocket addresses for browser runtimes.
            pub fn websocket_addrs(&self) -> &[String] {
                &self.websocket_addrs
            }

            /// Get a snapshot of LAN transport metrics.
            pub async fn metrics(&self) -> LanTransportMetrics {
                self.shared.metrics.read().await.clone()
            }

            /// Access the metrics handle for live updates.
            pub fn metrics_handle(&self) -> Arc<RwLock<LanTransportMetrics>> {
                Arc::clone(&self.shared.metrics)
            }

            /// Access the underlying listener.
            pub fn listener(&self) -> Arc<TcpListener> {
                self.listener.clone()
            }

            /// Access the WebSocket listener.
            pub fn websocket_listener(&self) -> Arc<TcpListener> {
                self.websocket_listener.clone()
            }
        }

        fn format_transport_addr(addr: IpAddr, port: u16) -> String {
            SocketAddr::new(addr, port).to_string()
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

        #[cfg(test)]
        mod tests {
            use super::format_transport_addr;
            use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

            #[test]
            fn format_transport_addr_preserves_ipv4_shape() {
                let addr = format_transport_addr(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 4242);
                assert_eq!(addr, "127.0.0.1:4242");
            }

            #[test]
            fn format_transport_addr_brackets_ipv6_hosts() {
                let addr = format_transport_addr(IpAddr::V6(Ipv6Addr::LOCALHOST), 4242);
                assert_eq!(addr, "[::1]:4242");
            }
        }
    }
}

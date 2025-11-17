//! Transport Utilities
//!
//! Essential transport utilities using mature libraries.
//! Target: <200 lines, focus on std/tokio ecosystem.

use super::{TransportError, TransportResult};
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

/// Address resolution utilities
pub struct AddressResolver;

impl AddressResolver {
    /// Resolve hostname to socket addresses
    pub async fn resolve(host: &str, port: u16) -> TransportResult<Vec<SocketAddr>> {
        let addresses: Vec<_> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|e| TransportError::ConnectionFailed(format!("DNS resolution failed: {}", e)))?
            .collect();

        if addresses.is_empty() {
            return Err(TransportError::ConnectionFailed(format!(
                "No addresses found for {}",
                host
            )));
        }

        Ok(addresses)
    }

    /// Parse URL to socket address
    pub fn url_to_socket_addr(url: &Url) -> TransportResult<SocketAddr> {
        let host = url
            .host_str()
            .ok_or_else(|| TransportError::Protocol("Missing host in URL".to_string()))?;

        let port = url
            .port_or_known_default()
            .ok_or_else(|| TransportError::Protocol("Missing port in URL".to_string()))?;

        // Try parsing as IP address first
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(SocketAddr::new(ip, port));
        }

        // For hostnames, we'd need async resolution
        Err(TransportError::Protocol(format!(
            "Hostname resolution not supported in synchronous context: {}",
            host
        )))
    }

    /// Check if address is local/loopback
    pub fn is_local_address(addr: &SocketAddr) -> bool {
        match addr.ip() {
            IpAddr::V4(ipv4) => ipv4.is_loopback() || ipv4.is_private(),
            IpAddr::V6(ipv6) => ipv6.is_loopback(),
        }
    }
}

/// Connection timeout utilities
pub struct TimeoutHelper;

impl TimeoutHelper {
    /// Apply timeout to async operation
    pub async fn with_timeout<F, T>(
        duration: Duration,
        operation: F,
        operation_name: &str,
    ) -> TransportResult<T>
    where
        F: std::future::Future<Output = TransportResult<T>>,
    {
        timeout(duration, operation)
            .await
            .map_err(|_| TransportError::Timeout(format!("{} timeout", operation_name)))?
    }

    /// Create exponential backoff delay
    pub fn exponential_backoff(
        attempt: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Duration {
        let delay = base_delay * 2_u32.pow(attempt.min(10)); // Cap to prevent overflow
        delay.min(max_delay)
    }

    /// Add jitter to delay
    pub fn add_jitter(delay: Duration, jitter_percent: u8) -> Duration {
        if jitter_percent == 0 || jitter_percent > 100 {
            return delay;
        }

        let jitter_range = delay * jitter_percent as u32 / 100;
        let jitter_ms = fastrand::u64(0..jitter_range.as_millis() as u64);
        delay + Duration::from_millis(jitter_ms)
    }
}

/// Buffer management utilities
pub struct BufferUtils;

impl BufferUtils {
    /// Calculate optimal buffer size for transport
    pub fn optimal_buffer_size(connection_type: TransportType) -> usize {
        match connection_type {
            TransportType::Tcp => 64 * 1024,       // 64KB for TCP
            TransportType::WebSocket => 32 * 1024, // 32KB for WebSocket
            TransportType::Memory => 128 * 1024,   // 128KB for memory (no network overhead)
        }
    }

    /// Validate buffer size
    pub fn validate_buffer_size(size: usize, max_size: usize) -> TransportResult<usize> {
        if size == 0 {
            return Err(TransportError::Protocol(
                "Buffer size cannot be zero".to_string(),
            ));
        }

        if size > max_size {
            return Err(TransportError::Protocol(format!(
                "Buffer size {} exceeds maximum {}",
                size, max_size
            )));
        }

        Ok(size)
    }

    /// Split large message into chunks
    pub fn chunk_message(data: &[u8], chunk_size: usize) -> Vec<&[u8]> {
        if chunk_size == 0 {
            return vec![data];
        }

        data.chunks(chunk_size).collect()
    }
}

/// Transport type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    /// TCP transport
    Tcp,
    /// WebSocket transport
    WebSocket,
    /// In-memory transport (testing)
    Memory,
}

/// Connection state tracking
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Connecting in progress
    Connecting,
    /// Active connection
    Connected,
    /// Disconnection in progress
    Disconnecting,
    /// Connection error state
    Error(String),
}

/// Simple connection metrics
#[derive(Debug, Clone, Default)]
pub struct ConnectionMetrics {
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Number of messages sent
    pub messages_sent: u64,
    /// Number of messages received
    pub messages_received: u64,
    /// When the connection was established
    pub connection_time: Option<std::time::Instant>,
    /// Last activity timestamp
    pub last_activity: Option<std::time::Instant>,
}

impl ConnectionMetrics {
    /// Create new connection metrics
    pub fn new() -> Self {
        Self::default()
    }

    /// Record outgoing message
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn record_sent(&mut self, bytes: u64, now: std::time::Instant) {
        self.bytes_sent += bytes;
        self.messages_sent += 1;
        self.last_activity = Some(now);
    }

    /// Record incoming message
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn record_received(&mut self, bytes: u64, now: std::time::Instant) {
        self.bytes_received += bytes;
        self.messages_received += 1;
        self.last_activity = Some(now);
    }

    /// Mark connection as established
    ///
    /// Note: Callers should obtain `now` via `TimeEffects::now_instant()` and pass it to this method
    pub fn connected(&mut self, now: std::time::Instant) {
        self.connection_time = Some(now);
        self.last_activity = Some(now);
    }

    /// Get total connection duration
    pub fn connection_duration(&self) -> Option<Duration> {
        self.connection_time.map(|start| start.elapsed())
    }

    /// Get idle time since last activity
    pub fn idle_time(&self) -> Option<Duration> {
        self.last_activity.map(|last| last.elapsed())
    }
}

/// URL validation utilities
pub struct UrlValidator;

impl UrlValidator {
    /// Validate WebSocket URL
    pub fn validate_websocket_url(url: &Url) -> TransportResult<()> {
        match url.scheme() {
            "ws" | "wss" => {}
            other => {
                return Err(TransportError::Protocol(format!(
                    "Invalid WebSocket scheme: {}",
                    other
                )))
            }
        }

        if url.host().is_none() {
            return Err(TransportError::Protocol(
                "WebSocket URL missing host".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate TCP connection string
    pub fn validate_tcp_address(addr: &str) -> TransportResult<SocketAddr> {
        addr.parse::<SocketAddr>()
            .map_err(|e| TransportError::Protocol(format!("Invalid TCP address: {}", e)))
    }
}

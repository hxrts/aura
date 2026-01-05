//! Layer 3: Transport Effect Handlers - Production Only
//!
//! Stateless single-party implementations of NetworkEffects from aura-core (Layer 1).
//! Each handler implements a pure effect: accept input, perform transport operation, produce output.
//!
//! **Handler Types**:
//! - **TcpTransportHandler**: Real TCP sockets with configurable timeouts
//! - **WebSocketTransportHandler**: Browser-compatible WebSocket protocol
//! - **RealTransportHandler**: Production transport with TransportEffects implementation
//! - **FramingHandler**: Message delimiting (length-prefix framing for streams)
//! - **Utils**: Address validation, connection metrics, timeout management
//!
//! **Layer Constraint** (per docs/106_effect_system_and_runtime.md):
//! NO choreography or multi-party coordination here. Multi-party transport logic
//! (routing, retry, connection management) belongs in aura-agent/transport (Layer 6).
//! These implement pure infrastructure effects only.

// REMOVED: pub mod coordination; // Moved to aura-protocol (Layer 4)
// REMOVED: pub mod memory; // Moved to aura-testkit (Layer 8)
pub mod framing;
pub mod real;
pub mod tcp;
pub mod utils;
pub mod websocket;

// REMOVED: Re-exports moved to aura-protocol
// pub use coordination::{RetryingTransportManager, TransportCoordinationConfig,
//                        TransportCoordinationError, CoordinationResult};
pub use framing::FramingHandler;
// REMOVED: pub use memory::InMemoryTransportHandler; // Moved to aura-testkit
pub use real::RealTransportHandler;
pub use tcp::TcpTransportHandler;
pub use utils::{AddressResolver, BufferUtils, ConnectionMetrics, TimeoutHelper, UrlValidator};
pub use websocket::WebSocketTransportHandler;

use std::num::NonZeroUsize;

/// Non-zero duration wrapper to prevent zero-timeout configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonZeroDuration(std::time::Duration);

impl NonZeroDuration {
    /// Creates a non-zero duration from a standard duration.
    pub fn from_duration(duration: std::time::Duration) -> Option<Self> {
        if duration.is_zero() {
            None
        } else {
            Some(Self(duration))
        }
    }

    /// Creates a non-zero duration from seconds.
    pub fn from_secs(secs: u64) -> Option<Self> {
        if secs == 0 {
            None
        } else {
            Some(Self(std::time::Duration::from_secs(secs)))
        }
    }

    /// Creates a non-zero duration from milliseconds.
    pub fn from_millis(ms: u64) -> Option<Self> {
        if ms == 0 {
            None
        } else {
            Some(Self(std::time::Duration::from_millis(ms)))
        }
    }

    /// Returns the inner duration.
    pub fn get(self) -> std::time::Duration {
        self.0
    }
}

impl From<NonZeroDuration> for std::time::Duration {
    fn from(value: NonZeroDuration) -> Self {
        value.0
    }
}

/// Typed connection identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConnectionId(String);

impl ConnectionId {
    /// Creates a new connection identifier.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the connection ID as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for ConnectionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Typed socket address wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransportSocketAddr(std::net::SocketAddr);

impl TransportSocketAddr {
    /// Creates a new transport socket address.
    pub fn new(value: std::net::SocketAddr) -> Self {
        Self(value)
    }

    /// Returns a reference to the inner socket address.
    pub fn as_socket_addr(&self) -> &std::net::SocketAddr {
        &self.0
    }

    /// Consumes self and returns the inner socket address.
    pub fn into_inner(self) -> std::net::SocketAddr {
        self.0
    }
}

impl From<std::net::SocketAddr> for TransportSocketAddr {
    fn from(value: std::net::SocketAddr) -> Self {
        Self(value)
    }
}

impl From<TransportSocketAddr> for std::net::SocketAddr {
    fn from(value: TransportSocketAddr) -> Self {
        value.0
    }
}

impl std::fmt::Display for TransportSocketAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Typed URL wrapper for transport endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportUrl(url::Url);

impl TransportUrl {
    /// Creates a new transport URL wrapper.
    pub fn new(value: url::Url) -> Self {
        Self(value)
    }

    /// Returns a reference to the inner URL.
    pub fn as_url(&self) -> &url::Url {
        &self.0
    }

    /// Returns the URL as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Consumes self and returns the inner URL.
    pub fn into_inner(self) -> url::Url {
        self.0
    }
}

impl From<url::Url> for TransportUrl {
    fn from(value: url::Url) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for TransportUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.as_str())
    }
}

/// Typed transport address wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransportAddress(String);

impl TransportAddress {
    /// Creates a new transport address.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the address as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<std::net::SocketAddr> for TransportAddress {
    fn from(value: std::net::SocketAddr) -> Self {
        Self(value.to_string())
    }
}

impl From<TransportSocketAddr> for TransportAddress {
    fn from(value: TransportSocketAddr) -> Self {
        Self(value.to_string())
    }
}

impl From<TransportUrl> for TransportAddress {
    fn from(value: TransportUrl) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for TransportAddress {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::fmt::Display for TransportAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Transport protocol classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TransportProtocol {
    /// TCP transport.
    Tcp,
    /// WebSocket transport.
    WebSocket,
    /// In-memory transport for testing.
    Memory,
    /// Unknown or unspecified transport.
    #[default]
    Unknown,
}

/// Connection role metadata for bidirectional transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportRole {
    /// Client-side of the connection.
    Client,
    /// Server-side of the connection.
    Server,
}

/// Typed connection metadata.
#[derive(Debug, Clone, Default)]
pub struct TransportMetadata {
    /// The transport protocol in use.
    pub protocol: TransportProtocol,
    /// TCP_NODELAY option, if applicable.
    pub nodelay: Option<bool>,
    /// WebSocket URL, if applicable.
    pub url: Option<TransportUrl>,
    /// HTTP status code for WebSocket connections.
    pub status: Option<u16>,
    /// Client or server role.
    pub role: Option<TransportRole>,
    /// WebSocket subprotocol negotiated.
    pub subprotocol: Option<String>,
}

impl TransportMetadata {
    /// Creates TCP transport metadata.
    pub fn tcp(nodelay: bool) -> Self {
        Self {
            protocol: TransportProtocol::Tcp,
            nodelay: Some(nodelay),
            url: None,
            status: None,
            role: None,
            subprotocol: None,
        }
    }

    /// Creates WebSocket client transport metadata.
    pub fn websocket_client(url: TransportUrl, status: u16, subprotocol: Option<String>) -> Self {
        Self {
            protocol: TransportProtocol::WebSocket,
            nodelay: None,
            url: Some(url),
            status: Some(status),
            role: Some(TransportRole::Client),
            subprotocol,
        }
    }

    /// Creates WebSocket server transport metadata.
    pub fn websocket_server() -> Self {
        Self {
            protocol: TransportProtocol::WebSocket,
            nodelay: None,
            url: None,
            status: None,
            role: Some(TransportRole::Server),
            subprotocol: None,
        }
    }

    /// Creates in-memory transport metadata.
    pub fn memory() -> Self {
        Self {
            protocol: TransportProtocol::Memory,
            nodelay: None,
            url: None,
            status: None,
            role: None,
            subprotocol: None,
        }
    }
}

/// Transport handler configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Connection timeout
    pub connect_timeout: NonZeroDuration,
    /// Read timeout
    pub read_timeout: NonZeroDuration,
    /// Write timeout
    pub write_timeout: NonZeroDuration,
    /// Buffer size
    pub buffer_size: NonZeroUsize,
}

#[allow(clippy::expect_used)] // Constants (30, 60, 64*1024) are always non-zero
impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: NonZeroDuration::from_secs(30)
                .expect("connect timeout should be non-zero"),
            read_timeout: NonZeroDuration::from_secs(60).expect("read timeout should be non-zero"),
            write_timeout: NonZeroDuration::from_secs(30)
                .expect("write timeout should be non-zero"),
            buffer_size: NonZeroUsize::new(64 * 1024).expect("buffer size should be non-zero"),
        }
    }
}

/// Transport connection result
#[derive(Debug, Clone)]
pub struct TransportConnection {
    /// Connection identifier
    pub connection_id: ConnectionId,
    /// Local address
    pub local_addr: TransportAddress,
    /// Remote address
    pub remote_addr: TransportAddress,
    /// Connection metadata
    pub metadata: TransportMetadata,
}

/// Transport error types
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Connection setup or management failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// Underlying IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Operation exceeded timeout
    #[error("Timeout: {0}")]
    Timeout(String),
    /// Protocol-level error (framing, serialization format)
    #[error("Protocol error: {0}")]
    Protocol(String),
    /// Message serialization/deserialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

type TransportResult<T> = Result<T, TransportError>;

// Transport facade patterns removed - migrated to orchestration layer
//
// **MIGRATION NOTE**: Facade patterns that violate Layer 3 principles have been removed:
//
// - `TransportManager`: Combined multiple handlers with routing logic → Move to aura-protocol
// - `RetryingTransportManager`: Wrapper with coordination logic → Move to aura-protocol
//
// These patterns violated Layer 3 principles by performing multi-handler coordination
// instead of implementing single-party, stateless effects. The coordination logic
// belongs in Layer 4 (aura-protocol).
// Individual transport handlers remain in this crate as they follow Layer 3 principles:
// - TcpTransportHandler, WebSocketTransportHandler, RealTransportHandler
//
// Coordination patterns now live in aura-protocol:
// - TransportManager coordinates multiple transport handlers
// - RetryingTransportManager provides retry orchestration
// - Protocol-level routing and connection management logic resides in Layer 4

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
//! (routing, retry, connection management) belongs in aura-protocol/transport (Layer 4).
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

/// Transport handler configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Connection timeout
    pub connect_timeout: std::time::Duration,
    /// Read timeout
    pub read_timeout: std::time::Duration,
    /// Write timeout
    pub write_timeout: std::time::Duration,
    /// Buffer size
    pub buffer_size: usize,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: std::time::Duration::from_secs(30),
            read_timeout: std::time::Duration::from_secs(60),
            write_timeout: std::time::Duration::from_secs(30),
            buffer_size: 64 * 1024,
        }
    }
}

/// Transport connection result
#[derive(Debug, Clone)]
pub struct TransportConnection {
    /// Connection identifier
    pub connection_id: String,
    /// Local address
    pub local_addr: String,
    /// Remote address
    pub remote_addr: String,
    /// Connection metadata
    pub metadata: std::collections::HashMap<String, String>,
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
// TODO: Implement proper Layer 4 coordination patterns in aura-protocol where:
// - TransportManager coordinates multiple transport handlers
// - RetryingTransportManager provides retry orchestration
// - Protocol-level routing and connection management logic resides

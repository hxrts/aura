//! Transport Effect Handlers
//!
//! Layer 3: Single-party, stateless transport effect implementations.
//! NO choreography - these are context-free effect handlers only.
//! Target: Each file <200 lines, use mature libraries.

pub mod framing;
pub mod memory;
pub mod tcp;
pub mod utils;
pub mod websocket;

#[cfg(test)]
mod tests;

pub use framing::FramingHandler;
// Facade patterns removed - see integration.rs migration notes
// pub use integration::{RetryingTransportManager, TransportManager};
pub use memory::InMemoryTransportHandler;
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
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Timeout: {0}")]
    Timeout(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
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
// - TcpTransportHandler, WebSocketTransportHandler, InMemoryTransportHandler
//
// TODO: Implement proper Layer 4 coordination patterns in aura-protocol where:
// - TransportManager coordinates multiple transport handlers
// - RetryingTransportManager provides retry orchestration
// - Protocol-level routing and connection management logic resides

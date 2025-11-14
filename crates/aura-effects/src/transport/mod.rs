//! Transport Effect Handlers
//!
//! Layer 3: Single-party, stateless transport effect implementations.
//! NO choreography - these are context-free effect handlers only.
//! Target: Each file <200 lines, use mature libraries.

pub mod tcp;
pub mod websocket;
pub mod memory;
pub mod framing;
pub mod utils;
pub mod integration;

#[cfg(test)]
mod tests;

pub use tcp::TcpTransportHandler;
pub use websocket::WebSocketTransportHandler;
pub use memory::InMemoryTransportHandler;
pub use framing::FramingHandler;
pub use utils::{AddressResolver, TimeoutHelper, BufferUtils, ConnectionMetrics, UrlValidator};
pub use integration::{TransportManager, RetryingTransportManager};

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
#[derive(Debug)]
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

//! Network effects trait definitions
//!
//! This module defines the trait interfaces for network communication operations.
//! Implementations are provided by aura-protocol handlers using aura-transport.

use async_trait::async_trait;
use uuid::Uuid;

/// Network address for peer communication
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NetworkAddress {
    address: String,
}

impl NetworkAddress {
    /// Create a new network address
    pub fn new(address: String) -> Self {
        Self { address }
    }

    /// Get the address string
    pub fn as_str(&self) -> &str {
        &self.address
    }
}

impl From<&str> for NetworkAddress {
    fn from(address: &str) -> Self {
        Self::new(address.to_string())
    }
}

impl From<String> for NetworkAddress {
    fn from(address: String) -> Self {
        Self::new(address)
    }
}

/// Network operation errors
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// Failed to send a message to the destination
    #[error("Failed to send message: {0}")]
    SendFailed(String),
    /// Failed to receive a message from the source
    #[error("Failed to receive message: {0}")]
    ReceiveFailed(String),
    /// No message is available to receive
    #[error("No message available")]
    NoMessage,
    /// Failed to establish a connection
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// Operation is not implemented
    #[error("Not implemented")]
    NotImplemented,
    /// Operation timed out
    #[error("Operation '{operation}' timed out after {timeout_ms}ms")]
    OperationTimeout {
        /// The operation that timed out
        operation: String,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },
    /// Request retry limit exceeded
    #[error("Retry limit exceeded after {attempts} attempts. Last error: {last_error}")]
    RetryLimitExceeded {
        /// Number of retry attempts made
        attempts: usize,
        /// Error message from the last attempt
        last_error: String,
    },
    /// Circuit breaker is open
    #[error("Circuit breaker is open: {reason}")]
    CircuitBreakerOpen {
        /// Reason the circuit breaker was opened
        reason: String,
    },
    /// Peer unreachable
    #[error("Peer unreachable: {peer_id}")]
    PeerUnreachable {
        /// Identifier of the unreachable peer
        peer_id: String,
    },
    /// Network partition detected
    #[error("Network partition detected: {details}")]
    NetworkPartition {
        /// Details about the detected partition
        details: String,
    },
    /// Message validation failed
    #[error("Message validation failed: {reason}")]
    ValidationFailed {
        /// Reason validation failed
        reason: String,
    },
    /// Rate limit exceeded
    #[error("Rate limit exceeded: {limit} requests per {window_ms}ms window")]
    RateLimitExceeded {
        /// Request limit
        limit: usize,
        /// Time window in milliseconds
        window_ms: u64,
    },
}

/// Stream type for peer connection events
pub type PeerEventStream = std::pin::Pin<Box<dyn futures::Stream<Item = PeerEvent> + Send>>;

/// Peer connection events
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PeerEvent {
    /// Peer connected
    Connected(Uuid),
    /// Peer disconnected
    Disconnected(Uuid),
    /// Connection failed
    ConnectionFailed(Uuid, String),
}

/// Network effects interface for communication operations
///
/// This trait defines network operations for the Aura effects system.
/// Implementations are provided in aura-protocol handlers using aura-transport.
/// Different implementations exist for:
/// - Production: Real network communication
/// - Testing: Mock network with controllable message delivery
/// - Simulation: Network scenarios with partitions and faults
#[async_trait]
pub trait NetworkEffects: Send + Sync {
    /// Send a message to a specific peer
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError>;

    /// Broadcast a message to all connected peers
    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError>;

    /// Receive the next available message
    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError>;

    /// Receive message from a specific peer
    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError>;

    /// Get list of currently connected peers
    async fn connected_peers(&self) -> Vec<Uuid>;

    /// Check if a peer is connected
    async fn is_peer_connected(&self, peer_id: Uuid) -> bool;

    /// Subscribe to peer connection events
    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError>;
}

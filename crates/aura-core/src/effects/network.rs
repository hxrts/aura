//! Network effects trait definitions
//!
//! This module defines the trait interfaces for network communication operations.
//! Implementations are provided by aura-agent handlers using aura-transport.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing network communication (TCP, message sending/receiving)
//!
//! This is an infrastructure effect that must be implemented in `aura-effects`
//! with stateless handlers. Domain crates should not implement this trait directly.

use async_trait::async_trait;
use std::sync::Arc;
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
#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum NetworkError {
    /// Failed to send a message to the destination
    #[error("Failed to send message to {peer_id:?}: {reason}")]
    SendFailed {
        /// Optional peer the send targeted
        peer_id: Option<Uuid>,
        /// Reason for the failure
        reason: String,
    },
    /// Failed to receive a message from the source
    #[error("Failed to receive message: {reason}")]
    ReceiveFailed {
        /// Reason for the failure
        reason: String,
    },
    /// No message is available to receive
    #[error("No message available")]
    NoMessage,
    /// Broadcast operation failed
    #[error("Broadcast failed: {reason}")]
    BroadcastFailed {
        /// Reason for broadcast failure
        reason: String,
    },
    /// Failed to establish a connection
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    /// Operation is unsupported by the current handler
    #[error("Unsupported operation")]
    NotImplemented,
    /// Serialization failed while preparing a network payload
    #[error("Serialization failed: {error}")]
    SerializationFailed {
        /// Serialization error message
        error: String,
    },
    /// Deserialization failed while decoding a payload
    #[error("Deserialization failed: {error}")]
    DeserializationFailed {
        /// Deserialization error message
        error: String,
    },
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
        attempts: u32,
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
        limit: u32,
        /// Time window in milliseconds
        window_ms: u64,
    },
    /// Subscription to peer events failed
    #[error("Subscription failed: {reason}")]
    SubscriptionFailed {
        /// Reason for failure
        reason: String,
    },
}

/// Opaque UDP endpoint wrapper to keep core effects runtime-agnostic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct UdpEndpoint {
    address: String,
}

impl UdpEndpoint {
    /// Create a new UDP endpoint wrapper.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
        }
    }

    /// Get the address string.
    pub fn as_str(&self) -> &str {
        &self.address
    }
}

impl std::fmt::Display for UdpEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.address)
    }
}

impl From<&str> for UdpEndpoint {
    fn from(address: &str) -> Self {
        Self::new(address)
    }
}

impl From<String> for UdpEndpoint {
    fn from(address: String) -> Self {
        Self::new(address)
    }
}

/// UDP endpoint operations for Aura effects.
#[async_trait]
pub trait UdpEndpointEffects: Send + Sync {
    /// Enable or disable UDP broadcast on the socket.
    async fn set_broadcast(&self, enabled: bool) -> Result<(), NetworkError>;

    /// Send a datagram to the destination address.
    async fn send_to(&self, payload: &[u8], addr: &UdpEndpoint) -> Result<usize, NetworkError>;

    /// Receive a datagram into the provided buffer.
    async fn recv_from(&self, buffer: &mut [u8]) -> Result<(usize, UdpEndpoint), NetworkError>;
}

/// UDP effect surface for binding sockets.
#[async_trait]
pub trait UdpEffects: Send + Sync {
    /// Bind a UDP socket to the given address.
    async fn udp_bind(
        &self,
        addr: UdpEndpoint,
    ) -> Result<Arc<dyn UdpEndpointEffects>, NetworkError>;
}

#[async_trait]
impl<T: UdpEffects + ?Sized> UdpEffects for Arc<T> {
    async fn udp_bind(
        &self,
        addr: UdpEndpoint,
    ) -> Result<Arc<dyn UdpEndpointEffects>, NetworkError> {
        (**self).udp_bind(addr).await
    }
}

#[async_trait]
impl<T: UdpEndpointEffects + ?Sized> UdpEndpointEffects for Arc<T> {
    async fn set_broadcast(&self, enabled: bool) -> Result<(), NetworkError> {
        (**self).set_broadcast(enabled).await
    }

    async fn send_to(&self, payload: &[u8], addr: &UdpEndpoint) -> Result<usize, NetworkError> {
        (**self).send_to(payload, addr).await
    }

    async fn recv_from(&self, buffer: &mut [u8]) -> Result<(usize, UdpEndpoint), NetworkError> {
        (**self).recv_from(buffer).await
    }
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

/// Core network effects interface for communication operations.
///
/// This trait defines network operations for the Aura effects system.
/// Implementations are provided in aura-agent handlers using aura-transport.
/// Different implementations exist for:
/// - Production: Real network communication
/// - Testing: Mock network with controllable message delivery
/// - Simulation: Network scenarios with partitions and faults
#[async_trait]
pub trait NetworkCoreEffects: Send + Sync {
    /// Send a message to a specific peer
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError>;

    /// Broadcast a message to all connected peers
    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError>;

    /// Receive the next available message
    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError>;
}

/// Optional network effects that build on the core interface.
#[async_trait]
pub trait NetworkExtendedEffects: NetworkCoreEffects + Send + Sync {
    /// Receive message from a specific peer
    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    /// Get list of currently connected peers
    async fn connected_peers(&self) -> Vec<Uuid> {
        Vec::new()
    }

    /// Check if a peer is connected
    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        false
    }

    /// Subscribe to peer connection events
    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    // === Connection-oriented methods (for transport coordination) ===

    /// Open a connection to the specified address
    ///
    /// Returns a connection identifier that can be used with `send` and `close`.
    /// This provides lower-level connection management for transport coordinators.
    async fn open(&self, _address: &str) -> Result<String, NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    /// Send data over an established connection
    ///
    /// The connection_id must be from a previous successful `open` call.
    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }

    /// Close an established connection
    ///
    /// After closing, the connection_id is no longer valid.
    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        Err(NetworkError::NotImplemented)
    }
}

/// Combined network effects surface (core + extended).
pub trait NetworkEffects: NetworkCoreEffects + NetworkExtendedEffects {}

impl<T: NetworkCoreEffects + NetworkExtendedEffects + ?Sized> NetworkEffects for T {}
/// Blanket implementation for Arc<T> where T: NetworkCoreEffects
#[async_trait]
impl<T: NetworkCoreEffects + ?Sized> NetworkCoreEffects for std::sync::Arc<T> {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        (**self).send_to_peer(peer_id, message).await
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        (**self).broadcast(message).await
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        (**self).receive().await
    }
}

/// Blanket implementation for Arc<T> where T: NetworkExtendedEffects
#[async_trait]
impl<T: NetworkExtendedEffects + ?Sized> NetworkExtendedEffects for std::sync::Arc<T> {
    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        (**self).receive_from(peer_id).await
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        (**self).connected_peers().await
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        (**self).is_peer_connected(peer_id).await
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        (**self).subscribe_to_peer_events().await
    }

    async fn open(&self, address: &str) -> Result<String, NetworkError> {
        (**self).open(address).await
    }

    async fn send(&self, connection_id: &str, data: Vec<u8>) -> Result<(), NetworkError> {
        (**self).send(connection_id, data).await
    }

    async fn close(&self, connection_id: &str) -> Result<(), NetworkError> {
        (**self).close(connection_id).await
    }
}

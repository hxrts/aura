//! Network effects trait definitions
//!
//! This module defines the trait interfaces for network communication operations.
//! Implementations are provided by aura-transport crate.
//! Effect handlers are provided by aura-protocol handlers.

use async_trait::async_trait;
use aura_types::identifiers::DeviceId;

/// Network address for peer communication
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    /// Send a message to a specific device
    async fn send_to_device(&self, device_id: DeviceId, data: &[u8]) -> Result<(), NetworkError>;
    
    /// Broadcast a message to all connected peers
    async fn broadcast(&self, data: &[u8]) -> Result<(), NetworkError>;
    
    /// Receive the next available message
    async fn receive_message(&self) -> Result<(DeviceId, Vec<u8>), NetworkError>;
    
    /// Connect to a peer by device ID
    async fn connect_to_device(&self, device_id: DeviceId) -> Result<(), NetworkError>;
    
    /// Disconnect from a peer
    async fn disconnect_from_device(&self, device_id: DeviceId) -> Result<(), NetworkError>;
    
    /// Get list of currently connected peers
    async fn connected_peers(&self) -> Vec<DeviceId>;
}
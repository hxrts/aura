//! Network effects interface
//!
//! Pure trait definitions for network operations used by protocols.

use async_trait::async_trait;
use uuid::Uuid;

/// Network effects for protocol communication
#[async_trait]
pub trait NetworkEffects: Send + Sync {
    /// Send a message to a specific peer
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError>;
    
    /// Broadcast a message to all peers in the session
    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError>;
    
    /// Receive the next message from any peer
    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError>;
    
    /// Receive a message from a specific peer
    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError>;
    
    /// Get the list of connected peers
    async fn connected_peers(&self) -> Vec<Uuid>;
    
    /// Check if a peer is connected
    async fn is_peer_connected(&self, peer_id: Uuid) -> bool;
    
    /// Subscribe to peer connection events
    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError>;
}

/// Network-related errors
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("Peer {peer_id} is not connected")]
    PeerNotConnected { peer_id: Uuid },
    
    #[error("Send timeout after {timeout_ms}ms")]
    SendTimeout { timeout_ms: u64 },
    
    #[error("Receive timeout after {timeout_ms}ms")]
    ReceiveTimeout { timeout_ms: u64 },
    
    #[error("Message too large: {size} bytes (max: {max_size})")]
    MessageTooLarge { size: usize, max_size: usize },
    
    #[error("Network transport error: {source}")]
    Transport { source: Box<dyn std::error::Error + Send + Sync> },
    
    #[error("Protocol error: {message}")]
    Protocol { message: String },
}

/// Stream of peer connection events
pub type PeerEventStream = Box<dyn futures::Stream<Item = PeerEvent> + Send + Unpin>;

/// Peer connection events
#[derive(Debug, Clone)]
pub enum PeerEvent {
    /// A peer connected
    Connected { peer_id: Uuid },
    /// A peer disconnected
    Disconnected { peer_id: Uuid },
    /// A peer's connection status changed
    StatusChanged { peer_id: Uuid, connected: bool },
}
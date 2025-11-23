//! In-Memory Transport Handler
//!
//! Stateful in-memory transport for testing and simulation.
//! Target: Transport handlers with shared state using Arc<RwLock<>>.

use async_trait::async_trait;
use aura_core::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

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
    pub metadata: HashMap<String, String>,
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

/// Registry for in-memory transport connections
#[derive(Debug, Default)]
pub struct TransportRegistry {
    /// Message channels by peer ID
    pub channels: HashMap<String, mpsc::UnboundedSender<Vec<u8>>>,
    /// Connection metadata
    pub connections: HashMap<String, TransportConnection>,
}

/// In-memory transport handler for testing
#[derive(Debug, Clone)]
pub struct InMemoryTransportHandler {
    _config: TransportConfig,
    registry: Arc<RwLock<TransportRegistry>>,
}

impl InMemoryTransportHandler {
    /// Create new in-memory transport handler
    pub fn new(config: TransportConfig) -> Self {
        Self {
            _config: config,
            registry: Arc::new(RwLock::new(TransportRegistry::default())),
        }
    }

    /// Create with default configuration
    #[allow(clippy::should_implement_trait)] // Method provides default config, not implementing Default trait
    pub fn default() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Create shared registry for multiple handlers
    pub fn with_shared_registry(registry: Arc<RwLock<TransportRegistry>>) -> Self {
        Self {
            _config: TransportConfig::default(),
            registry,
        }
    }

    /// Register a new peer with message channel
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()`
    /// and pass them to this method to avoid direct `Uuid::new_v4()` calls.
    pub async fn register_peer(
        &self,
        peer_id: &str,
        connection_uuid: Uuid,
    ) -> TransportResult<mpsc::UnboundedReceiver<Vec<u8>>> {
        let (tx, rx) = mpsc::unbounded_channel();

        let connection_id = format!("mem-{}", connection_uuid);
        let local_addr = "memory://local".to_string();
        let remote_addr = format!("memory://{}", peer_id);

        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "memory".to_string());
        metadata.insert("peer_id".to_string(), peer_id.to_string());

        let connection = TransportConnection {
            connection_id: connection_id.clone(),
            local_addr,
            remote_addr,
            metadata,
        };

        let mut registry = self.registry.write().await;
        registry.channels.insert(peer_id.to_string(), tx);
        registry.connections.insert(connection_id, connection);

        Ok(rx)
    }

    /// Unregister a peer
    pub async fn unregister_peer(&self, peer_id: &str) -> TransportResult<()> {
        let mut registry = self.registry.write().await;
        registry.channels.remove(peer_id);

        // Remove associated connections
        let connection_ids_to_remove: Vec<String> = registry
            .connections
            .iter()
            .filter(|(_, conn)| conn.metadata.get("peer_id") == Some(&peer_id.to_string()))
            .map(|(id, _)| id.clone())
            .collect();

        for id in connection_ids_to_remove {
            registry.connections.remove(&id);
        }

        Ok(())
    }

    /// Send message to peer
    pub async fn send_to_peer(&self, peer_id: &str, data: Vec<u8>) -> TransportResult<()> {
        let registry = self.registry.read().await;

        if let Some(tx) = registry.channels.get(peer_id) {
            tx.send(data).map_err(|_| {
                TransportError::ConnectionFailed(format!("Failed to send to peer: {}", peer_id))
            })?;
            Ok(())
        } else {
            Err(TransportError::ConnectionFailed(format!(
                "Peer not found: {}",
                peer_id
            )))
        }
    }

    /// Broadcast message to all registered peers
    pub async fn broadcast(&self, data: Vec<u8>) -> TransportResult<()> {
        let registry = self.registry.read().await;
        let mut failed_peers = Vec::new();

        for (peer_id, tx) in &registry.channels {
            if tx.send(data.clone()).is_err() {
                failed_peers.push(peer_id.clone());
            }
        }

        if !failed_peers.is_empty() {
            return Err(TransportError::ConnectionFailed(format!(
                "Failed to send to peers: {:?}",
                failed_peers
            )));
        }

        Ok(())
    }

    /// List all registered peers
    pub async fn list_peers(&self) -> TransportResult<Vec<String>> {
        let registry = self.registry.read().await;
        Ok(registry.channels.keys().cloned().collect())
    }

    /// Get connection info for peer
    pub async fn get_connection(
        &self,
        peer_id: &str,
    ) -> TransportResult<Option<TransportConnection>> {
        let registry = self.registry.read().await;

        let connection = registry
            .connections
            .values()
            .find(|conn| conn.metadata.get("peer_id") == Some(&peer_id.to_string()))
            .cloned();

        Ok(connection)
    }

    /// Get all active connections
    pub async fn get_all_connections(&self) -> TransportResult<Vec<TransportConnection>> {
        let registry = self.registry.read().await;
        Ok(registry.connections.values().cloned().collect())
    }

    /// Check if peer is registered
    pub async fn is_peer_registered(&self, peer_id: &str) -> bool {
        let registry = self.registry.read().await;
        registry.channels.contains_key(peer_id)
    }

    /// Get transport statistics
    pub async fn get_stats(&self) -> TransportResult<TransportStats> {
        let registry = self.registry.read().await;

        Ok(TransportStats {
            total_peers: registry.channels.len(),
            total_connections: registry.connections.len(),
            active_channels: registry
                .channels
                .values()
                .map(|tx| !tx.is_closed())
                .filter(|&active| active)
                .count(),
        })
    }
}

/// Transport statistics
#[derive(Debug, Clone)]
pub struct TransportStats {
    /// Total registered peers
    pub total_peers: usize,
    /// Total established connections
    pub total_connections: usize,
    /// Currently active message channels
    pub active_channels: usize,
}

#[async_trait]
impl NetworkEffects for InMemoryTransportHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let peer_str = peer_id.to_string();
        self.send_to_peer(&peer_str, message)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: e.to_string(),
            })
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.connected_peers().await;
        for peer in peers {
            // Use the trait method explicitly to avoid naming conflict
            NetworkEffects::send_to_peer(self, peer, message.clone()).await?;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // For memory transport, we need to implement proper message receiving
        // This is a placeholder implementation
        Err(NetworkError::ReceiveFailed {
            reason: "Not implemented for memory transport".to_string(),
        })
    }

    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        // Placeholder implementation
        Err(NetworkError::ReceiveFailed {
            reason: "Not implemented for memory transport".to_string(),
        })
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let registry = self.registry.read().await;
        registry
            .channels
            .keys()
            .filter_map(|k| Uuid::parse_str(k).ok())
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        let registry = self.registry.read().await;
        registry.channels.contains_key(&peer_id.to_string())
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        // Placeholder implementation for event subscription
        use futures::stream;
        use std::pin::Pin;

        let stream = stream::empty::<PeerEvent>();
        Ok(Pin::from(Box::new(stream)))
    }
}
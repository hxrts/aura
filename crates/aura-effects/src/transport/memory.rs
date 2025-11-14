//! In-Memory Transport Handler
//!
//! Stateless in-memory transport for testing and simulation.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, use std collections.

use super::{TransportConfig, TransportConnection, TransportError, TransportResult};
use aura_core::effects::NetworkEffects;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// In-memory transport handler for testing
#[derive(Debug, Clone)]
pub struct InMemoryTransportHandler {
    config: TransportConfig,
    registry: Arc<RwLock<TransportRegistry>>,
}

/// Registry for in-memory transport connections
#[derive(Debug, Default)]
struct TransportRegistry {
    /// Message channels by peer ID
    channels: HashMap<String, mpsc::UnboundedSender<Vec<u8>>>,
    /// Connection metadata
    connections: HashMap<String, TransportConnection>,
}

impl InMemoryTransportHandler {
    /// Create new in-memory transport handler
    pub fn new(config: TransportConfig) -> Self {
        Self {
            config,
            registry: Arc::new(RwLock::new(TransportRegistry::default())),
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Create shared registry for multiple handlers
    pub fn with_shared_registry(registry: Arc<RwLock<TransportRegistry>>) -> Self {
        Self {
            config: TransportConfig::default(),
            registry,
        }
    }

    /// Register a new peer with message channel
    pub async fn register_peer(&self, peer_id: &str) -> TransportResult<mpsc::UnboundedReceiver<Vec<u8>>> {
        let (tx, rx) = mpsc::unbounded_channel();
        
        let connection_id = format!("mem-{}", Uuid::new_v4());
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
            .filter(|(_, conn)| {
                conn.metadata.get("peer_id") == Some(&peer_id.to_string())
            })
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
            tx.send(data)
                .map_err(|_| TransportError::ConnectionFailed(
                    format!("Failed to send to peer: {}", peer_id)
                ))?;
            Ok(())
        } else {
            Err(TransportError::ConnectionFailed(
                format!("Peer not found: {}", peer_id)
            ))
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
            return Err(TransportError::ConnectionFailed(
                format!("Failed to send to peers: {:?}", failed_peers)
            ));
        }
        
        Ok(())
    }

    /// List all registered peers
    pub async fn list_peers(&self) -> TransportResult<Vec<String>> {
        let registry = self.registry.read().await;
        Ok(registry.channels.keys().cloned().collect())
    }

    /// Get connection info for peer
    pub async fn get_connection(&self, peer_id: &str) -> TransportResult<Option<TransportConnection>> {
        let registry = self.registry.read().await;
        
        let connection = registry
            .connections
            .values()
            .find(|conn| {
                conn.metadata.get("peer_id") == Some(&peer_id.to_string())
            })
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
            active_channels: registry.channels.values()
                .map(|tx| !tx.is_closed())
                .filter(|&active| active)
                .count(),
        })
    }
}

/// Transport statistics
#[derive(Debug, Clone)]
pub struct TransportStats {
    pub total_peers: usize,
    pub total_connections: usize,
    pub active_channels: usize,
}

#[async_trait]
impl NetworkEffects for InMemoryTransportHandler {
    type Error = TransportError;
    type PeerId = String;
    
    async fn send_to_peer(&self, peer_id: Self::PeerId, data: Vec<u8>) -> Result<(), Self::Error> {
        self.send_to_peer(&peer_id, data).await
    }
    
    async fn broadcast(&self, peers: Vec<Self::PeerId>, data: Vec<u8>) -> Result<(), Self::Error> {
        for peer in peers {
            self.send_to_peer(&peer, data.clone()).await?;
        }
        Ok(())
    }
}

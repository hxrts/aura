//! Transport adapter that bridges aura-transport with coordination Transport trait
//!
//! This module provides adapters to connect the transport crate's implementations
//! with the coordination layer's Transport interface, enabling real network transport
//! to be used in production.

use crate::{utils::ResultExt, AgentError, Result};
use async_trait::async_trait;
use aura_protocol::Transport as CoordinationTransport;
use aura_transport::Transport as AuraTransport;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Adapter that implements coordination Transport using aura-transport
pub struct CoordinationTransportAdapter<T: AuraTransport> {
    /// Underlying transport implementation
    transport: Arc<T>,
    /// Device ID mappings (peer_id string -> DeviceId)
    device_mappings: Arc<RwLock<HashMap<String, DeviceId>>>,
    /// Receive timeout for polling messages
    receive_timeout: Duration,
}

impl<T: AuraTransport> CoordinationTransportAdapter<T> {
    /// Create a new coordination transport adapter
    pub fn new(transport: Arc<T>) -> Self {
        info!("Creating coordination transport adapter");
        Self {
            transport,
            device_mappings: Arc::new(RwLock::new(HashMap::new())),
            receive_timeout: Duration::from_millis(100),
        }
    }

    /// Set the receive timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.receive_timeout = timeout;
        self
    }

    /// Register a device mapping for peer_id to DeviceId conversion
    pub async fn register_device_mapping(&self, peer_id: String, device_id: DeviceId) {
        let mut mappings = self.device_mappings.write().await;
        mappings.insert(peer_id.clone(), device_id);
        debug!("Registered device mapping: {} -> {}", peer_id, device_id);
    }

    /// Convert peer_id string to DeviceId
    async fn resolve_device_id(&self, peer_id: &str) -> std::result::Result<DeviceId, String> {
        let mappings = self.device_mappings.read().await;
        mappings
            .get(peer_id)
            .copied()
            .ok_or_else(|| format!("No device mapping found for peer_id: {}", peer_id))
    }
}

#[async_trait]
impl<T: AuraTransport> CoordinationTransport for CoordinationTransportAdapter<T> {
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> std::result::Result<(), String> {
        debug!("Sending {} bytes to peer {}", message.len(), peer_id);

        // Convert peer_id to DeviceId
        let device_id = self.resolve_device_id(peer_id).await?;

        // Ensure we're connected to the peer
        if !self.transport.is_peer_reachable(device_id).await {
            debug!("Not connected to peer {}, attempting to connect", peer_id);
            self.transport
                .connect_to_peer(device_id)
                .await
                .map_err(|e| format!("Failed to connect to peer {}: {}", peer_id, e))?;
        } else {
            debug!("Already connected to peer {}", peer_id);
        }

        // Send the message
        self.transport
            .send_to_peer(device_id, message)
            .await
            .map_err(|e| format!("Failed to send message to peer {}: {}", peer_id, e))
    }

    async fn broadcast_message(&self, message: &[u8]) -> std::result::Result<(), String> {
        debug!(
            "Broadcasting {} bytes to all connected peers",
            message.len()
        );

        // Get all device mappings and send to each
        let mappings = self.device_mappings.read().await;
        if mappings.is_empty() {
            warn!("No registered peer mappings for broadcast");
            return Ok(());
        }

        let device_ids: Vec<DeviceId> = mappings.values().copied().collect();
        drop(mappings);

        // Send to each registered peer that is reachable
        let mut errors = Vec::new();
        let mut sent_count = 0;

        for device_id in device_ids {
            if self.transport.is_peer_reachable(device_id).await {
                if let Err(e) = self.transport.send_to_peer(device_id, message).await {
                    errors.push(format!("Failed to send to {}: {}", device_id, e));
                } else {
                    sent_count += 1;
                }
            }
        }

        if sent_count == 0 && !errors.is_empty() {
            return Err(format!("Broadcast failed: {}", errors.join("; ")));
        }

        debug!("Broadcast completed: sent to {} peers", sent_count);
        Ok(())
    }

    async fn is_peer_reachable(&self, peer_id: &str) -> bool {
        debug!("Checking if peer {} is reachable", peer_id);

        // First try to resolve the device mapping
        let device_id = match self.resolve_device_id(peer_id).await {
            Ok(id) => id,
            Err(_) => {
                debug!("No device mapping for peer {}", peer_id);
                return false;
            }
        };

        // Check if peer is reachable
        let reachable = self.transport.is_peer_reachable(device_id).await;
        debug!("Peer {} reachability: {}", peer_id, reachable);
        reachable
    }
}

/// Factory for creating transport adapters
pub struct TransportAdapterFactory;

impl TransportAdapterFactory {
    /// Create a coordination transport adapter from a transport implementation
    pub fn create_coordination_adapter<T: AuraTransport>(
        transport: Arc<T>,
    ) -> CoordinationTransportAdapter<T> {
        CoordinationTransportAdapter::new(transport)
    }

    /// Create a coordination transport adapter with pre-registered device mappings
    pub async fn create_coordination_adapter_with_mappings<T: AuraTransport>(
        transport: Arc<T>,
        device_mappings: HashMap<String, DeviceId>,
    ) -> CoordinationTransportAdapter<T> {
        let adapter = CoordinationTransportAdapter::new(transport);

        for (peer_id, device_id) in device_mappings {
            adapter.register_device_mapping(peer_id, device_id).await;
        }

        adapter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_transport::MemoryTransport;

    #[tokio::test]
    async fn test_coordination_transport_adapter() {
        let device_id = DeviceId::new();
        let peer_device_id = DeviceId::new();
        let peer_id = "test_peer";

        // Create memory transport for testing
        let transport = Arc::new(MemoryTransport::default());

        // Create coordination adapter
        let adapter = CoordinationTransportAdapter::new(transport.clone());
        adapter
            .register_device_mapping(peer_id.to_string(), peer_device_id)
            .await;

        // Test is_peer_reachable with unconnected peer
        assert!(!adapter.is_peer_reachable(peer_id).await);
        assert!(!adapter.is_peer_reachable("unknown_peer").await);
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let device_id = DeviceId::new();
        let peer1_id = DeviceId::new();
        let peer2_id = DeviceId::new();

        // Create memory transport
        let transport = Arc::new(MemoryTransport::default());

        // Create coordination adapter
        let adapter = CoordinationTransportAdapter::new(transport.clone());
        adapter
            .register_device_mapping("peer1".to_string(), peer1_id)
            .await;
        adapter
            .register_device_mapping("peer2".to_string(), peer2_id)
            .await;

        // Test broadcast (will succeed even if peers not reachable)
        let message = b"broadcast message";
        let result = adapter.broadcast_message(message).await;
        // Broadcast succeeds as long as mappings exist, actual delivery depends on reachability
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_methods() {
        let transport = Arc::new(MemoryTransport::default());

        // Test basic factory method
        let _adapter = TransportAdapterFactory::create_coordination_adapter(transport.clone());

        // Test factory method with mappings
        let mut mappings = HashMap::new();
        mappings.insert("peer1".to_string(), DeviceId::new());
        mappings.insert("peer2".to_string(), DeviceId::new());

        let adapter = TransportAdapterFactory::create_coordination_adapter_with_mappings(
            transport,
            mappings.clone(),
        )
        .await;

        // Verify mappings were registered
        for (peer_id, device_id) in mappings {
            assert!(adapter.resolve_device_id(&peer_id).await.is_ok());
            assert_eq!(
                adapter.resolve_device_id(&peer_id).await.unwrap(),
                device_id
            );
        }
    }
}

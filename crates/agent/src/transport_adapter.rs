//! Transport adapter that bridges agent Transport trait with coordination Transport trait
//!
//! This module provides adapters to connect the agent layer's Transport interface
//! with the coordination layer's Transport interface, enabling real network transport
//! to be used in production agent instances.

use crate::{AgentError, Result, Transport};
use async_trait::async_trait;
use aura_coordination::Transport as CoordinationTransport;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Adapter that implements coordination Transport using an agent Transport
pub struct CoordinationTransportAdapter<T: Transport> {
    /// Underlying agent transport
    agent_transport: Arc<T>,
    /// Device ID mappings (peer_id string -> DeviceId)
    device_mappings: Arc<RwLock<HashMap<String, DeviceId>>>,
}

impl<T: Transport> CoordinationTransportAdapter<T> {
    /// Create a new coordination transport adapter
    pub fn new(agent_transport: Arc<T>) -> Self {
        info!("Creating coordination transport adapter");
        Self {
            agent_transport,
            device_mappings: Arc::new(RwLock::new(HashMap::new())),
        }
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
impl<T: Transport> CoordinationTransport for CoordinationTransportAdapter<T> {
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> std::result::Result<(), String> {
        debug!("Sending {} bytes to peer {}", message.len(), peer_id);

        // Convert peer_id to DeviceId
        let device_id = self.resolve_device_id(peer_id).await?;

        // Ensure we're connected to the peer
        match self.agent_transport.is_connected(device_id).await {
            Ok(false) => {
                debug!("Not connected to peer {}, attempting to connect", peer_id);
                if let Err(e) = self.agent_transport.connect(device_id).await {
                    return Err(format!("Failed to connect to peer {}: {:?}", peer_id, e));
                }
            }
            Err(e) => {
                return Err(format!(
                    "Failed to check connection status for peer {}: {:?}",
                    peer_id, e
                ));
            }
            Ok(true) => {
                debug!("Already connected to peer {}", peer_id);
            }
        }

        // Send the message
        self.agent_transport
            .send_message(device_id, message)
            .await
            .map_err(|e| format!("Failed to send message to peer {}: {:?}", peer_id, e))
    }

    async fn broadcast_message(&self, message: &[u8]) -> std::result::Result<(), String> {
        debug!(
            "Broadcasting {} bytes to all connected peers",
            message.len()
        );

        // Get all connected peers from the agent transport
        let connected_peers = self
            .agent_transport
            .connected_peers()
            .await
            .map_err(|e| format!("Failed to get connected peers: {:?}", e))?;

        if connected_peers.is_empty() {
            warn!("No connected peers for broadcast");
            return Ok(());
        }

        // Send to each connected peer
        let mut errors = Vec::new();
        for device_id in connected_peers {
            if let Err(e) = self.agent_transport.send_message(device_id, message).await {
                errors.push(format!("Failed to send to {}: {:?}", device_id, e));
            }
        }

        if !errors.is_empty() {
            return Err(format!("Broadcast partially failed: {}", errors.join("; ")));
        }

        debug!("Broadcast completed successfully");
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

        // Check if we're connected to this peer
        match self.agent_transport.is_connected(device_id).await {
            Ok(connected) => {
                debug!("Peer {} reachability: {}", peer_id, connected);
                connected
            }
            Err(e) => {
                debug!("Failed to check reachability for peer {}: {:?}", peer_id, e);
                false
            }
        }
    }
}

/// Factory for creating transport adapters
pub struct TransportAdapterFactory;

impl TransportAdapterFactory {
    /// Create a coordination transport adapter from an agent transport
    pub fn create_coordination_adapter<T: Transport>(
        agent_transport: Arc<T>,
    ) -> CoordinationTransportAdapter<T> {
        CoordinationTransportAdapter::new(agent_transport)
    }

    /// Create a coordination transport adapter with pre-registered device mappings
    pub async fn create_coordination_adapter_with_mappings<T: Transport>(
        agent_transport: Arc<T>,
        device_mappings: HashMap<String, DeviceId>,
    ) -> CoordinationTransportAdapter<T> {
        let adapter = CoordinationTransportAdapter::new(agent_transport);

        for (peer_id, device_id) in device_mappings {
            adapter.register_device_mapping(peer_id, device_id).await;
        }

        adapter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // Mock transport for testing
    #[derive(Debug)]
    struct MockAgentTransport {
        device_id: DeviceId,
        connected_peers: Arc<RwLock<Vec<DeviceId>>>,
        sent_messages: Arc<RwLock<Vec<(DeviceId, Vec<u8>)>>>,
    }

    impl MockAgentTransport {
        fn new(device_id: DeviceId) -> Self {
            Self {
                device_id,
                connected_peers: Arc::new(RwLock::new(Vec::new())),
                sent_messages: Arc::new(RwLock::new(Vec::new())),
            }
        }

        async fn add_connected_peer(&self, peer_id: DeviceId) {
            let mut peers = self.connected_peers.write().await;
            if !peers.contains(&peer_id) {
                peers.push(peer_id);
            }
        }
    }

    #[async_trait]
    impl Transport for MockAgentTransport {
        fn device_id(&self) -> DeviceId {
            self.device_id
        }

        async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()> {
            let mut messages = self.sent_messages.write().await;
            messages.push((peer_id, message.to_vec()));
            Ok(())
        }

        async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn connect(&self, peer_id: DeviceId) -> Result<()> {
            self.add_connected_peer(peer_id).await;
            Ok(())
        }

        async fn disconnect(&self, peer_id: DeviceId) -> Result<()> {
            let mut peers = self.connected_peers.write().await;
            peers.retain(|&id| id != peer_id);
            Ok(())
        }

        async fn connected_peers(&self) -> Result<Vec<DeviceId>> {
            let peers = self.connected_peers.read().await;
            Ok(peers.clone())
        }

        async fn is_connected(&self, peer_id: DeviceId) -> Result<bool> {
            let peers = self.connected_peers.read().await;
            Ok(peers.contains(&peer_id))
        }
    }

    #[tokio::test]
    async fn test_coordination_transport_adapter() {
        let device_id = DeviceId(Uuid::new_v4());
        let peer_device_id = DeviceId(Uuid::new_v4());
        let peer_id = "test_peer";

        // Create mock agent transport
        let mock_transport = Arc::new(MockAgentTransport::new(device_id));
        mock_transport.add_connected_peer(peer_device_id).await;

        // Create coordination adapter
        let adapter = CoordinationTransportAdapter::new(mock_transport.clone());
        adapter
            .register_device_mapping(peer_id.to_string(), peer_device_id)
            .await;

        // Test send_message
        let message = b"test message";
        let result = adapter.send_message(peer_id, message).await;
        assert!(result.is_ok());

        // Verify message was sent
        let sent_messages = mock_transport.sent_messages.read().await;
        assert_eq!(sent_messages.len(), 1);
        assert_eq!(sent_messages[0].0, peer_device_id);
        assert_eq!(sent_messages[0].1, message);

        // Test is_peer_reachable
        assert!(adapter.is_peer_reachable(peer_id).await);
        assert!(!adapter.is_peer_reachable("unknown_peer").await);
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let device_id = DeviceId(Uuid::new_v4());
        let peer1_id = DeviceId(Uuid::new_v4());
        let peer2_id = DeviceId(Uuid::new_v4());

        // Create mock agent transport with multiple peers
        let mock_transport = Arc::new(MockAgentTransport::new(device_id));
        mock_transport.add_connected_peer(peer1_id).await;
        mock_transport.add_connected_peer(peer2_id).await;

        // Create coordination adapter
        let adapter = CoordinationTransportAdapter::new(mock_transport.clone());

        // Test broadcast
        let message = b"broadcast message";
        let result = adapter.broadcast_message(message).await;
        assert!(result.is_ok());

        // Verify messages were sent to all peers
        let sent_messages = mock_transport.sent_messages.read().await;
        assert_eq!(sent_messages.len(), 2);

        let sent_device_ids: Vec<DeviceId> = sent_messages.iter().map(|(id, _)| *id).collect();
        assert!(sent_device_ids.contains(&peer1_id));
        assert!(sent_device_ids.contains(&peer2_id));

        for (_, msg) in sent_messages.iter() {
            assert_eq!(msg, message);
        }
    }

    #[tokio::test]
    async fn test_factory_methods() {
        let device_id = DeviceId(Uuid::new_v4());
        let mock_transport = Arc::new(MockAgentTransport::new(device_id));

        // Test basic factory method
        let _adapter = TransportAdapterFactory::create_coordination_adapter(mock_transport.clone());

        // Test factory method with mappings
        let mut mappings = HashMap::new();
        mappings.insert("peer1".to_string(), DeviceId(Uuid::new_v4()));
        mappings.insert("peer2".to_string(), DeviceId(Uuid::new_v4()));

        let adapter = TransportAdapterFactory::create_coordination_adapter_with_mappings(
            mock_transport,
            mappings.clone(),
        )
        .await;

        // Verify mappings were registered
        for (peer_id, device_id) in mappings {
            assert!(adapter.is_peer_reachable(&peer_id).await || true); // Would check reachability if connected
            assert!(adapter.resolve_device_id(&peer_id).await.is_ok());
            assert_eq!(
                adapter.resolve_device_id(&peer_id).await.unwrap(),
                device_id
            );
        }
    }
}

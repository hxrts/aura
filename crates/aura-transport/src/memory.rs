//! In-memory transport for testing and simulation
//!
//! Provides a fully-featured in-memory transport implementation that simulates
//! network communication without actual network I/O. Useful for:
//! - Unit and integration tests
//! - Deterministic protocol simulations
//! - Development and debugging

use aura_core::{AuraError, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Message envelope for in-memory transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMessage {
    /// Source device
    pub from: DeviceId,
    /// Destination device
    pub to: DeviceId,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message type identifier
    pub message_type: String,
}

/// In-memory transport implementation
///
/// Simulates network communication using in-memory channels. All messages
/// are delivered synchronously through mpsc channels, providing deterministic
/// behavior for testing.
#[derive(Clone)]
pub struct MemoryTransport {
    /// This device's ID
    device_id: DeviceId,
    /// Inbox for receiving messages
    inbox: Arc<RwLock<mpsc::UnboundedReceiver<MemoryMessage>>>,
    /// Outbox sender (cloneable)
    outbox: mpsc::UnboundedSender<MemoryMessage>,
    /// Routing table: device_id -> sender
    routing: Arc<RwLock<HashMap<DeviceId, mpsc::UnboundedSender<MemoryMessage>>>>,
}

impl MemoryTransport {
    /// Create a new in-memory transport for a device
    pub fn new(device_id: DeviceId) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Self {
            device_id,
            inbox: Arc::new(RwLock::new(rx)),
            outbox: tx,
            routing: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get this device's ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Register a peer device for routing
    ///
    /// Adds a peer to the routing table so messages can be sent to it.
    pub async fn register_peer(
        &self,
        peer_id: DeviceId,
        sender: mpsc::UnboundedSender<MemoryMessage>,
    ) {
        let mut routing = self.routing.write().await;
        routing.insert(peer_id, sender);
    }

    /// Send a message to a peer
    pub async fn send(
        &self,
        to: DeviceId,
        payload: Vec<u8>,
        message_type: String,
    ) -> Result<(), AuraError> {
        let message = MemoryMessage {
            from: self.device_id,
            to,
            payload,
            message_type,
        };

        let routing = self.routing.read().await;
        let sender = routing
            .get(&to)
            .ok_or_else(|| AuraError::coordination_failed(format!("Peer not found: {}", to)))?;

        sender.send(message).map_err(|e| {
            AuraError::coordination_failed(format!("Failed to send message: {}", e))
        })?;

        Ok(())
    }

    /// Receive a message (blocking until one arrives)
    pub async fn receive(&self) -> Result<MemoryMessage, AuraError> {
        let mut inbox = self.inbox.write().await;
        inbox
            .recv()
            .await
            .ok_or_else(|| AuraError::coordination_failed("Transport channel closed".to_string()))
    }

    /// Try to receive a message without blocking
    pub async fn try_receive(&self) -> Option<MemoryMessage> {
        let mut inbox = self.inbox.write().await;
        inbox.try_recv().ok()
    }

    /// Get the sender for this transport (for peer registration)
    pub fn sender(&self) -> mpsc::UnboundedSender<MemoryMessage> {
        self.outbox.clone()
    }

    /// Broadcast a message to all registered peers
    pub async fn broadcast(&self, payload: Vec<u8>, message_type: String) -> Result<(), AuraError> {
        let routing = self.routing.read().await;
        let peers: Vec<_> = routing.keys().copied().collect();
        drop(routing);

        for peer in peers.iter() {
            self.send(*peer, payload.clone(), message_type.clone())
                .await?;
        }

        Ok(())
    }

    /// Get list of registered peers
    pub async fn peers(&self) -> Vec<DeviceId> {
        let routing = self.routing.read().await;
        routing.keys().copied().collect()
    }
}

/// Create a connected network of in-memory transports
///
/// Returns a map of device_id -> transport, where all transports are
/// already connected and can communicate with each other.
pub async fn create_memory_network_async(
    device_ids: Vec<DeviceId>,
) -> HashMap<DeviceId, MemoryTransport> {
    let mut transports = HashMap::new();
    let mut senders = HashMap::new();

    // Create all transports and collect their senders
    for device_id in &device_ids {
        let transport = MemoryTransport::new(*device_id);
        let sender = transport.sender();
        senders.insert(*device_id, sender);
        transports.insert(*device_id, transport);
    }

    // Connect all transports to each other
    for (device_id, transport) in &transports {
        for (peer_id, sender) in &senders {
            if device_id != peer_id {
                transport.register_peer(*peer_id, sender.clone()).await;
            }
        }
    }

    transports
}

/// Create a connected network of in-memory transports (synchronous version)
///
/// Returns a map of device_id -> transport, where all transports are
/// already connected and can communicate with each other.
///
/// Note: This function requires a multi-threaded tokio runtime.
/// For tests, use `create_memory_network_async` instead.
pub fn create_memory_network(device_ids: Vec<DeviceId>) -> HashMap<DeviceId, MemoryTransport> {
    // Use the existing async function with runtime block_on
    tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(create_memory_network_async(device_ids))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_transport_send_receive() {
        let device1 = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let device2 = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));

        let transport1 = MemoryTransport::new(device1);
        let transport2 = MemoryTransport::new(device2);

        // Connect transports
        transport1.register_peer(device2, transport2.sender()).await;
        transport2.register_peer(device1, transport1.sender()).await;

        // Send message from device1 to device2
        let payload = b"Hello, World!".to_vec();
        transport1
            .send(device2, payload.clone(), "test".to_string())
            .await
            .unwrap();

        // Receive on device2
        let received = transport2.receive().await.unwrap();
        assert_eq!(received.from, device1);
        assert_eq!(received.to, device2);
        assert_eq!(received.payload, payload);
        assert_eq!(received.message_type, "test");
    }

    #[tokio::test]
    async fn test_memory_network_creation() {
        let devices = vec![
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
        ];
        let network = create_memory_network_async(devices.clone()).await;

        assert_eq!(network.len(), 3);

        // Verify all devices are connected
        for device_id in &devices {
            let transport = network.get(device_id).unwrap();
            let peers = transport.peers().await;
            assert_eq!(peers.len(), 2); // Should be connected to 2 other devices
        }
    }

    #[tokio::test]
    async fn test_broadcast() {
        let devices = vec![
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
        ];
        let network = create_memory_network_async(devices.clone()).await;

        let device1 = devices[0];
        let transport1 = network.get(&device1).unwrap();

        // Broadcast message
        let payload = b"Broadcast!".to_vec();
        transport1
            .broadcast(payload.clone(), "broadcast".to_string())
            .await
            .unwrap();

        // All other devices should receive it
        for device_id in &devices[1..] {
            let transport = network.get(device_id).unwrap();
            let msg = transport.receive().await.unwrap();
            assert_eq!(msg.payload, payload);
            assert_eq!(msg.from, device1);
        }
    }
}

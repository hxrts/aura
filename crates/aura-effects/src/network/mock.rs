//! Mock network effect handler for testing

use async_trait::async_trait;
use aura_core::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

/// Mock network handler for testing
#[derive(Debug, Clone)]
pub struct MockNetworkHandler {
    /// Simulated connection state
    connections: Arc<Mutex<HashMap<Uuid, bool>>>,
    /// Message queue for incoming messages
    message_queue: Arc<Mutex<VecDeque<(Uuid, Vec<u8>)>>>,
    /// Peer event broadcaster
    event_broadcaster: Arc<Mutex<Option<mpsc::UnboundedSender<PeerEvent>>>>,
    /// Deterministic behavior controls
    should_fail_send: Arc<Mutex<bool>>,
    should_fail_broadcast: Arc<Mutex<bool>>,
    network_delay_ms: Arc<Mutex<u64>>,
}

impl MockNetworkHandler {
    /// Create a new mock network handler
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            message_queue: Arc::new(Mutex::new(VecDeque::new())),
            event_broadcaster: Arc::new(Mutex::new(None)),
            should_fail_send: Arc::new(Mutex::new(false)),
            should_fail_broadcast: Arc::new(Mutex::new(false)),
            network_delay_ms: Arc::new(Mutex::new(0)),
        }
    }

    /// Add a simulated peer connection
    pub fn add_peer(&self, peer_id: Uuid) {
        let mut connections = self.connections.lock().unwrap();
        connections.insert(peer_id, true);

        // Send connection event
        if let Some(broadcaster) = self.event_broadcaster.lock().unwrap().as_ref() {
            let _ = broadcaster.send(PeerEvent::Connected(peer_id));
        }
    }

    /// Remove a simulated peer connection
    pub fn remove_peer(&self, peer_id: Uuid) {
        let mut connections = self.connections.lock().unwrap();
        connections.remove(&peer_id);

        // Send disconnection event
        if let Some(broadcaster) = self.event_broadcaster.lock().unwrap().as_ref() {
            let _ = broadcaster.send(PeerEvent::Disconnected(peer_id));
        }
    }

    /// Simulate a message received from a peer
    pub fn simulate_message_from_peer(&self, peer_id: Uuid, message: Vec<u8>) {
        let mut queue = self.message_queue.lock().unwrap();
        queue.push_back((peer_id, message));
    }

    /// Set whether sends should fail
    pub fn set_should_fail_send(&self, should_fail: bool) {
        *self.should_fail_send.lock().unwrap() = should_fail;
    }

    /// Set whether broadcasts should fail
    pub fn set_should_fail_broadcast(&self, should_fail: bool) {
        *self.should_fail_broadcast.lock().unwrap() = should_fail;
    }

    /// Set network delay simulation
    pub fn set_network_delay(&self, delay_ms: u64) {
        *self.network_delay_ms.lock().unwrap() = delay_ms;
    }

    /// Get the number of queued messages
    pub fn queued_message_count(&self) -> usize {
        self.message_queue.lock().unwrap().len()
    }
}

impl Default for MockNetworkHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkEffects for MockNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, _message: Vec<u8>) -> Result<(), NetworkError> {
        // Check if send should fail (drop guard immediately)
        let should_fail = *self.should_fail_send.lock().unwrap();
        if should_fail {
            return Err(NetworkError::SendFailed("Mock network failure".to_string()));
        }

        // Check if peer is connected (drop guard immediately)
        let is_connected = {
            let connections = self.connections.lock().unwrap();
            *connections.get(&peer_id).unwrap_or(&false)
        };

        if !is_connected {
            return Err(NetworkError::PeerUnreachable {
                peer_id: peer_id.to_string(),
            });
        }

        // Simulate network delay (drop guard immediately)
        let delay = *self.network_delay_ms.lock().unwrap();
        if delay > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }

        // In a real mock, this would be sent to the peer's handler
        // For testing purposes, we just succeed
        Ok(())
    }

    async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
        // Check if broadcast should fail (drop guard immediately)
        let should_fail = *self.should_fail_broadcast.lock().unwrap();
        if should_fail {
            return Err(NetworkError::SendFailed(
                "Mock broadcast failure".to_string(),
            ));
        }

        // Get delay and connected peer count (drop guards immediately)
        let delay = *self.network_delay_ms.lock().unwrap();
        let connected_count = {
            let connections = self.connections.lock().unwrap();
            connections
                .iter()
                .filter(|(_, &is_connected)| is_connected)
                .count()
        };

        // Simulate broadcasting to all connected peers
        for _ in 0..connected_count {
            // Simulate delay per peer
            if delay > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
            }
            // In a real mock, this would deliver the message to each peer
            // For testing, we just track that it would be sent
        }

        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        let mut queue = self.message_queue.lock().unwrap();
        if let Some((sender, message)) = queue.pop_front() {
            Ok((sender, message))
        } else {
            Err(NetworkError::NoMessage)
        }
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let mut queue = self.message_queue.lock().unwrap();

        // Find and remove first message from the specified peer
        let mut found_index = None;
        for (i, (sender, _)) in queue.iter().enumerate() {
            if *sender == peer_id {
                found_index = Some(i);
                break;
            }
        }

        if let Some(index) = found_index {
            let (_, message) = queue.remove(index).unwrap();
            Ok(message)
        } else {
            Err(NetworkError::NoMessage)
        }
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let connections = self.connections.lock().unwrap();
        connections
            .iter()
            .filter(|(_, is_connected)| **is_connected)
            .map(|(peer_id, _)| *peer_id)
            .collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        let connections = self.connections.lock().unwrap();
        connections.get(&peer_id).unwrap_or(&false).clone()
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Store the broadcaster for sending events
        {
            let mut broadcaster = self.event_broadcaster.lock().unwrap();
            *broadcaster = Some(tx);
        }

        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn test_mock_network_basic_operations() {
        let handler = MockNetworkHandler::new();
        let peer1 = Uuid::new_v4();
        let peer2 = Uuid::new_v4();

        // Initially no peers connected
        assert_eq!(handler.connected_peers().await.len(), 0);
        assert!(!handler.is_peer_connected(peer1).await);

        // Add peers
        handler.add_peer(peer1);
        handler.add_peer(peer2);

        // Check connections
        assert_eq!(handler.connected_peers().await.len(), 2);
        assert!(handler.is_peer_connected(peer1).await);
        assert!(handler.is_peer_connected(peer2).await);

        // Remove a peer
        handler.remove_peer(peer1);
        assert_eq!(handler.connected_peers().await.len(), 1);
        assert!(!handler.is_peer_connected(peer1).await);
        assert!(handler.is_peer_connected(peer2).await);
    }

    #[tokio::test]
    async fn test_mock_network_messaging() {
        let handler = MockNetworkHandler::new();
        let peer1 = Uuid::new_v4();
        let message = b"test message".to_vec();

        // Add peer
        handler.add_peer(peer1);

        // Test send to peer
        handler.send_to_peer(peer1, message.clone()).await.unwrap();

        // Simulate receiving a message
        handler.simulate_message_from_peer(peer1, message.clone());
        assert_eq!(handler.queued_message_count(), 1);

        // Receive the message
        let (sender, received_msg) = handler.receive().await.unwrap();
        assert_eq!(sender, peer1);
        assert_eq!(received_msg, message);
        assert_eq!(handler.queued_message_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_network_failure_simulation() {
        let handler = MockNetworkHandler::new();
        let peer1 = Uuid::new_v4();
        let message = b"test message".to_vec();

        handler.add_peer(peer1);

        // Test normal send
        handler.send_to_peer(peer1, message.clone()).await.unwrap();

        // Enable send failure
        handler.set_should_fail_send(true);
        let result = handler.send_to_peer(peer1, message.clone()).await;
        assert!(result.is_err());

        // Enable broadcast failure
        handler.set_should_fail_broadcast(true);
        let result = handler.broadcast(message).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mock_network_peer_events() {
        let handler = MockNetworkHandler::new();
        let peer1 = Uuid::new_v4();

        // Subscribe to events
        let mut event_stream = handler.subscribe_to_peer_events().await.unwrap();

        // Add a peer - should generate connection event
        handler.add_peer(peer1);

        // Check for connection event
        if let Some(event) = event_stream.next().await {
            match event {
                PeerEvent::Connected(id) => assert_eq!(id, peer1),
                _ => panic!("Expected Connected event"),
            }
        }

        // Remove peer - should generate disconnection event
        handler.remove_peer(peer1);

        // Check for disconnection event
        if let Some(event) = event_stream.next().await {
            match event {
                PeerEvent::Disconnected(id) => assert_eq!(id, peer1),
                _ => panic!("Expected Disconnected event"),
            }
        }
    }

    #[tokio::test]
    async fn test_mock_network_receive_from_specific_peer() {
        let handler = MockNetworkHandler::new();
        let peer1 = Uuid::new_v4();
        let peer2 = Uuid::new_v4();
        let message1 = b"message from peer1".to_vec();
        let message2 = b"message from peer2".to_vec();

        // Simulate messages from different peers
        handler.simulate_message_from_peer(peer1, message1.clone());
        handler.simulate_message_from_peer(peer2, message2.clone());
        handler.simulate_message_from_peer(peer1, b"another message from peer1".to_vec());

        // Receive from specific peer
        let msg = handler.receive_from(peer1).await.unwrap();
        assert_eq!(msg, message1);

        let msg = handler.receive_from(peer2).await.unwrap();
        assert_eq!(msg, message2);

        // Should still have one message from peer1
        let msg = handler.receive_from(peer1).await.unwrap();
        assert_eq!(msg, b"another message from peer1".to_vec());

        // No more messages from peer1
        let result = handler.receive_from(peer1).await;
        assert!(result.is_err());
    }
}

//! WebSocket network handler for browser-compatible communication
//!
//! Provides WebSocket-based network communication for browser compatibility
//! and firewall-friendly fallback. Integrates with aura-transport WebSocket implementation.

use async_trait::async_trait;
use aura_core::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use aura_core::DeviceId;
use aura_transport::{
    WebSocketConfig, WebSocketConnection, WebSocketEnvelope, WebSocketServer, WebSocketTransport,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// WebSocket network handler using aura-transport WebSocket implementation
pub struct WebSocketNetworkHandler {
    device_id: DeviceId,
    transport: WebSocketTransport,
    connections: Arc<RwLock<HashMap<Uuid, WebSocketConnection>>>,
    server: Arc<Mutex<Option<WebSocketServer>>>,
    event_sender: Arc<Mutex<Option<mpsc::UnboundedSender<PeerEvent>>>>,
}

impl WebSocketNetworkHandler {
    /// Create a new WebSocket network handler
    pub fn new(device_id: DeviceId, config: WebSocketConfig) -> Self {
        let transport = WebSocketTransport::new(device_id.clone(), config);

        Self {
            device_id,
            transport,
            connections: Arc::new(RwLock::new(HashMap::new())),
            server: Arc::new(Mutex::new(None)),
            event_sender: Arc::new(Mutex::new(None)),
        }
    }

    /// Start WebSocket server for incoming connections
    pub async fn start_server(&self) -> Result<(), NetworkError> {
        let server = self.transport.start_server().await.map_err(|e| {
            NetworkError::ConnectionFailed(format!("Failed to start WebSocket server: {}", e))
        })?;

        *self.server.lock().await = Some(server);

        tracing::info!(
            device_id = %self.device_id.0,
            "WebSocket server started"
        );

        Ok(())
    }

    /// Connect to a remote WebSocket server as client
    pub async fn connect_client(&self, url: &str, addr: SocketAddr) -> Result<Uuid, NetworkError> {
        let mut connection = self
            .transport
            .connect_client(url, addr)
            .await
            .map_err(|e| {
                NetworkError::ConnectionFailed(format!("Failed to connect to WebSocket: {}", e))
            })?;

        // Generate a UUID for this connection
        let peer_id = Uuid::new_v4();

        // Store the connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(peer_id, connection);
        }

        // Notify connection established
        if let Some(sender) = &*self.event_sender.lock().await {
            let _ = sender.send(PeerEvent::Connected(peer_id));
        }

        tracing::info!(
            peer_id = %peer_id,
            url = %url,
            "WebSocket client connected"
        );

        Ok(peer_id)
    }

    /// Accept incoming connections (call this repeatedly)
    pub async fn accept_connection(&self) -> Result<Uuid, NetworkError> {
        let mut server_guard = self.server.lock().await;
        let server = server_guard.as_mut().ok_or_else(|| {
            NetworkError::ConnectionFailed("WebSocket server not started".to_string())
        })?;

        let connection = server.accept().await.map_err(|e| {
            NetworkError::ConnectionFailed(format!("Failed to accept WebSocket connection: {}", e))
        })?;

        // Generate a UUID for this connection
        let peer_id = Uuid::new_v4();

        // Store the connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(peer_id, connection);
        }

        // Notify connection established
        if let Some(sender) = &*self.event_sender.lock().await {
            let _ = sender.send(PeerEvent::Connected(peer_id));
        }

        tracing::info!(
            peer_id = %peer_id,
            "WebSocket connection accepted"
        );

        Ok(peer_id)
    }

    /// Remove a peer connection
    pub async fn disconnect_peer(&self, peer_id: Uuid) -> Result<(), NetworkError> {
        let mut connections = self.connections.write().await;
        if let Some(mut connection) = connections.remove(&peer_id) {
            let _ = connection.close().await; // Best effort close

            // Notify disconnection
            if let Some(sender) = &*self.event_sender.lock().await {
                let _ = sender.send(PeerEvent::Disconnected(peer_id));
            }

            tracing::info!(
                peer_id = %peer_id,
                "WebSocket peer disconnected"
            );
        }

        Ok(())
    }

    /// Convert Uuid to DeviceId (for WebSocket envelope addressing)
    fn uuid_to_device_id(&self, uuid: Uuid) -> DeviceId {
        DeviceId::from_uuid(uuid)
    }

    /// Convert DeviceId to Uuid (best effort parsing)
    fn device_id_to_uuid(&self, device_id: &DeviceId) -> Result<Uuid, NetworkError> {
        Ok(device_id.0)
    }
}

#[async_trait]
impl NetworkEffects for WebSocketNetworkHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        let mut connections = self.connections.write().await;
        let connection = connections.get_mut(&peer_id).ok_or_else(|| {
            NetworkError::ConnectionFailed(format!("Peer not connected: {}", peer_id))
        })?;

        let target_device_id = self.uuid_to_device_id(peer_id);
        connection
            .send(target_device_id, &message)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: format!("Failed to send WebSocket message: {}", e),
            })?;

        tracing::debug!(
            peer_id = %peer_id,
            message_size = message.len(),
            "Sent WebSocket message to peer"
        );

        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peer_ids: Vec<Uuid> = {
            let connections = self.connections.read().await;
            connections.keys().copied().collect()
        };

        for peer_id in peer_ids {
            if let Err(e) = self.send_to_peer(peer_id, message.clone()).await {
                tracing::warn!(
                    peer_id = %peer_id,
                    error = %e,
                    "Failed to send broadcast message to peer"
                );
                // Continue broadcasting to other peers
            }
        }

        tracing::debug!(
            message_size = message.len(),
            "Broadcast WebSocket message to all peers"
        );

        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // Try receiving from any connected peer
        let peer_ids: Vec<Uuid> = {
            let connections = self.connections.read().await;
            connections.keys().copied().collect()
        };

        if peer_ids.is_empty() {
            return Err(NetworkError::NoMessage);
        }

        // Round-robin through connections looking for messages
        for peer_id in peer_ids {
            let mut connections = self.connections.write().await;
            if let Some(connection) = connections.get_mut(&peer_id) {
                // Try to receive without blocking (timeout of 0)
                tokio::select! {
                    result = connection.receive() => {
                        match result {
                            Ok(envelope) => {
                                tracing::debug!(
                                    peer_id = %peer_id,
                                    message_size = envelope.payload.len(),
                                    "Received WebSocket message from peer"
                                );
                                return Ok((peer_id, envelope.payload));
                            }
                            Err(_) => {
                                // Connection might be closed, continue to next peer
                                continue;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(1)) => {
                        // Short timeout, try next peer
                        continue;
                    }
                }
            }
        }

        Err(NetworkError::NoMessage)
    }

    async fn receive_from(&self, peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        let mut connections = self.connections.write().await;
        let connection = connections.get_mut(&peer_id).ok_or_else(|| {
            NetworkError::ConnectionFailed(format!("Peer not connected: {}", peer_id))
        })?;

        let envelope = connection
            .receive()
            .await
            .map_err(|e| NetworkError::ReceiveFailed {
                reason: format!("Failed to receive WebSocket message: {}", e),
            })?;

        tracing::debug!(
            peer_id = %peer_id,
            message_size = envelope.payload.len(),
            "Received WebSocket message from specific peer"
        );

        Ok(envelope.payload)
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        let connections = self.connections.read().await;
        connections.keys().copied().collect()
    }

    async fn is_peer_connected(&self, peer_id: Uuid) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(&peer_id)
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        let (sender, receiver) = mpsc::unbounded_channel();
        *self.event_sender.lock().await = Some(sender);

        Ok(Box::pin(
            tokio_stream::wrappers::UnboundedReceiverStream::new(receiver),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_handler_creation() {
        let device_id = DeviceId::from("test_device");
        let config = WebSocketConfig::default();
        let handler = WebSocketNetworkHandler::new(device_id.clone(), config);

        assert_eq!(handler.device_id, device_id);
        assert!(handler.connected_peers().await.is_empty());
    }

    #[tokio::test]
    async fn test_uuid_device_id_conversion() {
        let device_id = DeviceId::from("test_device");
        let config = WebSocketConfig::default();
        let handler = WebSocketNetworkHandler::new(device_id, config);

        let uuid = Uuid::new_v4();
        let device_id = handler.uuid_to_device_id(uuid);
        let converted_uuid = handler.device_id_to_uuid(&device_id).unwrap();

        assert_eq!(uuid, converted_uuid);
    }
}

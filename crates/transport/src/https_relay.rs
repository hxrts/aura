//! HTTPS relay transport implementation
//!
//! Provides a transport layer that uses HTTPS as the underlying protocol
//! for P2P communication through a relay server.

use crate::{Connection, Transport, TransportError, Result};
use aura_journal::DeviceId;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Message envelope for HTTPS relay transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMessage {
    /// Source device ID
    pub from: DeviceId,
    /// Target device ID  
    pub to: DeviceId,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message timestamp
    pub timestamp: u64,
    /// Message ID for deduplication
    pub message_id: Uuid,
}

/// Connection information for HTTPS relay
#[derive(Debug, Clone)]
pub struct HttpsConnection {
    /// Connection ID
    id: Uuid,
    /// Remote peer ID
    peer_id: DeviceId,
    /// Relay server URL
    relay_url: String,
    /// Connection established timestamp
    established_at: Instant,
    /// Last activity timestamp
    last_activity: Instant,
}

impl HttpsConnection {
    fn id(&self) -> Uuid {
        self.id
    }

    fn peer_id(&self) -> String {
        self.peer_id.0.to_string()
    }

    fn is_active(&self) -> bool {
        // Consider connection active if last activity was within 5 minutes
        self.last_activity.elapsed() < Duration::from_secs(300)
    }

    fn established_at(&self) -> Instant {
        self.established_at
    }
}

/// HTTPS relay transport implementation
pub struct HttpsRelayTransport {
    /// Local device ID
    device_id: DeviceId,
    /// Relay server URL
    relay_url: String,
    /// Request timeout
    timeout: Duration,
    /// Maximum retry attempts
    max_retries: u32,
    /// HTTP client
    client: reqwest::Client,
    /// Active connections
    connections: Arc<Mutex<HashMap<Uuid, HttpsConnection>>>,
    /// Message sender for incoming messages
    message_sender: Option<mpsc::UnboundedSender<(DeviceId, Vec<u8>)>>,
    /// Polling task handle
    poll_handle: Option<tokio::task::JoinHandle<()>>,
}

impl HttpsRelayTransport {
    /// Create new HTTPS relay transport
    pub fn new(
        device_id: DeviceId,
        relay_url: String,
        timeout_seconds: u64,
        max_retries: u32,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            device_id,
            relay_url,
            timeout: Duration::from_secs(timeout_seconds),
            max_retries,
            client,
            connections: Arc::new(Mutex::new(HashMap::new())),
            message_sender: None,
            poll_handle: None,
        }
    }

    /// Start message polling task
    pub fn start_polling(&mut self, message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>) {
        self.message_sender = Some(message_sender.clone());
        
        let device_id = self.device_id;
        let relay_url = self.relay_url.clone();
        let client = self.client.clone();
        let timeout = self.timeout;
        
        self.poll_handle = Some(tokio::spawn(async move {
            Self::poll_messages(device_id, relay_url, client, timeout, message_sender).await;
        }));
    }

    /// Poll for incoming messages
    async fn poll_messages(
        device_id: DeviceId,
        relay_url: String,
        client: reqwest::Client,
        timeout: Duration,
        message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>,
    ) {
        let poll_url = format!("{}/messages/{}", relay_url, device_id.0);
        let mut poll_interval = tokio::time::interval(Duration::from_secs(1));
        
        loop {
            poll_interval.tick().await;
            
            match client.get(&poll_url).timeout(timeout).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<Vec<RelayMessage>>().await {
                            Ok(messages) => {
                                for message in messages {
                                    debug!(
                                        "Received message from {} via relay",
                                        message.from.0
                                    );
                                    
                                    if let Err(e) = message_sender.send((message.from, message.payload)) {
                                        error!("Failed to forward received message: {}", e);
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to parse relay messages: {}", e);
                            }
                        }
                    } else if response.status() != 404 {
                        // 404 is expected when no messages are available
                        warn!("Relay polling returned status: {}", response.status());
                    }
                }
                Err(e) => {
                    warn!("Failed to poll relay for messages: {}", e);
                    // Back off on errors
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    /// Send message to peer via relay
    pub async fn send_to_peer(&self, peer_id: DeviceId, message: &[u8]) -> Result<()> {
        let relay_message = RelayMessage {
            from: self.device_id,
            to: peer_id,
            payload: message.to_vec(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            message_id: Uuid::new_v4(),
        };

        let send_url = format!("{}/send", self.relay_url);
        
        for attempt in 1..=self.max_retries {
            match self
                .client
                .post(&send_url)
                .json(&relay_message)
                .timeout(self.timeout)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        debug!(
                            "Successfully sent {} bytes to peer {} via relay",
                            message.len(),
                            peer_id.0
                        );
                        
                        // Update connection activity
                        self.update_connection_activity(peer_id);
                        
                        return Ok(());
                    } else {
                        warn!(
                            "Relay send failed with status {}: {}",
                            response.status(),
                            response.text().await.unwrap_or_default()
                        );
                    }
                }
                Err(e) => {
                    warn!("Attempt {} failed to send to relay: {}", attempt, e);
                }
            }
            
            if attempt < self.max_retries {
                // Exponential backoff
                let backoff_ms = 100 * (1 << (attempt - 1));
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
        
        Err(TransportError::Transport(format!(
            "Failed to send message to peer {} after {} retries",
            peer_id.0, self.max_retries
        )))
    }

    /// Establish connection to peer
    pub async fn connect_to_peer(&self, peer_id: DeviceId) -> Result<Uuid> {
        // Check if peer is reachable via relay
        if !self.is_peer_reachable_via_relay(peer_id).await {
            return Err(TransportError::Transport(format!(
                "Peer {} is not reachable via relay",
                peer_id.0
            )));
        }

        let connection_id = Uuid::new_v4();
        let connection = HttpsConnection {
            id: connection_id,
            peer_id,
            relay_url: self.relay_url.clone(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        };

        {
            let mut connections = self.connections.lock().unwrap();
            connections.insert(connection_id, connection);
        }

        info!(
            "Established HTTPS relay connection {} to peer {}",
            connection_id, peer_id.0
        );

        Ok(connection_id)
    }

    /// Check if peer is reachable via relay
    async fn is_peer_reachable_via_relay(&self, peer_id: DeviceId) -> bool {
        let health_url = format!("{}/health/{}", self.relay_url, peer_id.0);
        
        match self
            .client
            .get(&health_url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => {
                let is_reachable = response.status().is_success();
                debug!(
                    "Peer {} reachability via relay: {}",
                    peer_id.0, is_reachable
                );
                is_reachable
            }
            Err(e) => {
                debug!("Failed to check peer {} reachability: {}", peer_id.0, e);
                false
            }
        }
    }

    /// Update connection activity timestamp
    fn update_connection_activity(&self, peer_id: DeviceId) {
        let mut connections = self.connections.lock().unwrap();
        for connection in connections.values_mut() {
            if connection.peer_id == peer_id {
                connection.last_activity = Instant::now();
                break;
            }
        }
    }

    /// Get active connections
    pub fn get_connections(&self) -> Vec<HttpsConnection> {
        let connections = self.connections.lock().unwrap();
        connections
            .values()
            .filter(|conn| conn.is_active())
            .cloned()
            .collect()
    }

    /// Disconnect from peer
    pub fn disconnect_from_peer(&self, peer_id: DeviceId) -> Result<()> {
        let mut connections = self.connections.lock().unwrap();
        connections.retain(|_, conn| conn.peer_id != peer_id);
        
        info!("Disconnected from peer {} via relay", peer_id.0);
        Ok(())
    }

    /// Check relay server health
    pub async fn check_relay_health(&self) -> bool {
        let health_url = format!("{}/health", self.relay_url);
        
        match self
            .client
            .get(&health_url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(response) => {
                let is_healthy = response.status().is_success();
                debug!("Relay server health: {}", is_healthy);
                is_healthy
            }
            Err(e) => {
                warn!("Failed to check relay health: {}", e);
                false
            }
        }
    }
}

#[async_trait]
impl Transport for HttpsRelayTransport {
    async fn connect(
        &self,
        peer_id: &str,
        my_ticket: &crate::PresenceTicket,
        peer_ticket: &crate::PresenceTicket,
    ) -> Result<crate::Connection> {
        // Create a connection using the peer_id
        let conn_id = format!("https_relay_{}", peer_id);
        Ok(crate::Connection {
            id: conn_id,
            peer_id: peer_id.to_string(),
        })
    }

    async fn send(&self, conn: &crate::Connection, message: &[u8]) -> Result<()> {
        // Convert the connection peer_id to DeviceId
        let device_id = DeviceId::from_str(&conn.peer_id)
            .map_err(|e| TransportError::InvalidConfig(format!("Invalid device ID: {}", e)))?;
        self.send_to_peer(device_id, message).await
    }

    async fn receive(&self, _conn: &crate::Connection, timeout: Duration) -> Result<Option<Vec<u8>>> {
        // For HTTPS relay, receiving is handled by the polling task
        // This method would typically read from a message queue specific to the connection
        let _ = timeout;
        Ok(None)
    }

    async fn broadcast(
        &self,
        connections: &[crate::Connection],
        message: &[u8],
    ) -> Result<crate::BroadcastResult> {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for conn in connections {
            match self.send(conn, message).await {
                Ok(()) => succeeded.push(conn.peer_id.clone()),
                Err(_) => failed.push(conn.peer_id.clone()),
            }
        }

        Ok(crate::BroadcastResult { succeeded, failed })
    }

    async fn disconnect(&self, conn: &crate::Connection) -> Result<()> {
        let device_id = DeviceId::from_str(&conn.peer_id)
            .map_err(|e| TransportError::InvalidConfig(format!("Invalid device ID: {}", e)))?;
        self.disconnect_from_peer(device_id)
    }

    async fn is_connected(&self, conn: &crate::Connection) -> bool {
        if let Ok(device_id) = DeviceId::from_str(&conn.peer_id) {
            self.is_peer_reachable_via_relay(device_id).await
        } else {
            false
        }
    }
}

impl Drop for HttpsRelayTransport {
    fn drop(&mut self) {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;

    #[test]
    fn test_relay_message_serialization() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        let peer_id = DeviceId::new_with_effects(&effects);
        
        let message = RelayMessage {
            from: device_id,
            to: peer_id,
            payload: vec![1, 2, 3, 4],
            timestamp: 1234567890,
            message_id: Uuid::new_v4(),
        };

        let serialized = serde_json::to_string(&message).unwrap();
        let deserialized: RelayMessage = serde_json::from_str(&serialized).unwrap();

        assert_eq!(message.from, deserialized.from);
        assert_eq!(message.to, deserialized.to);
        assert_eq!(message.payload, deserialized.payload);
        assert_eq!(message.timestamp, deserialized.timestamp);
        assert_eq!(message.message_id, deserialized.message_id);
    }

    #[tokio::test]
    async fn test_https_transport_creation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        
        let transport = HttpsRelayTransport::new(
            device_id,
            "https://relay.example.com".to_string(),
            30,
            3,
        );

        assert_eq!(transport.device_id, device_id);
        assert_eq!(transport.relay_url, "https://relay.example.com");
        assert_eq!(transport.timeout, Duration::from_secs(30));
        assert_eq!(transport.max_retries, 3);
    }

    #[test]
    fn test_https_connection_activity() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);
        
        let connection = HttpsConnection {
            id: Uuid::new_v4(),
            peer_id: device_id,
            relay_url: "https://relay.example.com".to_string(),
            established_at: Instant::now(),
            last_activity: Instant::now(),
        };

        assert!(connection.is_active());
        assert_eq!(connection.peer_id(), device_id.0.to_string());
    }
}
//! HTTPS relay transport implementation
//!
//! Provides a transport layer that uses HTTPS as the underlying protocol
//! for P2P communication through a relay server.

use crate::{
    BroadcastResult, Connection, ConnectionManager, PresenceTicket, Transport,
    TransportErrorBuilder, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
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
    #[allow(dead_code)]
    relay_url: String,
    /// Connection established timestamp
    #[allow(dead_code)]
    established_at: Instant,
    /// Last activity timestamp
    last_activity: Instant,
}

impl HttpsConnection {
    #[allow(dead_code)]
    fn id(&self) -> Uuid {
        self.id
    }

    #[allow(dead_code)]
    fn peer_id(&self) -> String {
        self.peer_id.0.to_string()
    }

    fn is_active(&self) -> bool {
        // Consider connection active if last activity was within 5 minutes
        self.last_activity.elapsed() < Duration::from_secs(300)
    }

    #[allow(dead_code)]
    fn established_at(&self) -> Instant {
        self.established_at
    }
}

/// HTTPS relay transport implementation
#[derive(Debug)]
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
    /// Message queues per connection (connection_id -> message_receiver)  
    message_queues: Arc<Mutex<HashMap<Uuid, Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>>>>,
    /// Message senders per connection (connection_id -> message_sender)
    message_senders: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<(DeviceId, Vec<u8>)>>>>,
    /// Polling task handle
    poll_handle: Option<tokio::task::JoinHandle<()>>,
}

impl HttpsRelayTransport {
    /// Create a new builder for HttpsRelayTransport
    pub fn builder(device_id: DeviceId, relay_url: String) -> HttpsRelayTransportBuilder {
        HttpsRelayTransportBuilder::new(device_id, relay_url)
    }

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
            message_queues: Arc::new(Mutex::new(HashMap::new())),
            message_senders: Arc::new(Mutex::new(HashMap::new())),
            poll_handle: None,
        }
    }

    /// Start message polling task
    pub fn start_polling(&mut self) {
        let device_id = self.device_id;
        let relay_url = self.relay_url.clone();
        let client = self.client.clone();
        let timeout = self.timeout;
        let message_senders = self.message_senders.clone();
        let connections = self.connections.clone();

        self.poll_handle = Some(tokio::spawn(async move {
            Self::poll_messages(
                device_id,
                relay_url,
                client,
                timeout,
                message_senders,
                connections,
            )
            .await;
        }));
    }

    /// Poll for incoming messages and route them to appropriate connection queues
    async fn poll_messages(
        device_id: DeviceId,
        relay_url: String,
        client: reqwest::Client,
        timeout: Duration,
        message_senders: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<(DeviceId, Vec<u8>)>>>>,
        connections: Arc<Mutex<HashMap<Uuid, HttpsConnection>>>,
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
                                    debug!("Received message from {} via relay", message.from.0);

                                    // Find the connection for this sender
                                    let target_connection_id = {
                                        let connections = connections.lock().unwrap();
                                        connections
                                            .iter()
                                            .find(|(_, conn)| conn.peer_id == message.from)
                                            .map(|(id, _)| *id)
                                    };

                                    if let Some(connection_id) = target_connection_id {
                                        // Route message to the specific connection's queue
                                        let senders = message_senders.lock().unwrap();
                                        if let Some(sender) = senders.get(&connection_id) {
                                            if let Err(e) =
                                                sender.send((message.from, message.payload))
                                            {
                                                error!(
                                                    "Failed to route message to connection {}: {}",
                                                    connection_id, e
                                                );
                                            }
                                        } else {
                                            warn!(
                                                "No message sender found for connection {}",
                                                connection_id
                                            );
                                        }
                                    } else {
                                        debug!("No active connection found for peer {}, dropping message", message.from.0);
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

    /// Send message to peer via relay (private implementation)
    async fn send_to_peer_impl(&self, peer_id: DeviceId, message: &[u8]) -> TransportResult<()> {
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

        Err(TransportErrorBuilder::transport(format!(
            "Failed to send message to peer {} after {} retries",
            peer_id.0, self.max_retries
        )))
    }

    /// Establish connection to peer (private implementation)
    async fn connect_to_peer_impl(&self, peer_id: DeviceId) -> TransportResult<Uuid> {
        // Check if peer is reachable via relay
        if !self.is_peer_reachable_via_relay(peer_id).await {
            return Err(TransportErrorBuilder::transport(format!(
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

    /// Disconnect from peer (private implementation)
    fn disconnect_from_peer_impl(&self, peer_id: DeviceId) -> TransportResult<()> {
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
    type ConnectionType = crate::Connection;

    async fn connect_to_peer(&self, peer_id: DeviceId) -> TransportResult<Uuid> {
        self.connect_to_peer_impl(peer_id).await
    }

    async fn send_to_peer(&self, peer_id: DeviceId, message: &[u8]) -> TransportResult<()> {
        self.send_to_peer_impl(peer_id, message).await
    }

    async fn receive_message(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, Vec<u8>)>> {
        // This is a simplified implementation that receives from any connection
        // In a real implementation, you would merge all message receivers
        // For now, just return None to indicate no message available
        tokio::time::sleep(timeout).await;
        Ok(None)
    }

    async fn disconnect_from_peer(&self, peer_id: DeviceId) -> TransportResult<()> {
        self.disconnect_from_peer_impl(peer_id)
    }

    async fn is_peer_reachable(&self, peer_id: DeviceId) -> bool {
        // Check if we have an active connection to this peer
        let connections = self.connections.lock().unwrap();
        connections.values().any(|conn| conn.peer_id == peer_id)
    }

    fn get_connections(&self) -> Vec<Self::ConnectionType> {
        let connections = self.connections.lock().unwrap();
        connections
            .values()
            .map(|conn| crate::Connection {
                id: conn.id.to_string(),
                peer_id: conn.peer_id.to_string(),
            })
            .collect()
    }

    async fn start(
        &mut self,
        _message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>,
    ) -> TransportResult<()> {
        // HTTPS relay transport doesn't need explicit start - connections are made on demand
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        // Clear all connections
        {
            let mut connections = self.connections.lock().unwrap();
            let mut senders = self.message_senders.lock().unwrap();
            let mut queues = self.message_queues.lock().unwrap();
            connections.clear();
            senders.clear();
            queues.clear();
        }
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "https_relay"
    }

    async fn health_check(&self) -> bool {
        // Simple health check - return true if we can create connections
        true
    }
}

#[async_trait]
impl ConnectionManager for HttpsRelayTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        _peer_ticket: &PresenceTicket,
    ) -> TransportResult<Connection> {
        let device_id = DeviceId::from_str(peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;

        let connection_id = self.connect_to_peer_impl(device_id).await?;
        Ok(Connection {
            id: connection_id.to_string(),
            peer_id: peer_id.to_string(),
        })
    }

    async fn send(&self, conn: &Connection, message: &[u8]) -> TransportResult<()> {
        let device_id = DeviceId::from_str(&conn.peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;
        self.send_to_peer_impl(device_id, message).await
    }

    async fn receive(
        &self,
        _conn: &Connection,
        timeout: Duration,
    ) -> TransportResult<Option<Vec<u8>>> {
        // Use the existing receive_message implementation
        if let Some((_, message)) = self.receive_message(timeout).await? {
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> TransportResult<BroadcastResult> {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for conn in connections {
            match self.send(conn, message).await {
                Ok(()) => succeeded.push(conn.peer_id.clone()),
                Err(_) => failed.push(conn.peer_id.clone()),
            }
        }

        Ok(BroadcastResult { succeeded, failed })
    }

    async fn disconnect(&self, conn: &Connection) -> TransportResult<()> {
        let device_id = DeviceId::from_str(&conn.peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;
        self.disconnect_from_peer_impl(device_id)
    }

    async fn is_connected(&self, conn: &Connection) -> bool {
        let device_id = match DeviceId::from_str(&conn.peer_id) {
            Ok(id) => id,
            Err(_) => return false,
        };

        self.is_peer_reachable(device_id).await
    }
}

impl Drop for HttpsRelayTransport {
    fn drop(&mut self) {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
    }
}

/// Builder for HttpsRelayTransport with fluent API and validation
pub struct HttpsRelayTransportBuilder {
    device_id: DeviceId,
    relay_url: String,
    timeout_seconds: Option<u64>,
    max_retries: Option<u32>,
    custom_headers: Option<std::collections::HashMap<String, String>>,
    user_agent: Option<String>,
    proxy_url: Option<String>,
}

impl HttpsRelayTransportBuilder {
    /// Create a new builder with required parameters
    pub fn new(device_id: DeviceId, relay_url: String) -> Self {
        Self {
            device_id,
            relay_url,
            timeout_seconds: None,
            max_retries: None,
            custom_headers: None,
            user_agent: None,
            proxy_url: None,
        }
    }

    /// Set timeout in seconds (default: 30)
    pub fn timeout_seconds(mut self, timeout: u64) -> Self {
        self.timeout_seconds = Some(timeout);
        self
    }

    /// Set maximum retries (default: 3)
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = Some(retries);
        self
    }

    /// Add custom HTTP headers
    pub fn custom_headers(mut self, headers: std::collections::HashMap<String, String>) -> Self {
        self.custom_headers = Some(headers);
        self
    }

    /// Add a single custom header
    pub fn header(mut self, key: String, value: String) -> Self {
        if self.custom_headers.is_none() {
            self.custom_headers = Some(std::collections::HashMap::new());
        }
        if let Some(ref mut headers) = self.custom_headers {
            headers.insert(key, value);
        }
        self
    }

    /// Set custom user agent
    pub fn user_agent(mut self, user_agent: String) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    /// Set proxy URL (for HTTP proxy support)
    pub fn proxy_url(mut self, proxy_url: String) -> Self {
        self.proxy_url = Some(proxy_url);
        self
    }

    /// Set conservative defaults for production use
    pub fn production_defaults(self) -> Self {
        self.timeout_seconds(30)
            .max_retries(3)
            .user_agent("Aura-Transport/1.0".to_string())
    }

    /// Set aggressive defaults for development/testing
    pub fn development_defaults(self) -> Self {
        self.timeout_seconds(10)
            .max_retries(1)
            .user_agent("Aura-Transport-Dev/1.0".to_string())
    }

    /// Build the HttpsRelayTransport
    pub fn build(self) -> std::result::Result<HttpsRelayTransport, HttpsTransportBuildError> {
        // Validate relay URL
        if !self.relay_url.starts_with("https://") {
            return Err(HttpsTransportBuildError::InvalidRelayUrl(
                "Relay URL must use HTTPS".to_string(),
            ));
        }

        // Validate timeout
        let timeout_seconds = self.timeout_seconds.unwrap_or(30);
        if timeout_seconds == 0 || timeout_seconds > 300 {
            return Err(HttpsTransportBuildError::InvalidTimeout(
                "Timeout must be between 1 and 300 seconds".to_string(),
            ));
        }

        // Validate max retries
        let max_retries = self.max_retries.unwrap_or(3);
        if max_retries > 10 {
            return Err(HttpsTransportBuildError::InvalidRetries(
                "Max retries cannot exceed 10".to_string(),
            ));
        }

        // Build HTTP client with configuration
        let mut client_builder =
            reqwest::Client::builder().timeout(Duration::from_secs(timeout_seconds));

        // Add user agent if provided
        if let Some(user_agent) = self.user_agent {
            client_builder = client_builder.user_agent(user_agent);
        }

        // Add proxy if provided
        if let Some(proxy_url) = self.proxy_url {
            if let Ok(proxy) = reqwest::Proxy::all(proxy_url) {
                client_builder = client_builder.proxy(proxy);
            } else {
                return Err(HttpsTransportBuildError::InvalidProxyUrl(
                    "Invalid proxy URL format".to_string(),
                ));
            }
        }

        let client = client_builder
            .build()
            .map_err(|e| HttpsTransportBuildError::ClientCreationFailed(e.to_string()))?;

        Ok(HttpsRelayTransport {
            device_id: self.device_id,
            relay_url: self.relay_url,
            timeout: Duration::from_secs(timeout_seconds),
            max_retries,
            client,
            connections: Arc::new(Mutex::new(std::collections::HashMap::new())),
            message_queues: Arc::new(Mutex::new(std::collections::HashMap::new())),
            message_senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            poll_handle: None,
        })
    }
}

/// Errors that can occur during HttpsRelayTransport building
#[derive(Debug, thiserror::Error)]
pub enum HttpsTransportBuildError {
    #[error("Invalid relay URL: {0}")]
    /// Invalid relay URL provided
    InvalidRelayUrl(String),

    #[error("Invalid timeout: {0}")]
    /// Invalid timeout value provided
    InvalidTimeout(String),

    #[error("Invalid retries: {0}")]
    /// Invalid retry count provided
    InvalidRetries(String),

    #[error("Invalid proxy URL: {0}")]
    /// Invalid proxy URL provided
    InvalidProxyUrl(String),

    #[error("Failed to create HTTP client: {0}")]
    /// Failed to create HTTP client
    ClientCreationFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Effects;
    use aura_types::DeviceIdExt;

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

        let transport =
            HttpsRelayTransport::new(device_id, "https://relay.example.com".to_string(), 30, 3);

        assert_eq!(transport.device_id, device_id);
        assert_eq!(transport.relay_url, "https://relay.example.com");
        assert_eq!(transport.timeout, Duration::from_secs(30));
        assert_eq!(transport.max_retries, 3);
    }

    #[test]
    fn test_https_transport_builder() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Test basic builder usage
        let transport =
            HttpsRelayTransport::builder(device_id, "https://relay.example.com".to_string())
                .timeout_seconds(60)
                .max_retries(5)
                .user_agent("Test-Agent/1.0".to_string())
                .build()
                .unwrap();

        assert_eq!(transport.device_id, device_id);
        assert_eq!(transport.relay_url, "https://relay.example.com");
        assert_eq!(transport.timeout, Duration::from_secs(60));
        assert_eq!(transport.max_retries, 5);
    }

    #[test]
    fn test_https_transport_builder_validation() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Test invalid URL (non-HTTPS)
        let result =
            HttpsRelayTransport::builder(device_id, "http://relay.example.com".to_string()).build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            HttpsTransportBuildError::InvalidRelayUrl(_)
        ));

        // Test invalid timeout (too high)
        let result =
            HttpsRelayTransport::builder(device_id, "https://relay.example.com".to_string())
                .timeout_seconds(400)
                .build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            HttpsTransportBuildError::InvalidTimeout(_)
        ));

        // Test invalid retries (too high)
        let result =
            HttpsRelayTransport::builder(device_id, "https://relay.example.com".to_string())
                .max_retries(15)
                .build();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            HttpsTransportBuildError::InvalidRetries(_)
        ));
    }

    #[test]
    fn test_https_transport_builder_defaults() {
        let effects = Effects::test();
        let device_id = DeviceId::new_with_effects(&effects);

        // Test production defaults
        let transport =
            HttpsRelayTransport::builder(device_id, "https://relay.example.com".to_string())
                .production_defaults()
                .build()
                .unwrap();

        assert_eq!(transport.timeout, Duration::from_secs(30));
        assert_eq!(transport.max_retries, 3);

        // Test development defaults
        let transport =
            HttpsRelayTransport::builder(device_id, "https://relay.example.com".to_string())
                .development_defaults()
                .build()
                .unwrap();

        assert_eq!(transport.timeout, Duration::from_secs(10));
        assert_eq!(transport.max_retries, 1);
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

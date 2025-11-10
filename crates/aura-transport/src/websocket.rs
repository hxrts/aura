//! WebSocket Transport Implementation
//!
//! Provides WebSocket transport for browser compatibility and firewall-friendly fallback.
//! Clean, minimal implementation following the "zero legacy code" principle.

use aura_core::{AuraError, DeviceId};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, client_async, tungstenite::Message, WebSocketStream};
use tracing;

/// WebSocket message envelope for framing CBOR-encoded data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketEnvelope {
    /// Source device ID
    pub from: DeviceId,
    /// Destination device ID  
    pub to: DeviceId,
    /// CBOR-encoded message payload
    pub payload: Vec<u8>,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was sent (seconds since UNIX epoch)
    pub timestamp: u64,
}

/// WebSocket transport configuration
#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    /// Bind address for server mode
    pub bind_addr: String,
    /// Port to listen on
    pub port: u16,
    /// Maximum message size in bytes (16KB default for fixed-size envelopes)
    pub max_message_size: usize,
    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0".to_string(),
            port: 8081,
            max_message_size: 16 * 1024, // 16KB for privacy-friendly fixed-size envelopes
            timeout_ms: 10_000,
        }
    }
}

/// WebSocket transport implementation
pub struct WebSocketTransport {
    device_id: DeviceId,
    config: WebSocketConfig,
    sequence_counter: u64,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub fn new(device_id: DeviceId, config: WebSocketConfig) -> Self {
        Self {
            device_id,
            config,
            sequence_counter: 0,
        }
    }

    /// Get this device's ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get configuration
    pub fn config(&self) -> &WebSocketConfig {
        &self.config
    }

    /// Start WebSocket server
    pub async fn start_server(&self) -> Result<WebSocketServer, AuraError> {
        let bind_addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            AuraError::coordination_failed(format!(
                "Failed to bind WebSocket server to {}: {}",
                bind_addr, e
            ))
        })?;

        tracing::info!(
            device_id = %self.device_id.0,
            bind_addr = %bind_addr,
            "Started WebSocket server"
        );

        Ok(WebSocketServer {
            listener,
            device_id: self.device_id,
            config: self.config.clone(),
        })
    }

    /// Connect to WebSocket server as client
    pub async fn connect_client(
        &self,
        url: &str,
        addr: SocketAddr,
    ) -> Result<WebSocketConnection, AuraError> {
        let tcp_stream = TcpStream::connect(addr).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to connect to TCP socket: {}", e))
        })?;

        let (ws_stream, _) = client_async(url, tcp_stream).await.map_err(|e| {
            AuraError::coordination_failed(format!("WebSocket handshake failed: {}", e))
        })?;

        tracing::info!(
            device_id = %self.device_id.0,
            url = %url,
            "Connected WebSocket client"
        );

        Ok(WebSocketConnection {
            stream: ws_stream,
            device_id: self.device_id,
            peer_id: None,
            config: self.config.clone(),
            sequence: 0,
        })
    }

    /// Get next sequence number
    fn next_sequence(&mut self) -> u64 {
        self.sequence_counter += 1;
        self.sequence_counter
    }
}

/// WebSocket server for accepting connections
pub struct WebSocketServer {
    listener: TcpListener,
    device_id: DeviceId,
    config: WebSocketConfig,
}

impl WebSocketServer {
    /// Accept next incoming connection
    pub async fn accept(&mut self) -> Result<WebSocketConnection, AuraError> {
        let (stream, addr) = self.listener.accept().await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to accept WebSocket connection: {}", e))
        })?;

        let ws_stream = accept_async(stream).await.map_err(|e| {
            AuraError::coordination_failed(format!("WebSocket handshake failed: {}", e))
        })?;

        tracing::info!(
            device_id = %self.device_id.0,
            peer_addr = %addr,
            "Accepted WebSocket connection"
        );

        Ok(WebSocketConnection {
            stream: ws_stream,
            device_id: self.device_id,
            peer_id: None,
            config: self.config.clone(),
            sequence: 0,
        })
    }
}

/// WebSocket connection for bidirectional communication
pub struct WebSocketConnection {
    stream: WebSocketStream<TcpStream>,
    device_id: DeviceId,
    peer_id: Option<DeviceId>,
    config: WebSocketConfig,
    sequence: u64,
}

impl WebSocketConnection {
    /// Send message over WebSocket
    pub async fn send(&mut self, to: DeviceId, payload: &[u8]) -> Result<(), AuraError> {
        if payload.len() > self.config.max_message_size {
            return Err(AuraError::coordination_failed(format!(
                "Message too large: {} > {}",
                payload.len(),
                self.config.max_message_size
            )));
        }

        self.sequence += 1;
        let envelope = WebSocketEnvelope {
            from: self.device_id,
            to,
            payload: payload.to_vec(),
            sequence: self.sequence,
            timestamp: current_timestamp(),
        };

        let serialized = serde_cbor::to_vec(&envelope).map_err(|e| {
            AuraError::coordination_failed(format!("Failed to serialize WebSocket envelope: {}", e))
        })?;

        self.stream
            .send(Message::Binary(serialized))
            .await
            .map_err(|e| {
                AuraError::coordination_failed(format!("Failed to send WebSocket message: {}", e))
            })?;

        tracing::debug!(
            from = %self.device_id.0,
            to = %to.0,
            sequence = self.sequence,
            payload_size = payload.len(),
            "Sent WebSocket message"
        );

        Ok(())
    }

    /// Receive message from WebSocket
    pub async fn receive(&mut self) -> Result<WebSocketEnvelope, AuraError> {
        loop {
            let msg = self.stream.next().await.ok_or_else(|| {
                AuraError::coordination_failed("WebSocket connection closed".to_string())
            })?;

            let msg = msg.map_err(|e| {
                AuraError::coordination_failed(format!("WebSocket receive error: {}", e))
            })?;

            match msg {
                Message::Binary(data) => {
                    if data.len() > self.config.max_message_size {
                        tracing::warn!(
                            size = data.len(),
                            max_size = self.config.max_message_size,
                            "Received oversized WebSocket message, dropping"
                        );
                        continue;
                    }

                    let envelope: WebSocketEnvelope =
                        serde_cbor::from_slice(&data).map_err(|e| {
                            AuraError::coordination_failed(format!(
                                "Failed to deserialize WebSocket envelope: {}",
                                e
                            ))
                        })?;

                    // Update peer ID on first message
                    if self.peer_id.is_none() {
                        self.peer_id = Some(envelope.from);
                        tracing::debug!(
                            peer_id = %envelope.from.0,
                            "Identified WebSocket peer"
                        );
                    }

                    tracing::debug!(
                        from = %envelope.from.0,
                        to = %envelope.to.0,
                        sequence = envelope.sequence,
                        payload_size = envelope.payload.len(),
                        "Received WebSocket message"
                    );

                    return Ok(envelope);
                }
                Message::Text(_) => {
                    tracing::warn!("Received text WebSocket message, expected binary");
                    continue;
                }
                Message::Ping(data) => {
                    self.stream.send(Message::Pong(data)).await.map_err(|e| {
                        AuraError::coordination_failed(format!(
                            "Failed to send WebSocket pong: {}",
                            e
                        ))
                    })?;
                    continue;
                }
                Message::Pong(_) => {
                    // Ignore pong messages
                    continue;
                }
                Message::Close(_) => {
                    return Err(AuraError::coordination_failed(
                        "WebSocket connection closed by peer".to_string(),
                    ));
                }
                Message::Frame(_) => {
                    // Ignore raw frames
                    continue;
                }
            }
        }
    }

    /// Get peer device ID (if identified)
    pub fn peer_id(&self) -> Option<DeviceId> {
        self.peer_id
    }

    /// Check if connection is still alive
    pub async fn is_alive(&mut self) -> bool {
        // Send a ping and wait for response (simple liveness check)
        if let Err(_) = self.stream.send(Message::Ping(vec![])).await {
            return false;
        }
        true
    }

    /// Close the connection gracefully
    pub async fn close(&mut self) -> Result<(), AuraError> {
        self.stream.send(Message::Close(None)).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to close WebSocket: {}", e))
        })?;

        tracing::debug!(
            device_id = %self.device_id.0,
            peer_id = ?self.peer_id.map(|id| id.0),
            "Closed WebSocket connection"
        );

        Ok(())
    }
}

/// Get current timestamp as seconds since UNIX epoch
fn current_timestamp() -> u64 {
    std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_envelope_serialization() {
        let device1 = DeviceId("device1".to_string());
        let device2 = DeviceId("device2".to_string());

        let envelope = WebSocketEnvelope {
            from: device1,
            to: device2,
            payload: b"test message".to_vec(),
            sequence: 1,
            timestamp: current_timestamp(),
        };

        let serialized = serde_cbor::to_vec(&envelope).expect("Serialization failed");
        let deserialized: WebSocketEnvelope =
            serde_cbor::from_slice(&serialized).expect("Deserialization failed");

        assert_eq!(envelope.from, deserialized.from);
        assert_eq!(envelope.to, deserialized.to);
        assert_eq!(envelope.payload, deserialized.payload);
        assert_eq!(envelope.sequence, deserialized.sequence);
    }

    #[tokio::test]
    async fn test_websocket_config_defaults() {
        let config = WebSocketConfig::default();
        assert_eq!(config.bind_addr, "0.0.0.0");
        assert_eq!(config.port, 8081);
        assert_eq!(config.max_message_size, 16 * 1024);
        assert_eq!(config.timeout_ms, 10_000);
    }

    #[tokio::test]
    async fn test_websocket_transport_creation() {
        let device_id = DeviceId("test_device".to_string());
        let config = WebSocketConfig::default();
        let transport = WebSocketTransport::new(device_id.clone(), config);

        assert_eq!(transport.device_id(), device_id);
        assert_eq!(transport.config().port, 8081);
    }
}

//! WebSocket Transport Handler
//!
//! Stateless WebSocket transport implementation using tungstenite.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tungstenite ecosystem.

use super::{TransportConfig, TransportConnection, TransportError, TransportResult};
use async_trait::async_trait;
use aura_core::effects::NetworkEffects;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, client_async, tungstenite::Message, WebSocketStream};
use url::Url;

/// WebSocket transport handler implementation
#[derive(Debug, Clone)]
pub struct WebSocketTransportHandler {
    config: TransportConfig,
}

impl WebSocketTransportHandler {
    /// Create new WebSocket transport handler
    pub fn new(config: TransportConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Connect to WebSocket server
    pub async fn connect(
        &self,
        url: Url,
    ) -> TransportResult<(WebSocketStream<TcpStream>, TransportConnection)> {
        let (ws_stream, response) = timeout(
            self.config.connect_timeout,
            client_async(
                url.clone(),
                TcpStream::connect(url.socket_addrs(|| None)?[0]).await?,
            ),
        )
        .await
        .map_err(|_| TransportError::Timeout("WebSocket connect timeout".to_string()))?
        .map_err(|e| {
            TransportError::ConnectionFailed(format!("WebSocket connect failed: {}", e))
        })?;

        let local_addr = ws_stream.get_ref().local_addr()?.to_string();
        let remote_addr = ws_stream.get_ref().peer_addr()?.to_string();
        let connection_id = format!("ws-{}-{}", local_addr, remote_addr);

        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "websocket".to_string());
        metadata.insert("url".to_string(), url.to_string());
        metadata.insert("status".to_string(), response.status().to_string());

        if let Some(subprotocol) = response.headers().get("sec-websocket-protocol") {
            metadata.insert(
                "subprotocol".to_string(),
                subprotocol.to_str().unwrap_or("").to_string(),
            );
        }

        let connection = TransportConnection {
            connection_id,
            local_addr,
            remote_addr,
            metadata,
        };

        Ok((ws_stream, connection))
    }

    /// Accept incoming WebSocket connection
    pub async fn accept(
        &self,
        stream: TcpStream,
    ) -> TransportResult<(WebSocketStream<TcpStream>, TransportConnection)> {
        let local_addr = stream.local_addr()?.to_string();
        let remote_addr = stream.peer_addr()?.to_string();

        let ws_stream = timeout(self.config.connect_timeout, accept_async(stream))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket accept timeout".to_string()))?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket accept failed: {}", e))
            })?;

        let connection_id = format!("ws-{}-{}", local_addr, remote_addr);

        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "websocket".to_string());
        metadata.insert("role".to_string(), "server".to_string());

        let connection = TransportConnection {
            connection_id,
            local_addr,
            remote_addr,
            metadata,
        };

        Ok((ws_stream, connection))
    }

    /// Send message over WebSocket
    pub async fn send(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
        data: &[u8],
    ) -> TransportResult<()> {
        let message = Message::Binary(data.to_vec());

        timeout(self.config.write_timeout, ws_stream.send(message))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket send timeout".to_string()))?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket send failed: {}", e))
            })?;

        Ok(())
    }

    /// Receive message from WebSocket
    pub async fn receive(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
    ) -> TransportResult<Vec<u8>> {
        let message = timeout(self.config.read_timeout, ws_stream.next())
            .await
            .map_err(|_| TransportError::Timeout("WebSocket receive timeout".to_string()))?
            .ok_or_else(|| {
                TransportError::ConnectionFailed("WebSocket connection closed".to_string())
            })?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket receive failed: {}", e))
            })?;

        match message {
            Message::Binary(data) => Ok(data),
            Message::Text(text) => Ok(text.into_bytes()),
            Message::Close(_) => Err(TransportError::ConnectionFailed(
                "WebSocket connection closed by peer".to_string(),
            )),
            Message::Ping(data) => {
                // Auto-respond to ping with pong
                ws_stream
                    .send(Message::Pong(data.clone()))
                    .await
                    .map_err(|e| {
                        TransportError::ConnectionFailed(format!("WebSocket pong failed: {}", e))
                    })?;
                // Return ping data
                Ok(data)
            }
            Message::Pong(_) => {
                // Ignore pong messages
                self.receive(ws_stream).await
            }
            Message::Frame(_) => Err(TransportError::Protocol(
                "Unexpected WebSocket frame".to_string(),
            )),
        }
    }

    /// Send text message over WebSocket
    pub async fn send_text(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
        text: &str,
    ) -> TransportResult<()> {
        let message = Message::Text(text.to_string());

        timeout(self.config.write_timeout, ws_stream.send(message))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket send text timeout".to_string()))?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket send text failed: {}", e))
            })?;

        Ok(())
    }

    /// Close WebSocket connection gracefully
    pub async fn close(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
        reason: Option<String>,
    ) -> TransportResult<()> {
        let close_frame = reason
            .map(|r| {
                Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
                    code:
                        tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                    reason: r.into(),
                }))
            })
            .unwrap_or(Message::Close(None));

        ws_stream.send(close_frame).await.map_err(|e| {
            TransportError::ConnectionFailed(format!("WebSocket close failed: {}", e))
        })?;

        Ok(())
    }
}

#[async_trait]
impl NetworkEffects for WebSocketTransportHandler {
    type Error = TransportError;
    type PeerId = Url;

    async fn send_to_peer(&self, peer_id: Self::PeerId, data: Vec<u8>) -> Result<(), Self::Error> {
        let (mut ws_stream, _connection) = self.connect(peer_id).await?;
        self.send(&mut ws_stream, &data).await?;
        self.close(&mut ws_stream, Some("Message sent".to_string()))
            .await?;
        Ok(())
    }

    async fn broadcast(&self, peers: Vec<Self::PeerId>, data: Vec<u8>) -> Result<(), Self::Error> {
        // Simple sequential broadcast - real implementation would use concurrent connections
        for peer in peers {
            self.send_to_peer(peer, data.clone()).await?;
        }
        Ok(())
    }
}

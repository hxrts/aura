//! WebSocket Transport Handler
//!
//! Stateless WebSocket transport implementation using tungstenite.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tungstenite ecosystem.

use super::{
    ConnectionId, TransportAddress, TransportConfig, TransportConnection, TransportError,
    TransportMetadata, TransportResult, TransportSocketAddr, TransportUrl,
};
use async_trait::async_trait;
use aura_core::effects::{
    NetworkCoreEffects, NetworkError, NetworkExtendedEffects, PeerEvent, PeerEventStream,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::{accept_async, client_async, tungstenite::Message, WebSocketStream};
use url::Url;
use uuid::Uuid;

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
    #[allow(clippy::should_implement_trait)] // Method provides default config, not implementing Default trait
    pub fn default() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Connect to WebSocket server
    pub async fn connect(
        &self,
        url: TransportUrl,
    ) -> TransportResult<(WebSocketStream<TcpStream>, TransportConnection)> {
        let url_ref = url.as_url();
        let (ws_stream, response) = timeout(
            self.config.connect_timeout.get(),
            client_async(
                url.as_str(),
                TcpStream::connect(url_ref.socket_addrs(|| None)?[0]).await?,
            ),
        )
        .await
        .map_err(|_| TransportError::Timeout("WebSocket connect timeout".to_string()))?
        .map_err(|e| TransportError::ConnectionFailed(format!("WebSocket connect failed: {e}")))?;

        let local_addr =
            TransportAddress::from(TransportSocketAddr::from(ws_stream.get_ref().local_addr()?));
        let remote_addr =
            TransportAddress::from(TransportSocketAddr::from(ws_stream.get_ref().peer_addr()?));
        let connection_id = ConnectionId::new(format!("ws-{local_addr}-{remote_addr}"));

        let subprotocol = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|header| header.to_str().ok())
            .map(|value| value.to_string());
        let metadata = TransportMetadata::websocket_client(
            url.clone(),
            response.status().as_u16(),
            subprotocol,
        );

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
        let local_addr = TransportAddress::from(TransportSocketAddr::from(stream.local_addr()?));
        let remote_addr = TransportAddress::from(TransportSocketAddr::from(stream.peer_addr()?));

        let ws_stream = timeout(self.config.connect_timeout.get(), accept_async(stream))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket accept timeout".to_string()))?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket accept failed: {e}"))
            })?;

        let connection_id = ConnectionId::new(format!("ws-{local_addr}-{remote_addr}"));

        let metadata = TransportMetadata::websocket_server();

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

        timeout(self.config.write_timeout.get(), ws_stream.send(message))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket send timeout".to_string()))?
            .map_err(|e| TransportError::ConnectionFailed(format!("WebSocket send failed: {e}")))?;

        Ok(())
    }

    /// Receive message from WebSocket
    pub async fn receive(
        &self,
        ws_stream: &mut WebSocketStream<TcpStream>,
    ) -> TransportResult<Vec<u8>> {
        let message = timeout(self.config.read_timeout.get(), ws_stream.next())
            .await
            .map_err(|_| TransportError::Timeout("WebSocket receive timeout".to_string()))?
            .ok_or_else(|| {
                TransportError::ConnectionFailed("WebSocket connection closed".to_string())
            })?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket receive failed: {e}"))
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
                        TransportError::ConnectionFailed(format!("WebSocket pong failed: {e}"))
                    })?;
                // Return ping data
                Ok(data)
            }
            Message::Pong(_) => {
                // Ignore pong messages and try to receive the next message
                Box::pin(self.receive(ws_stream)).await
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

        timeout(self.config.write_timeout.get(), ws_stream.send(message))
            .await
            .map_err(|_| TransportError::Timeout("WebSocket send text timeout".to_string()))?
            .map_err(|e| {
                TransportError::ConnectionFailed(format!("WebSocket send text failed: {e}"))
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
            TransportError::ConnectionFailed(format!("WebSocket close failed: {e}"))
        })?;

        Ok(())
    }
}

#[async_trait]
impl NetworkExtendedEffects for WebSocketTransportHandler {
    async fn open(&self, endpoint: &str) -> Result<String, NetworkError> {
        let url: Url = endpoint
            .parse()
            .map_err(|e: url::ParseError| NetworkError::ConnectionFailed(e.to_string()))?;
        let (ws_stream, _connection) = self
            .connect(TransportUrl::from(url))
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        // Track the connection by a deterministic id (hash of endpoint)
        let conn_id =
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, endpoint.as_bytes()).to_string();
        // Store in memory map? Handler is stateless by design; return a token so caller can map.
        // We just drop the stream here; production should manage connection lifecycle externally.
        drop(ws_stream);
        Ok(conn_id)
    }

    async fn send(&self, connection_id: &str, data: Vec<u8>) -> Result<(), NetworkError> {
        // For stateless WebSocket, connection_id is treated as endpoint URL
        let url: Url =
            connection_id
                .parse()
                .map_err(|e: url::ParseError| NetworkError::SendFailed {
                    peer_id: None,
                    reason: e.to_string(),
                })?;
        let (mut ws_stream, _connection) = self
            .connect(TransportUrl::from(url))
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        WebSocketTransportHandler::send(self, &mut ws_stream, &data)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: e.to_string(),
            })?;
        WebSocketTransportHandler::close(self, &mut ws_stream, Some("completed".to_string()))
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: e.to_string(),
            })?;
        Ok(())
    }

    async fn close(&self, connection_id: &str) -> Result<(), NetworkError> {
        // For stateless WebSocket, connection_id is treated as endpoint URL
        let url: Url = connection_id
            .parse()
            .map_err(|e: url::ParseError| NetworkError::ConnectionFailed(e.to_string()))?;
        let (mut ws_stream, _connection) = self
            .connect(TransportUrl::from(url))
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        WebSocketTransportHandler::close(self, &mut ws_stream, Some("closed".into()))
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }
    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Err(NetworkError::ReceiveFailed {
            reason: "WebSocket receive_from requires connection management".to_string(),
        })
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        // Stateless WebSocket handler doesn't track connections
        Vec::new()
    }

    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        use futures::stream;
        use std::pin::Pin;

        let stream = stream::empty::<PeerEvent>();
        Ok(Pin::from(Box::new(stream)))
    }
}

#[async_trait]
impl NetworkCoreEffects for WebSocketTransportHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        // Convert UUID to WebSocket URL - simplified mapping
        let url_str = format!("ws://localhost:8080/{peer_id}");
        let url: Url = url_str.parse().map_err(|e| NetworkError::SendFailed {
            peer_id: Some(peer_id),
            reason: format!("Invalid WebSocket URL: {e}"),
        })?;

        let (mut ws_stream, _connection) = self
            .connect(TransportUrl::from(url))
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        self.send(&mut ws_stream, &message)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: e.to_string(),
            })?;
        self.close(&mut ws_stream, Some("Message sent".to_string()))
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: Some(peer_id),
                reason: e.to_string(),
            })?;
        Ok(())
    }

    async fn broadcast(&self, message: Vec<u8>) -> Result<(), NetworkError> {
        let peers = self.connected_peers().await;
        for peer in peers {
            // Ignore individual failures in broadcast
            let _ = self.send_to_peer(peer, message.clone()).await;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        Err(NetworkError::ReceiveFailed {
            reason: "WebSocket receive requires connection management".to_string(),
        })
    }
}

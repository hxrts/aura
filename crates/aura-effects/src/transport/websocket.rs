//! WebSocket Transport Handler
//!
//! Stateless WebSocket transport implementation using tungstenite.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tungstenite ecosystem.

use super::{
    utils::TimeoutHelper, ConnectionId, TransportAddress, TransportConfig, TransportConnection,
    TransportError, TransportMetadata, TransportResult, TransportSocketAddr, TransportUrl,
};
use async_trait::async_trait;
use aura_core::effects::{
    NetworkCoreEffects, NetworkError, NetworkExtendedEffects, PeerEvent, PeerEventStream,
};
use futures_util::{SinkExt, StreamExt};
use std::io;
use std::net::{IpAddr, SocketAddr};
use tokio::net::{lookup_host, TcpStream};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{
    accept_async, client_async,
    tungstenite::{self, Message},
    WebSocketStream,
};
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

    fn is_retryable_connect_error(error: &io::Error) -> bool {
        matches!(
            error.kind(),
            io::ErrorKind::ConnectionRefused
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::NotConnected
                | io::ErrorKind::AddrNotAvailable
                | io::ErrorKind::TimedOut
                | io::ErrorKind::Interrupted
                | io::ErrorKind::WouldBlock
                | io::ErrorKind::HostUnreachable
                | io::ErrorKind::NetworkUnreachable
        )
    }

    fn connect_retry_delay(&self, attempt: usize) -> std::time::Duration {
        let delay = TimeoutHelper::exponential_backoff(
            attempt as u32,
            self.config.connect_retry_base_delay.get(),
            self.config.connect_retry_max_delay.get(),
        );
        TimeoutHelper::add_jitter(delay, 20)
    }

    async fn resolve_socket_addr(
        &self,
        url: &TransportUrl,
    ) -> Result<SocketAddr, WebSocketConnectError> {
        let url_ref = url.as_url();
        let host = url_ref
            .host_str()
            .ok_or_else(|| WebSocketConnectError::Protocol("Missing host in URL".to_string()))?;
        let port = url_ref
            .port_or_known_default()
            .ok_or_else(|| WebSocketConnectError::Protocol("Missing port in URL".to_string()))?;

        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(SocketAddr::new(ip, port));
        }

        let mut addresses = timeout(self.config.dns_timeout.get(), lookup_host((host, port)))
            .await
            .map_err(|_| WebSocketConnectError::DnsTimeout {
                host: host.to_string(),
                port,
            })?
            .map_err(WebSocketConnectError::Dns)?;

        addresses
            .next()
            .ok_or_else(|| WebSocketConnectError::NoAddresses {
                host: host.to_string(),
                port,
            })
    }

    async fn connect_once(
        &self,
        url: &TransportUrl,
    ) -> Result<
        (
            WebSocketStream<TcpStream>,
            tungstenite::handshake::client::Response,
        ),
        WebSocketConnectError,
    > {
        let addr = self.resolve_socket_addr(url).await?;
        let stream = timeout(self.config.connect_timeout.get(), TcpStream::connect(addr))
            .await
            .map_err(|_| WebSocketConnectError::ConnectTimeout { addr })?
            .map_err(WebSocketConnectError::ConnectIo)?;

        timeout(
            self.config.connect_timeout.get(),
            client_async(url.as_str(), stream),
        )
        .await
        .map_err(|_| WebSocketConnectError::HandshakeTimeout)?
        .map_err(WebSocketConnectError::Handshake)
    }

    async fn connect_with_retry(
        &self,
        url: &TransportUrl,
    ) -> TransportResult<(
        WebSocketStream<TcpStream>,
        tungstenite::handshake::client::Response,
    )> {
        let attempts = self.config.connect_retry_attempts.get();
        let mut last_error = None;

        for attempt in 0..attempts {
            match self.connect_once(url).await {
                Ok(connection) => return Ok(connection),
                Err(error) => {
                    let retryable = error.is_retryable();
                    last_error = Some(error);
                    if !retryable || attempt + 1 == attempts {
                        break;
                    }
                    sleep(self.connect_retry_delay(attempt)).await;
                }
            }
        }

        Err(last_error
            .unwrap_or(WebSocketConnectError::HandshakeTimeout)
            .into_transport_error())
    }

    /// Connect to WebSocket server
    pub async fn connect(
        &self,
        url: TransportUrl,
    ) -> TransportResult<(WebSocketStream<TcpStream>, TransportConnection)> {
        let (ws_stream, response) = self.connect_with_retry(&url).await?;

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

#[derive(Debug)]
enum WebSocketConnectError {
    Protocol(String),
    DnsTimeout { host: String, port: u16 },
    NoAddresses { host: String, port: u16 },
    Dns(io::Error),
    ConnectTimeout { addr: SocketAddr },
    ConnectIo(io::Error),
    HandshakeTimeout,
    Handshake(tungstenite::Error),
}

impl WebSocketConnectError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::DnsTimeout { .. } | Self::ConnectTimeout { .. } | Self::HandshakeTimeout => true,
            Self::Dns(error) | Self::ConnectIo(error) => {
                WebSocketTransportHandler::is_retryable_connect_error(error)
            }
            Self::Handshake(tungstenite::Error::Io(error)) => {
                WebSocketTransportHandler::is_retryable_connect_error(error)
            }
            Self::Protocol(_) | Self::NoAddresses { .. } | Self::Handshake(_) => false,
        }
    }

    fn into_transport_error(self) -> TransportError {
        match self {
            Self::Protocol(reason) => TransportError::Protocol(reason),
            Self::DnsTimeout { host, port } => {
                TransportError::Timeout(format!("WebSocket DNS lookup timeout for {host}:{port}"))
            }
            Self::NoAddresses { host, port } => TransportError::ConnectionFailed(format!(
                "WebSocket DNS lookup returned no addresses for {host}:{port}"
            )),
            Self::Dns(error) => {
                TransportError::ConnectionFailed(format!("WebSocket DNS lookup failed: {error}"))
            }
            Self::ConnectTimeout { addr } => {
                TransportError::Timeout(format!("WebSocket TCP connect timeout for {addr}"))
            }
            Self::ConnectIo(error) => {
                TransportError::ConnectionFailed(format!("WebSocket TCP connect failed: {error}"))
            }
            Self::HandshakeTimeout => {
                TransportError::Timeout("WebSocket handshake timeout".to_string())
            }
            Self::Handshake(error) => {
                TransportError::ConnectionFailed(format!("WebSocket connect failed: {error}"))
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio::time::Duration;

    fn test_config() -> TransportConfig {
        TransportConfig {
            connect_timeout: super::super::NonZeroDuration::from_millis(75)
                .expect("connect timeout"),
            dns_timeout: super::super::NonZeroDuration::from_millis(50).expect("dns timeout"),
            read_timeout: super::super::NonZeroDuration::from_millis(75).expect("read timeout"),
            write_timeout: super::super::NonZeroDuration::from_millis(75).expect("write timeout"),
            connect_retry_attempts: std::num::NonZeroUsize::new(4).expect("retry attempts"),
            connect_retry_base_delay: super::super::NonZeroDuration::from_millis(30)
                .expect("retry base delay"),
            connect_retry_max_delay: super::super::NonZeroDuration::from_millis(30)
                .expect("retry max delay"),
            buffer_size: std::num::NonZeroUsize::new(4096).expect("buffer size"),
        }
    }

    #[tokio::test]
    async fn websocket_connect_retries_transient_connection_refusals() {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
        let addr = socket.local_addr().expect("reserved addr");
        drop(socket);

        let server = async move {
            sleep(Duration::from_millis(35)).await;
            let listener = TcpListener::bind(addr)
                .await
                .expect("bind delayed websocket listener");
            let (stream, _) = listener.accept().await.expect("accept websocket client");
            let _ = accept_async(stream)
                .await
                .expect("complete websocket handshake after retry");
        };

        let handler = WebSocketTransportHandler::new(test_config());
        let url = Url::parse(&format!("ws://127.0.0.1:{}/retry", addr.port()))
            .expect("retry websocket url");

        let client = async move {
            handler
                .connect(TransportUrl::from(url))
                .await
                .expect("websocket connect should retry until listener is ready")
        };
        let (_, (_stream, connection)) = tokio::join!(server, client);

        assert_eq!(
            connection.metadata.protocol,
            super::super::TransportProtocol::WebSocket
        );
    }
}

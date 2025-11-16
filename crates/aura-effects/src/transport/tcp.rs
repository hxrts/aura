//! TCP Transport Handler
//!
//! Stateless TCP transport implementation using tokio.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tokio ecosystem.

use super::{TransportConfig, TransportConnection, TransportError, TransportResult};
use async_trait::async_trait;
use aura_core::effects::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;
use uuid::Uuid;

/// TCP transport handler implementation
#[derive(Debug, Clone)]
pub struct TcpTransportHandler {
    config: TransportConfig,
}

impl TcpTransportHandler {
    /// Create new TCP transport handler
    pub fn new(config: TransportConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(TransportConfig::default())
    }

    /// Connect to remote peer via TCP
    pub async fn connect(&self, addr: SocketAddr) -> TransportResult<TransportConnection> {
        let stream = timeout(self.config.connect_timeout, TcpStream::connect(addr))
            .await
            .map_err(|_| TransportError::Timeout("TCP connect timeout".to_string()))?
            .map_err(|e| TransportError::ConnectionFailed(format!("TCP connect failed: {}", e)))?;

        let local_addr = stream.local_addr()?.to_string();
        let remote_addr = stream.peer_addr()?.to_string();

        let connection_id = format!("tcp-{}-{}", local_addr, remote_addr);

        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "tcp".to_string());
        metadata.insert("nodelay".to_string(), "true".to_string());

        // Configure TCP socket
        stream.set_nodelay(true)?;

        Ok(TransportConnection {
            connection_id,
            local_addr,
            remote_addr,
            metadata,
        })
    }

    /// Listen for incoming TCP connections
    pub async fn listen(&self, bind_addr: SocketAddr) -> TransportResult<TcpListener> {
        let listener = TcpListener::bind(bind_addr)
            .await
            .map_err(|e| TransportError::ConnectionFailed(format!("TCP bind failed: {}", e)))?;

        Ok(listener)
    }

    /// Accept incoming connection
    pub async fn accept(
        &self,
        listener: &TcpListener,
    ) -> TransportResult<(TcpStream, TransportConnection)> {
        let (stream, peer_addr) = listener
            .accept()
            .await
            .map_err(|e| TransportError::ConnectionFailed(format!("TCP accept failed: {}", e)))?;

        let local_addr = stream.local_addr()?.to_string();
        let remote_addr = peer_addr.to_string();
        let connection_id = format!("tcp-{}-{}", local_addr, remote_addr);

        let mut metadata = HashMap::new();
        metadata.insert("protocol".to_string(), "tcp".to_string());
        metadata.insert("nodelay".to_string(), "true".to_string());

        // Configure TCP socket
        stream.set_nodelay(true)?;

        let connection = TransportConnection {
            connection_id,
            local_addr,
            remote_addr,
            metadata,
        };

        Ok((stream, connection))
    }

    /// Send data over TCP stream
    pub async fn send(&self, stream: &mut TcpStream, data: &[u8]) -> TransportResult<usize> {
        let _ = timeout(self.config.write_timeout, stream.write_all(data))
            .await
            .map_err(|_| TransportError::Timeout("TCP write timeout".to_string()))?
            .map_err(TransportError::Io)?;

        stream.flush().await.map_err(TransportError::Io)?;
        Ok(data.len())
    }

    /// Receive data from TCP stream
    pub async fn receive(&self, stream: &mut TcpStream) -> TransportResult<Vec<u8>> {
        let mut buffer = vec![0u8; self.config.buffer_size];

        let bytes_read = timeout(self.config.read_timeout, stream.read(&mut buffer))
            .await
            .map_err(|_| TransportError::Timeout("TCP read timeout".to_string()))?
            .map_err(TransportError::Io)?;

        if bytes_read == 0 {
            return Err(TransportError::ConnectionFailed(
                "TCP connection closed".to_string(),
            ));
        }

        buffer.truncate(bytes_read);
        Ok(buffer)
    }

    /// Send framed message (length-prefixed)
    pub async fn send_framed(&self, stream: &mut TcpStream, data: &[u8]) -> TransportResult<usize> {
        let len = data.len() as u32;
        let len_bytes = len.to_be_bytes();

        // Send length prefix
        self.send(stream, &len_bytes).await?;
        // Send data
        self.send(stream, data).await?;

        Ok(4 + data.len())
    }

    /// Receive framed message (length-prefixed)
    pub async fn receive_framed(&self, stream: &mut TcpStream) -> TransportResult<Vec<u8>> {
        // Read 4-byte length prefix
        let mut len_bytes = [0u8; 4];
        timeout(self.config.read_timeout, stream.read_exact(&mut len_bytes))
            .await
            .map_err(|_| TransportError::Timeout("TCP read length timeout".to_string()))?
            .map_err(TransportError::Io)?;

        let len = u32::from_be_bytes(len_bytes) as usize;

        // Validate message length
        if len > self.config.buffer_size {
            return Err(TransportError::Protocol(format!(
                "Message too large: {} > {}",
                len, self.config.buffer_size
            )));
        }

        // Read message data
        let mut data = vec![0u8; len];
        timeout(self.config.read_timeout, stream.read_exact(&mut data))
            .await
            .map_err(|_| TransportError::Timeout("TCP read data timeout".to_string()))?
            .map_err(TransportError::Io)?;

        Ok(data)
    }
}

#[async_trait]
impl NetworkEffects for TcpTransportHandler {
    async fn send_to_peer(&self, peer_id: Uuid, message: Vec<u8>) -> Result<(), NetworkError> {
        // Convert UUID to socket address - this is a simplified implementation
        // In practice, you'd need a proper peer discovery/registry system
        let addr_str = format!("127.0.0.1:{}", peer_id.as_u128() % 65535 + 1024);
        let addr: SocketAddr = addr_str.parse().map_err(|e| NetworkError::SendFailed {
            peer_id: Some(peer_id),
            reason: format!("Invalid address: {}", e),
        })?;

        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        self.send_framed(&mut stream, &message)
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
            // Ignore individual send failures in broadcast
            let _ = self.send_to_peer(peer, message.clone()).await;
        }
        Ok(())
    }

    async fn receive(&self) -> Result<(Uuid, Vec<u8>), NetworkError> {
        // TCP receiving requires a persistent connection and listener
        // This is a placeholder for stateless implementation
        Err(NetworkError::ReceiveFailed {
            reason: "TCP receive requires connection management".to_string(),
        })
    }

    async fn receive_from(&self, _peer_id: Uuid) -> Result<Vec<u8>, NetworkError> {
        Err(NetworkError::ReceiveFailed {
            reason: "TCP receive_from requires connection management".to_string(),
        })
    }

    async fn connected_peers(&self) -> Vec<Uuid> {
        // TCP stateless handler doesn't maintain connection state
        Vec::new()
    }

    async fn is_peer_connected(&self, _peer_id: Uuid) -> bool {
        // Stateless TCP handler doesn't track connections
        false
    }

    async fn subscribe_to_peer_events(&self) -> Result<PeerEventStream, NetworkError> {
        use futures::stream;
        use std::pin::Pin;

        let stream = stream::empty::<PeerEvent>();
        Ok(Pin::from(Box::new(stream)))
    }
}

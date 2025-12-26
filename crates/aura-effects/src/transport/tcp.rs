//! TCP Transport Handler
//!
//! Stateless TCP transport implementation using tokio.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tokio ecosystem.

use super::{TransportConfig, TransportConnection, TransportError, TransportResult};
use async_trait::async_trait;
use aura_core::{
    effects::{NetworkCoreEffects, NetworkError, NetworkExtendedEffects, PeerEvent, PeerEventStream},
    hash,
};
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
    #[allow(clippy::should_implement_trait)] // Method provides default config, not implementing Default trait
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
        timeout(self.config.write_timeout, stream.write_all(data))
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
impl NetworkCoreEffects for TcpTransportHandler {
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
        // Bind to the configured address (or ephemeral if not provided) and
        // read a single framed message. This keeps the handler stateless while
        // still allowing inbound delivery in tests and small deployments.
        let bind_addr: SocketAddr = std::env::var("AURA_TCP_LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:0".to_string())
            .parse()
            .map_err(|e| NetworkError::ReceiveFailed {
                reason: format!("Invalid listen address: {}", e),
            })?;

        let listener =
            TcpListener::bind(bind_addr)
                .await
                .map_err(|e| NetworkError::ReceiveFailed {
                    reason: format!("Failed to bind listener: {}", e),
                })?;

        let accept_result = timeout(self.config.read_timeout, listener.accept())
            .await
            .map_err(|_| NetworkError::ReceiveFailed {
                reason: "TCP receive timed out waiting for connection".to_string(),
            })
            .and_then(|res| {
                res.map_err(|e| NetworkError::ReceiveFailed {
                    reason: format!("Failed to accept TCP connection: {}", e),
                })
            })?;

        let (mut stream, peer_addr) = accept_result;
        let peer_id = uuid_from_addr(&peer_addr);

        let mut buffer = vec![0u8; self.config.buffer_size];
        let bytes_read = timeout(self.config.read_timeout, stream.read(&mut buffer))
            .await
            .map_err(|_| NetworkError::ReceiveFailed {
                reason: "TCP receive timed out while reading".to_string(),
            })
            .and_then(|res| {
                res.map_err(|e| NetworkError::ReceiveFailed {
                    reason: format!("TCP read failed: {}", e),
                })
            })?;

        buffer.truncate(bytes_read);
        Ok((peer_id, buffer))
    }
}

#[async_trait]
impl NetworkExtendedEffects for TcpTransportHandler {

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

    async fn open(&self, address: &str) -> Result<String, NetworkError> {
        // Open a TCP connection and return a connection ID
        let addr: SocketAddr = address
            .parse()
            .map_err(|e| NetworkError::ConnectionFailed(format!("Invalid address: {}", e)))?;
        let _stream = TcpStream::connect(addr)
            .await
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        // Generate deterministic connection ID from address
        let conn_hash = hash::hash(address.as_bytes());
        let mut conn_id_bytes = [0u8; 16];
        conn_id_bytes.copy_from_slice(&conn_hash[..16]);
        Ok(Uuid::from_bytes(conn_id_bytes).to_string())
    }

    async fn send(&self, _connection_id: &str, _data: Vec<u8>) -> Result<(), NetworkError> {
        // Stateless TCP handler doesn't maintain connection state for send
        Err(NetworkError::NotImplemented)
    }

    async fn close(&self, _connection_id: &str) -> Result<(), NetworkError> {
        // Stateless TCP handler doesn't maintain connection state for close
        Ok(())
    }
}

fn uuid_from_addr(addr: &SocketAddr) -> Uuid {
    let hash_bytes = hash::hash(addr.to_string().as_bytes());
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&hash_bytes[..16]);
    Uuid::from_bytes(uuid_bytes)
}

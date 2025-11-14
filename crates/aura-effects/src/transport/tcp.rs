//! TCP Transport Handler
//!
//! Stateless TCP transport implementation using tokio.
//! NO choreography - single-party effect handler only.
//! Target: <200 lines, leverage tokio ecosystem.

use super::{TransportConfig, TransportConnection, TransportError, TransportResult};
use aura_core::effects::NetworkEffects;
use async_trait::async_trait;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

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
        let stream = timeout(
            self.config.connect_timeout,
            TcpStream::connect(addr),
        )
        .await
        .map_err(|_| TransportError::Timeout("TCP connect timeout".to_string()))?
        .map_err(|e| TransportError::ConnectionFailed(format!("TCP connect failed: {}", e)))?;

        let local_addr = stream.local_addr()?
            .to_string();
        let remote_addr = stream.peer_addr()?
            .to_string();

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

        let local_addr = stream.local_addr()?
            .to_string();
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
        let bytes_written = timeout(
            self.config.write_timeout,
            stream.write_all(data),
        )
        .await
        .map_err(|_| TransportError::Timeout("TCP write timeout".to_string()))?
        .map_err(TransportError::Io)?;

        stream.flush().await.map_err(TransportError::Io)?;
        Ok(data.len())
    }

    /// Receive data from TCP stream
    pub async fn receive(&self, stream: &mut TcpStream) -> TransportResult<Vec<u8>> {
        let mut buffer = vec![0u8; self.config.buffer_size];
        
        let bytes_read = timeout(
            self.config.read_timeout,
            stream.read(&mut buffer),
        )
        .await
        .map_err(|_| TransportError::Timeout("TCP read timeout".to_string()))?
        .map_err(TransportError::Io)?;

        if bytes_read == 0 {
            return Err(TransportError::ConnectionFailed("TCP connection closed".to_string()));
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
        timeout(
            self.config.read_timeout,
            stream.read_exact(&mut len_bytes),
        )
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
        timeout(
            self.config.read_timeout,
            stream.read_exact(&mut data),
        )
        .await
        .map_err(|_| TransportError::Timeout("TCP read data timeout".to_string()))?
        .map_err(TransportError::Io)?;
        
        Ok(data)
    }
}

#[async_trait]
impl NetworkEffects for TcpTransportHandler {
    type Error = TransportError;
    type PeerId = SocketAddr;
    
    async fn send_to_peer(&self, peer_id: Self::PeerId, data: Vec<u8>) -> Result<(), Self::Error> {
        let mut stream = TcpStream::connect(peer_id).await
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        
        self.send_framed(&mut stream, &data).await?;
        Ok(())
    }
    
    async fn broadcast(&self, _peers: Vec<Self::PeerId>, _data: Vec<u8>) -> Result<(), Self::Error> {
        // TCP doesn't support native broadcast - would need connection management
        Err(TransportError::Protocol("TCP broadcast not implemented in stateless handler".to_string()))
    }
}

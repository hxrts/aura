//! TCP transport implementation for production peer-to-peer communication

use crate::{
    core::traits::Transport, error::TransportErrorBuilder, TransportEnvelope, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Mutex, RwLock},
    time::timeout,
};

/// TCP transport for production networking
pub struct TcpTransport {
    device_id: DeviceId,
    address: String,
    port: u16,
    listener: Option<TcpListener>,
    connections: Arc<RwLock<HashMap<DeviceId, Arc<Mutex<TcpStream>>>>>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<TransportEnvelope>>>,
    _sender: mpsc::UnboundedSender<TransportEnvelope>,
    running: Arc<Mutex<bool>>,
}

impl TcpTransport {
    /// Create a new TCP transport
    pub fn new(device_id: DeviceId, address: String, port: u16) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            device_id,
            address,
            port,
            listener: None,
            connections: Arc::new(RwLock::new(HashMap::new())),
            receiver: Arc::new(Mutex::new(receiver)),
            _sender: sender,
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Start listening for incoming connections
    async fn listen(&mut self) -> TransportResult<()> {
        let bind_addr = format!("{}:{}", self.address, self.port);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            TransportErrorBuilder::connection(format!("Failed to bind to {}: {}", bind_addr, e))
        })?;

        self.listener = Some(listener);
        tracing::info!("TCP transport listening on {}", bind_addr);
        Ok(())
    }

    /// Handle incoming connection
    async fn _handle_connection(
        &self,
        stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> TransportResult<()> {
        tracing::debug!("Handling TCP connection from {}", peer_addr);

        // For now, we'll use a simple protocol:
        // - First 32 bytes: DeviceId of sender
        // - Next 4 bytes: Message length (u32, big endian)
        // - Remaining bytes: Serialized envelope

        let mut stream = stream;
        let mut device_id_bytes = [0u8; 32];

        // Read device ID
        stream.read_exact(&mut device_id_bytes).await.map_err(|e| {
            TransportErrorBuilder::protocol_error(format!("Failed to read device ID: {}", e))
        })?;

        let peer_device_id = DeviceId::from_uuid(uuid::Uuid::from_bytes_le(
            device_id_bytes[..16].try_into().unwrap_or_default(),
        ));

        // Store connection
        self.connections
            .write()
            .await
            .insert(peer_device_id, Arc::new(Mutex::new(stream)));

        tracing::info!("TCP connection established with device {}", peer_device_id);
        Ok(())
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send(&self, envelope: TransportEnvelope) -> TransportResult<()> {
        let peer_id = envelope.to;
        let connections = self.connections.read().await;

        if let Some(conn) = connections.get(&peer_id) {
            let mut stream = conn.lock().await;

            // Serialize envelope
            let serialized = serde_json::to_vec(&envelope).map_err(|e| {
                TransportErrorBuilder::protocol_error(format!(
                    "Failed to serialize envelope: {}",
                    e
                ))
            })?;

            // Send our device ID first
            stream
                .write_all(self.device_id.0.as_bytes())
                .await
                .map_err(|e| {
                    TransportErrorBuilder::transport(format!("Failed to send device ID: {}", e))
                })?;

            // Send message length
            let len = serialized.len() as u32;
            stream.write_all(&len.to_be_bytes()).await.map_err(|e| {
                TransportErrorBuilder::transport(format!("Failed to send message length: {}", e))
            })?;

            // Send serialized envelope
            stream.write_all(&serialized).await.map_err(|e| {
                TransportErrorBuilder::transport(format!("Failed to send envelope: {}", e))
            })?;

            tracing::debug!("TCP message sent to device {}", peer_id);
            Ok(())
        } else {
            Err(TransportErrorBuilder::connection(format!(
                "No TCP connection to device {}",
                peer_id
            )))
        }
    }

    async fn receive(
        &self,
        timeout_duration: Duration,
    ) -> TransportResult<Option<TransportEnvelope>> {
        let mut receiver = self.receiver.lock().await;

        match timeout(timeout_duration, receiver.recv()).await {
            Ok(Some(envelope)) => {
                tracing::debug!("TCP message received from device {}", envelope.from);
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None), // Channel closed
            Err(_) => Ok(None),   // Timeout
        }
    }

    async fn connect(&self, peer_id: DeviceId) -> TransportResult<()> {
        // For TCP, we need peer address information
        // This would typically come from peer discovery or configuration
        // For now, return an error indicating we need address information
        Err(TransportErrorBuilder::connection(format!(
            "TCP connect requires peer address information for device {}",
            peer_id
        )))
    }

    async fn disconnect(&self, peer_id: DeviceId) -> TransportResult<()> {
        let mut connections = self.connections.write().await;
        if connections.remove(&peer_id).is_some() {
            tracing::info!("TCP connection to device {} disconnected", peer_id);
        }
        Ok(())
    }

    async fn is_reachable(&self, peer_id: DeviceId) -> bool {
        self.connections.read().await.contains_key(&peer_id)
    }

    async fn start(&mut self) -> TransportResult<()> {
        {
            let mut running = self.running.lock().await;
            if *running {
                return Ok(());
            }
            *running = true;
        }

        self.listen().await?;

        tracing::info!("TCP transport started for device {}", self.device_id);
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        let mut running = self.running.lock().await;
        if !*running {
            return Ok(());
        }

        // Close all connections
        self.connections.write().await.clear();
        self.listener = None;
        *running = false;

        tracing::info!("TCP transport stopped for device {}", self.device_id);
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "tcp"
    }
}

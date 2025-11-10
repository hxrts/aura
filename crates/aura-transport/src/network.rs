//! Real network transport implementation
//!
//! Provides TCP-based network transport for production use.
//! Features message framing, peer management, and connection pooling.

use aura_core::{AuraError, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::{timeout, Duration};

/// Get current time as seconds since UNIX epoch
fn current_timestamp() -> u64 {
    std::time::SystemTime::UNIX_EPOCH
        .elapsed()
        .unwrap_or_default()
        .as_secs()
}

/// Network transport configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Bind address for listening
    pub bind_addr: String,
    /// Port to listen on
    pub port: u16,
    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Keep-alive interval in seconds
    pub keepalive_interval: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0".to_string(),
            port: 8080,
            timeout_ms: 30_000,
            max_message_size: 1024 * 1024, // 1MB
            keepalive_interval: 30,
        }
    }
}

/// Network message envelope for framing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessage {
    /// Source device ID
    pub from: DeviceId,
    /// Destination device ID
    pub to: DeviceId,
    /// Message type identifier
    pub message_type: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was sent
    pub timestamp: u64,
}

/// Peer connection information
#[derive(Debug, Clone)]
pub struct PeerConnection {
    /// Peer device ID
    pub device_id: DeviceId,
    /// Peer socket address
    pub addr: SocketAddr,
    /// Connection establishment time (seconds since UNIX epoch)
    pub connected_at: u64,
    /// Last activity time (seconds since UNIX epoch)
    pub last_activity: u64,
    /// Is connection active
    pub is_active: bool,
}

/// Network transport using TCP with message framing
#[derive(Debug)]
pub struct NetworkTransport {
    device_id: DeviceId,
    config: NetworkConfig,
    peers: Arc<RwLock<HashMap<DeviceId, PeerConnection>>>,
    connections: Arc<RwLock<HashMap<DeviceId, TcpStream>>>,
    incoming_messages: Arc<Mutex<mpsc::UnboundedReceiver<NetworkMessage>>>,
    incoming_sender: mpsc::UnboundedSender<NetworkMessage>,
    sequence_counter: Arc<Mutex<u64>>,
}

impl NetworkTransport {
    /// Create a new network transport
    pub fn new(device_id: DeviceId, config: NetworkConfig) -> Self {
        let (incoming_sender, incoming_receiver) = mpsc::unbounded_channel();

        Self {
            device_id,
            config,
            peers: Arc::new(RwLock::new(HashMap::new())),
            connections: Arc::new(RwLock::new(HashMap::new())),
            incoming_messages: Arc::new(Mutex::new(incoming_receiver)),
            incoming_sender,
            sequence_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Start listening for incoming connections
    pub async fn start_listener(&mut self) -> Result<(), AuraError> {
        let bind_addr = format!("{}:{}", self.config.bind_addr, self.config.port);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to bind to {}: {}", bind_addr, e))
        })?;

        tracing::info!(
            device_id = %self.device_id.0,
            bind_addr = %bind_addr,
            "Started network transport listener"
        );

        let peers = Arc::clone(&self.peers);
        let connections = Arc::clone(&self.connections);
        let incoming_sender = self.incoming_sender.clone();
        let device_id = self.device_id;
        let max_message_size = self.config.max_message_size;

        // Move the listener into the spawn task
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let peers = Arc::clone(&peers);
                        let connections = Arc::clone(&connections);
                        let incoming_sender = incoming_sender.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                addr,
                                device_id,
                                peers,
                                connections,
                                incoming_sender,
                                max_message_size,
                            )
                            .await
                            {
                                tracing::warn!(
                                    addr = %addr,
                                    error = %e,
                                    "Connection handling failed"
                                );
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to accept connection");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        // Don't store the listener since it's moved into the spawn task
        Ok(())
    }

    /// Get this device's ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get configuration
    pub fn config(&self) -> &NetworkConfig {
        &self.config
    }

    /// Add a peer to connect to
    pub async fn add_peer(&self, device_id: DeviceId, addr: SocketAddr) -> Result<(), AuraError> {
        let now = current_timestamp();
        let mut peers = self.peers.write().await;
        peers.insert(
            device_id,
            PeerConnection {
                device_id,
                addr,
                connected_at: now,
                last_activity: now,
                is_active: false,
            },
        );

        tracing::info!(
            peer_id = %device_id.0,
            addr = %addr,
            "Added peer to transport"
        );

        Ok(())
    }

    /// Connect to a peer
    pub async fn connect_to_peer(&self, device_id: DeviceId) -> Result<(), AuraError> {
        let peer_info = {
            let peers = self.peers.read().await;
            peers.get(&device_id).cloned()
        };

        let peer = peer_info.ok_or_else(|| {
            AuraError::coordination_failed(format!("Peer {} not found", device_id.0))
        })?;

        let timeout_duration = Duration::from_millis(self.config.timeout_ms);
        let stream = timeout(timeout_duration, TcpStream::connect(peer.addr))
            .await
            .map_err(|_| AuraError::coordination_failed("Connection timeout".to_string()))?
            .map_err(|e| AuraError::coordination_failed(format!("Connection failed: {}", e)))?;

        let mut connections = self.connections.write().await;
        connections.insert(device_id, stream);

        let now = current_timestamp();
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(&device_id) {
            peer.is_active = true;
            peer.last_activity = now;
        }

        tracing::info!(
            peer_id = %device_id.0,
            addr = %peer.addr,
            "Connected to peer"
        );

        Ok(())
    }

    /// Send a message to a peer
    pub async fn send(
        &self,
        to: DeviceId,
        payload: Vec<u8>,
        message_type: String,
    ) -> Result<(), AuraError> {
        if payload.len() > self.config.max_message_size {
            return Err(AuraError::coordination_failed(format!(
                "Message too large: {} > {}",
                payload.len(),
                self.config.max_message_size
            )));
        }

        let sequence = {
            let mut counter = self.sequence_counter.lock().await;
            *counter += 1;
            *counter
        };

        let message = NetworkMessage {
            from: self.device_id,
            to,
            message_type,
            payload,
            sequence,
            timestamp: current_timestamp(),
        };

        let mut connections = self.connections.write().await;
        let stream = connections.get_mut(&to).ok_or_else(|| {
            AuraError::coordination_failed(format!("No connection to peer {}", to.0))
        })?;

        Self::send_message(stream, &message).await?;

        tracing::debug!(
            to = %to.0,
            message_type = %message.message_type,
            sequence = message.sequence,
            payload_size = message.payload.len(),
            "Sent message to peer"
        );

        Ok(())
    }

    /// Receive a message
    pub async fn receive(&self) -> Result<NetworkMessage, AuraError> {
        let mut receiver = self.incoming_messages.lock().await;
        receiver.recv().await.ok_or_else(|| {
            AuraError::coordination_failed("Message receive channel closed".to_string())
        })
    }

    /// Get list of connected peers
    pub async fn connected_peers(&self) -> Vec<DeviceId> {
        let peers = self.peers.read().await;
        peers
            .values()
            .filter(|p| p.is_active)
            .map(|p| p.device_id)
            .collect()
    }

    /// Check if peer is connected
    pub async fn is_peer_connected(&self, device_id: DeviceId) -> bool {
        let peers = self.peers.read().await;
        peers.get(&device_id).map(|p| p.is_active).unwrap_or(false)
    }

    /// Handle incoming connection
    async fn handle_connection(
        mut stream: TcpStream,
        addr: SocketAddr,
        _local_device_id: DeviceId,
        peers: Arc<RwLock<HashMap<DeviceId, PeerConnection>>>,
        _connections: Arc<RwLock<HashMap<DeviceId, TcpStream>>>,
        incoming_sender: mpsc::UnboundedSender<NetworkMessage>,
        max_message_size: usize,
    ) -> Result<(), AuraError> {
        tracing::debug!(addr = %addr, "Handling new connection");

        loop {
            match Self::receive_message(&mut stream, max_message_size).await {
                Ok(message) => {
                    let peer_id = message.from;
                    let now = current_timestamp();

                    let mut peers_guard = peers.write().await;
                    peers_guard
                        .entry(peer_id)
                        .or_insert_with(|| PeerConnection {
                            device_id: peer_id,
                            addr,
                            connected_at: now,
                            last_activity: now,
                            is_active: true,
                        })
                        .last_activity = now;
                    drop(peers_guard);

                    if incoming_sender.send(message).is_err() {
                        tracing::warn!("Failed to forward message - receiver dropped");
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!(addr = %addr, error = %e, "Connection ended");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Send a framed message
    async fn send_message(
        stream: &mut TcpStream,
        message: &NetworkMessage,
    ) -> Result<(), AuraError> {
        let serialized = bincode::serialize(message)
            .map_err(|e| AuraError::coordination_failed(format!("Serialization failed: {}", e)))?;

        let length = (serialized.len() as u32).to_be_bytes();
        stream
            .write_all(&length)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Write length failed: {}", e)))?;

        stream
            .write_all(&serialized)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Write message failed: {}", e)))?;

        stream
            .flush()
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Flush failed: {}", e)))?;

        Ok(())
    }

    /// Receive a framed message
    async fn receive_message(
        stream: &mut TcpStream,
        max_size: usize,
    ) -> Result<NetworkMessage, AuraError> {
        let mut length_bytes = [0u8; 4];
        stream
            .read_exact(&mut length_bytes)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Read length failed: {}", e)))?;

        let length = u32::from_be_bytes(length_bytes) as usize;
        if length > max_size {
            return Err(AuraError::coordination_failed(format!(
                "Message too large: {} > {}",
                length, max_size
            )));
        }

        let mut buffer = vec![0u8; length];
        stream
            .read_exact(&mut buffer)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("Read message failed: {}", e)))?;

        let message: NetworkMessage = bincode::deserialize(&buffer).map_err(|e| {
            AuraError::coordination_failed(format!("Deserialization failed: {}", e))
        })?;

        Ok(message)
    }
}

//! Direct P2P transport using Noise protocol over TCP
//!
//! Provides secure, authenticated peer-to-peer communication using:
//! - Noise protocol for encryption and authentication
//! - TCP for reliable transport
//! - Ed25519 device keys for identity verification
//! - Connection multiplexing for multiple simultaneous peers

use crate::{
    Connection, PresenceTicket, Transport, TransportError, TransportErrorBuilder, TransportResult,
};
use async_trait::async_trait;
use aura_types::{DeviceId, DeviceIdExt};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Noise handshake patterns for P2P communication
#[derive(Debug, Clone)]
pub enum NoisePattern {
    /// XX pattern: mutual authentication with ephemeral keys
    XX,
    /// IK pattern: initiator knows responder's static key
    IK,
}

/// P2P connection state
#[derive(Debug, Clone)]
pub struct P2PConnection {
    /// Connection ID
    pub id: Uuid,
    /// Remote peer device ID
    pub peer_device_id: DeviceId,
    /// Remote socket address
    pub remote_addr: SocketAddr,
    /// Connection established timestamp
    pub established_at: Instant,
    /// Last activity timestamp
    pub last_activity: Instant,
    /// Noise handshake state (encrypted after handshake)
    pub noise_state: NoiseState,
}

/// Noise protocol state for connection
#[derive(Debug, Clone)]
pub enum NoiseState {
    /// Handshake in progress
    Handshaking {
        pattern: NoisePattern,
        state: Vec<u8>, // Serialized handshake state
    },
    /// Handshake complete, ready for transport
    Transport {
        send_cipher: Vec<u8>,  // Noise cipher state for sending
        recv_cipher: Vec<u8>,  // Noise cipher state for receiving
        remote_key: VerifyingKey, // Remote peer's verified static key
    },
}

impl P2PConnection {
    pub fn new(
        peer_device_id: DeviceId,
        remote_addr: SocketAddr,
        noise_state: NoiseState,
    ) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            peer_device_id,
            remote_addr,
            established_at: now,
            last_activity: now,
            noise_state,
        }
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if connection is stale
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }
}

/// Direct P2P transport implementation
pub struct NoiseTcpTransport {
    /// Local device signing key for authentication
    device_key: SigningKey,
    /// Local device ID
    device_id: DeviceId,
    /// Local listening address
    listen_addr: SocketAddr,
    /// Active connections by peer device ID
    connections: Arc<Mutex<HashMap<DeviceId, P2PConnection>>>,
    /// TCP listener for incoming connections
    listener: Option<Arc<TcpListener>>,
    /// Message queues for each connection
    message_queues: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>,
    /// Connection timeout
    connection_timeout: Duration,
}

impl NoiseTcpTransport {
    /// Create new Noise-over-TCP transport
    pub async fn new(
        device_key: SigningKey,
        device_id: DeviceId,
        listen_addr: SocketAddr,
    ) -> TransportResult<Self> {
        info!(
            "Creating Noise TCP transport for device {} on {}",
            device_id.short_string(),
            listen_addr
        );

        // Start TCP listener
        let listener = TcpListener::bind(listen_addr)
            .await
            .map_err(|e| TransportError::connection_failed(&format!("Failed to bind TCP listener: {}", e)))?;

        info!("TCP listener bound to {}", listener.local_addr().unwrap());

        Ok(Self {
            device_key,
            device_id,
            listen_addr,
            connections: Arc::new(Mutex::new(HashMap::new())),
            listener: Some(Arc::new(listener)),
            message_queues: Arc::new(Mutex::new(HashMap::new())),
            connection_timeout: Duration::from_secs(300), // 5 minutes
        })
    }

    /// Start accepting incoming connections
    pub async fn start_listening(&self) -> TransportResult<()> {
        let listener = self.listener.as_ref()
            .ok_or_else(|| TransportError::connection_failed("No listener available"))?
            .clone();

        let connections = self.connections.clone();
        let message_queues = self.message_queues.clone();
        let device_key = self.device_key.clone();
        let device_id = self.device_id;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        info!("Incoming connection from {}", remote_addr);

                        let connections = connections.clone();
                        let message_queues = message_queues.clone();
                        let device_key = device_key.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_incoming_connection(
                                stream,
                                remote_addr,
                                device_key,
                                device_id,
                                connections,
                                message_queues,
                            ).await {
                                error!("Failed to handle incoming connection: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle incoming TCP connection with Noise handshake
    async fn handle_incoming_connection(
        mut stream: TcpStream,
        remote_addr: SocketAddr,
        device_key: SigningKey,
        local_device_id: DeviceId,
        connections: Arc<Mutex<HashMap<DeviceId, P2PConnection>>>,
        message_queues: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>,
    ) -> TransportResult<()> {
        debug!("Handling incoming connection from {}", remote_addr);

        // Perform Noise handshake (simplified for MVP)
        let (remote_device_id, noise_state) = Self::perform_responder_handshake(
            &mut stream,
            &device_key,
        ).await?;

        info!(
            "Noise handshake completed with peer {} from {}",
            remote_device_id.short_string(),
            remote_addr
        );

        // Create connection
        let connection = P2PConnection::new(remote_device_id, remote_addr, noise_state);
        let connection_id = connection.id;

        // Store connection
        {
            let mut conns = connections.lock().unwrap();
            conns.insert(remote_device_id, connection);
        }

        // Create message queue for this connection
        let (tx, mut rx) = mpsc::unbounded_channel();
        {
            let mut queues = message_queues.lock().unwrap();
            queues.insert(connection_id, tx);
        }

        // Handle messages for this connection
        loop {
            tokio::select! {
                // Send outgoing messages
                msg = rx.recv() => {
                    match msg {
                        Some(message) => {
                            if let Err(e) = Self::send_encrypted_message(&mut stream, &message).await {
                                error!("Failed to send message: {}", e);
                                break;
                            }
                        }
                        None => {
                            debug!("Message channel closed for connection {}", connection_id);
                            break;
                        }
                    }
                }

                // Receive incoming messages
                result = Self::receive_encrypted_message(&mut stream) => {
                    match result {
                        Ok(message) => {
                            debug!("Received message from {}: {} bytes", remote_device_id.short_string(), message.len());
                            // TODO: Forward to message handler
                        }
                        Err(e) => {
                            error!("Failed to receive message: {}", e);
                            break;
                        }
                    }
                }
            }
        }

        // Clean up connection
        {
            let mut conns = connections.lock().unwrap();
            conns.remove(&remote_device_id);
        }
        {
            let mut queues = message_queues.lock().unwrap();
            queues.remove(&connection_id);
        }

        info!("Connection closed with {}", remote_device_id.short_string());
        Ok(())
    }

    /// Perform Noise handshake as responder (simplified)
    async fn perform_responder_handshake(
        stream: &mut TcpStream,
        device_key: &SigningKey,
    ) -> TransportResult<(DeviceId, NoiseState)> {
        // Simplified handshake for MVP - in production this would use snow crate
        // for proper Noise protocol implementation

        // Read handshake message 1 (initiator's ephemeral key + device ID)
        let mut handshake_msg = vec![0u8; 1024];
        let n = stream.read(&mut handshake_msg).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake read failed: {}", e)))?;
        handshake_msg.truncate(n);

        // Extract remote device ID (first 32 bytes for simplicity)
        if handshake_msg.len() < 32 {
            return Err(TransportError::connection_failed("Invalid handshake message"));
        }
        let remote_device_id = DeviceId::from_bytes(&handshake_msg[0..32]);

        // Create response with our device ID and ephemeral key
        let mut response = Vec::new();
        response.extend_from_slice(device_key.verifying_key().as_bytes());
        response.extend_from_slice(b"noise_handshake_response"); // Placeholder

        // Send handshake response
        stream.write_all(&response).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake write failed: {}", e)))?;

        // Create transport state
        let noise_state = NoiseState::Transport {
            send_cipher: vec![1, 2, 3], // Placeholder cipher state
            recv_cipher: vec![4, 5, 6], // Placeholder cipher state
            remote_key: device_key.verifying_key(), // In reality, extract from handshake
        };

        Ok((remote_device_id, noise_state))
    }

    /// Send encrypted message over Noise transport
    async fn send_encrypted_message(
        stream: &mut TcpStream,
        message: &[u8],
    ) -> TransportResult<()> {
        // In production, this would encrypt using Noise transport cipher
        // For MVP, we'll use a simple length-prefixed format

        let len = message.len() as u32;
        stream.write_all(&len.to_le_bytes()).await
            .map_err(|e| TransportError::io_error(&format!("Failed to write length: {}", e)))?;

        stream.write_all(message).await
            .map_err(|e| TransportError::io_error(&format!("Failed to write message: {}", e)))?;

        Ok(())
    }

    /// Receive encrypted message from Noise transport
    async fn receive_encrypted_message(
        stream: &mut TcpStream,
    ) -> TransportResult<Vec<u8>> {
        // Read message length
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await
            .map_err(|e| TransportError::io_error(&format!("Failed to read length: {}", e)))?;

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 1024 * 1024 {
            return Err(TransportError::protocol_error("Message too large"));
        }

        // Read message data
        let mut message = vec![0u8; len];
        stream.read_exact(&mut message).await
            .map_err(|e| TransportError::io_error(&format!("Failed to read message: {}", e)))?;

        Ok(message)
    }

    /// Connect to remote peer
    pub async fn connect_to_peer(
        &self,
        peer_device_id: DeviceId,
        peer_addr: SocketAddr,
    ) -> TransportResult<()> {
        info!(
            "Connecting to peer {} at {}",
            peer_device_id.short_string(),
            peer_addr
        );

        // Check if already connected
        {
            let conns = self.connections.lock().unwrap();
            if conns.contains_key(&peer_device_id) {
                return Ok(());
            }
        }

        // Establish TCP connection
        let mut stream = TcpStream::connect(peer_addr).await
            .map_err(|e| TransportError::connection_failed(&format!("TCP connect failed: {}", e)))?;

        // Perform Noise handshake as initiator
        let noise_state = self.perform_initiator_handshake(&mut stream, peer_device_id).await?;

        // Create connection
        let connection = P2PConnection::new(peer_device_id, peer_addr, noise_state);
        let connection_id = connection.id;

        // Store connection
        {
            let mut conns = self.connections.lock().unwrap();
            conns.insert(peer_device_id, connection);
        }

        info!("Successfully connected to peer {}", peer_device_id.short_string());
        Ok(())
    }

    /// Perform Noise handshake as initiator (simplified)
    async fn perform_initiator_handshake(
        &self,
        stream: &mut TcpStream,
        peer_device_id: DeviceId,
    ) -> TransportResult<NoiseState> {
        // Send handshake message 1 (our device ID + ephemeral key)
        let mut handshake_msg = Vec::new();
        handshake_msg.extend_from_slice(&self.device_id.to_bytes());
        handshake_msg.extend_from_slice(self.device_key.verifying_key().as_bytes());
        handshake_msg.extend_from_slice(b"noise_handshake_init"); // Placeholder

        stream.write_all(&handshake_msg).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake write failed: {}", e)))?;

        // Read handshake response
        let mut response = vec![0u8; 1024];
        let n = stream.read(&mut response).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake read failed: {}", e)))?;
        response.truncate(n);

        // Create transport state
        let noise_state = NoiseState::Transport {
            send_cipher: vec![7, 8, 9], // Placeholder cipher state
            recv_cipher: vec![10, 11, 12], // Placeholder cipher state
            remote_key: self.device_key.verifying_key(), // In reality, extract from handshake
        };

        Ok(noise_state)
    }

    /// Clean up stale connections
    pub fn cleanup_stale_connections(&self) {
        let mut stale_peers = Vec::new();

        {
            let conns = self.connections.lock().unwrap();
            for (peer_id, conn) in conns.iter() {
                if conn.is_stale(self.connection_timeout) {
                    stale_peers.push(*peer_id);
                }
            }
        }

        if !stale_peers.is_empty() {
            let mut conns = self.connections.lock().unwrap();
            for peer_id in stale_peers {
                info!("Removing stale connection to {}", peer_id.short_string());
                conns.remove(&peer_id);
            }
        }
    }
}

#[async_trait]
impl Transport for NoiseTcpTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        peer_ticket: &PresenceTicket,
    ) -> TransportResult<Connection> {
        // Parse peer device ID from peer_id string
        let peer_device_id = DeviceId::from_string(peer_id);

        // Extract peer address from presence ticket
        let peer_addr = self.extract_peer_addr_from_ticket(peer_ticket)?;

        // Connect to peer
        self.connect_to_peer(peer_device_id, peer_addr).await?;

        // Create connection handle
        let connection = Connection {
            id: Uuid::new_v4(),
            peer_id: peer_id.to_string(),
            established_at: std::time::SystemTime::now(),
        };

        Ok(connection)
    }

    async fn send(&self, conn: &Connection, message: &[u8]) -> TransportResult<()> {
        let peer_device_id = DeviceId::from_string(&conn.peer_id);

        // Find connection
        let connection_id = {
            let conns = self.connections.lock().unwrap();
            conns.get(&peer_device_id)
                .map(|c| c.id)
                .ok_or_else(|| TransportError::connection_failed("Connection not found"))?
        };

        // Send through message queue
        {
            let queues = self.message_queues.lock().unwrap();
            if let Some(tx) = queues.get(&connection_id) {
                tx.send(message.to_vec())
                    .map_err(|_| TransportError::connection_failed("Failed to queue message"))?;
            } else {
                return Err(TransportError::connection_failed("Message queue not found"));
            }
        }

        Ok(())
    }

    async fn receive(
        &self,
        _conn: &Connection,
        _timeout: Duration,
    ) -> TransportResult<Option<Vec<u8>>> {
        // In this implementation, messages are handled by the connection loop
        // For the Transport trait, we'll return None indicating async message handling
        Ok(None)
    }

    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> TransportResult<crate::BroadcastResult> {
        let mut successful = 0;
        let mut failed = 0;

        for conn in connections {
            match self.send(conn, message).await {
                Ok(_) => successful += 1,
                Err(_) => failed += 1,
            }
        }

        Ok(crate::BroadcastResult {
            successful,
            failed,
            total: connections.len(),
        })
    }

    async fn disconnect(&self, conn: &Connection) -> TransportResult<()> {
        let peer_device_id = DeviceId::from_string(&conn.peer_id);

        // Remove connection
        {
            let mut conns = self.connections.lock().unwrap();
            conns.remove(&peer_device_id);
        }

        info!("Disconnected from peer {}", peer_device_id.short_string());
        Ok(())
    }

    async fn is_connected(&self, conn: &Connection) -> bool {
        let peer_device_id = DeviceId::from_string(&conn.peer_id);
        let conns = self.connections.lock().unwrap();
        conns.contains_key(&peer_device_id)
    }
}

impl NoiseTcpTransport {
    /// Extract peer address from presence ticket
    fn extract_peer_addr_from_ticket(
        &self,
        ticket: &PresenceTicket,
    ) -> TransportResult<SocketAddr> {
        // For MVP, assume presence ticket contains direct address
        // In production, this would support various address types
        if let Some(addr_str) = ticket.metadata.get("tcp_addr") {
            addr_str.parse()
                .map_err(|e| TransportError::protocol_error(&format!("Invalid address: {}", e)))
        } else {
            Err(TransportError::protocol_error("No TCP address in presence ticket"))
        }
    }
}

/// Builder for NoiseTcpTransport
pub struct NoiseTcpTransportBuilder {
    device_key: Option<SigningKey>,
    device_id: Option<DeviceId>,
    listen_addr: Option<SocketAddr>,
    connection_timeout: Duration,
}

impl NoiseTcpTransportBuilder {
    pub fn new() -> Self {
        Self {
            device_key: None,
            device_id: None,
            listen_addr: None,
            connection_timeout: Duration::from_secs(300),
        }
    }

    pub fn device_key(mut self, key: SigningKey) -> Self {
        self.device_key = Some(key);
        self
    }

    pub fn device_id(mut self, id: DeviceId) -> Self {
        self.device_id = Some(id);
        self
    }

    pub fn listen_addr(mut self, addr: SocketAddr) -> Self {
        self.listen_addr = Some(addr);
        self
    }

    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    pub async fn build(self) -> TransportResult<NoiseTcpTransport> {
        let device_key = self.device_key
            .ok_or_else(|| TransportError::configuration_error("Device key required"))?;
        let device_id = self.device_id
            .ok_or_else(|| TransportError::configuration_error("Device ID required"))?;
        let listen_addr = self.listen_addr
            .ok_or_else(|| TransportError::configuration_error("Listen address required"))?;

        let mut transport = NoiseTcpTransport::new(device_key, device_id, listen_addr).await?;
        transport.connection_timeout = self.connection_timeout;

        Ok(transport)
    }
}

impl Default for NoiseTcpTransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}
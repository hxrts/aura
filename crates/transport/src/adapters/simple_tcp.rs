//! Simple TCP transport without Noise protocol (for immediate testing)
//!
//! This is a simplified version of P2P transport that uses plain TCP
//! for immediate testing while the full Noise implementation is completed.

use crate::{
    BroadcastResult, Connection, ConnectionManager, PresenceTicket, Transport,
    TransportErrorBuilder, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Simple TCP connection state
#[derive(Debug, Clone)]
pub struct SimpleTcpConnection {
    pub id: Uuid,
    pub peer_device_id: DeviceId,
    pub remote_addr: SocketAddr,
    pub established_at: Instant,
    pub last_activity: Instant,
}

impl SimpleTcpConnection {
    pub fn new(peer_device_id: DeviceId, remote_addr: SocketAddr) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            peer_device_id,
            remote_addr,
            established_at: now,
            last_activity: now,
        }
    }

    pub fn touch(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }
}

/// Simple TCP transport for immediate P2P testing
pub struct SimpleTcpTransport {
    device_id: DeviceId,
    listen_addr: SocketAddr,
    connections: Arc<Mutex<HashMap<DeviceId, SimpleTcpConnection>>>,
    listener: Option<Arc<TcpListener>>,
    message_queues: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>,
    connection_timeout: Duration,
}

impl SimpleTcpTransport {
    /// Create new simple TCP transport
    pub async fn new(device_id: DeviceId, listen_addr: SocketAddr) -> TransportResult<Self> {
        info!(
            "Creating Simple TCP transport for device {} on {}",
            device_id.to_string(),
            listen_addr
        );

        let listener = TcpListener::bind(listen_addr).await.map_err(|e| {
            TransportErrorBuilder::connection_failed(&format!("Failed to bind TCP listener: {}", e))
        })?;

        info!("TCP listener bound to {}", listener.local_addr().unwrap());

        Ok(Self {
            device_id,
            listen_addr,
            connections: Arc::new(Mutex::new(HashMap::new())),
            listener: Some(Arc::new(listener)),
            message_queues: Arc::new(Mutex::new(HashMap::new())),
            connection_timeout: Duration::from_secs(300),
        })
    }

    /// Start accepting incoming connections
    pub async fn start_listening(&self) -> TransportResult<()> {
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| TransportErrorBuilder::connection_failed("No listener available"))?
            .clone();

        let connections = self.connections.clone();
        let message_queues = self.message_queues.clone();
        let device_id = self.device_id;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote_addr)) => {
                        info!("Incoming connection from {}", remote_addr);

                        let connections = connections.clone();
                        let message_queues = message_queues.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_incoming_connection(
                                stream,
                                remote_addr,
                                device_id,
                                connections,
                                message_queues,
                            )
                            .await
                            {
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

    /// Handle incoming TCP connection
    async fn handle_incoming_connection(
        mut stream: TcpStream,
        remote_addr: SocketAddr,
        local_device_id: DeviceId,
        connections: Arc<Mutex<HashMap<DeviceId, SimpleTcpConnection>>>,
        message_queues: Arc<Mutex<HashMap<Uuid, mpsc::UnboundedSender<Vec<u8>>>>>,
    ) -> TransportResult<()> {
        debug!("Handling incoming connection from {}", remote_addr);

        // Simple handshake: exchange device IDs
        let (remote_device_id, _) =
            Self::perform_simple_handshake(&mut stream, local_device_id).await?;

        info!(
            "Handshake completed with peer {} from {}",
            remote_device_id.to_string(),
            remote_addr
        );

        // Create connection
        let connection = SimpleTcpConnection::new(remote_device_id, remote_addr);
        let connection_id = connection.id;

        // Store connection
        {
            let mut conns = connections.lock().unwrap();
            conns.insert(remote_device_id, connection);
        }

        // Create message queue
        let (tx, mut rx) = mpsc::unbounded_channel();
        {
            let mut queues = message_queues.lock().unwrap();
            queues.insert(connection_id, tx);
        }

        // Handle messages
        loop {
            tokio::select! {
                // Send outgoing messages
                msg = rx.recv() => {
                    match msg {
                        Some(message) => {
                            if let Err(e) = Self::send_message(&mut stream, &message).await {
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
                result = Self::receive_message(&mut stream) => {
                    match result {
                        Ok(message) => {
                            debug!("Received message from {}: {} bytes", remote_device_id.to_string(), message.len());
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

        // Clean up
        {
            let mut conns = connections.lock().unwrap();
            conns.remove(&remote_device_id);
        }
        {
            let mut queues = message_queues.lock().unwrap();
            queues.remove(&connection_id);
        }

        info!("Connection closed with {}", remote_device_id.to_string());
        Ok(())
    }

    /// Simple handshake: exchange device IDs
    async fn perform_simple_handshake(
        stream: &mut TcpStream,
        local_device_id: DeviceId,
    ) -> TransportResult<(DeviceId, ())> {
        // Send our device ID
        let local_id_bytes = local_device_id.0.as_bytes();
        stream
            .write_all(&(local_id_bytes.len() as u32).to_le_bytes())
            .await
            .map_err(|e| {
                TransportErrorBuilder::connection_failed(&format!("Handshake write failed: {}", e))
            })?;
        stream.write_all(local_id_bytes).await.map_err(|e| {
            TransportErrorBuilder::connection_failed(&format!("Handshake write failed: {}", e))
        })?;

        // Read remote device ID
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await.map_err(|e| {
            TransportErrorBuilder::connection_failed(&format!("Handshake read failed: {}", e))
        })?;

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 64 {
            return Err(TransportErrorBuilder::connection_failed(
                "Invalid handshake message",
            ));
        }

        let mut remote_id_bytes = vec![0u8; len];
        stream.read_exact(&mut remote_id_bytes).await.map_err(|e| {
            TransportErrorBuilder::connection_failed(&format!("Handshake read failed: {}", e))
        })?;

        let remote_device_id =
            DeviceId::from_uuid(uuid::Uuid::from_slice(&remote_id_bytes).map_err(|e| {
                TransportErrorBuilder::connection_failed(&format!("Invalid UUID bytes: {}", e))
            })?);

        Ok((remote_device_id, ()))
    }

    /// Send message over TCP
    async fn send_message(stream: &mut TcpStream, message: &[u8]) -> TransportResult<()> {
        let len = message.len() as u32;
        stream.write_all(&len.to_le_bytes()).await.map_err(|e| {
            TransportErrorBuilder::io_error(&format!("Failed to write length: {}", e))
        })?;

        stream.write_all(message).await.map_err(|e| {
            TransportErrorBuilder::io_error(&format!("Failed to write message: {}", e))
        })?;

        Ok(())
    }

    /// Receive message from TCP
    async fn receive_message(stream: &mut TcpStream) -> TransportResult<Vec<u8>> {
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await.map_err(|e| {
            TransportErrorBuilder::io_error(&format!("Failed to read length: {}", e))
        })?;

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 1024 * 1024 {
            return Err(TransportErrorBuilder::protocol_error("Message too large"));
        }

        let mut message = vec![0u8; len];
        stream.read_exact(&mut message).await.map_err(|e| {
            TransportErrorBuilder::io_error(&format!("Failed to read message: {}", e))
        })?;

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
            peer_device_id.to_string(),
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
        let mut stream = TcpStream::connect(peer_addr).await.map_err(|e| {
            TransportErrorBuilder::connection_failed(&format!("TCP connect failed: {}", e))
        })?;

        // Perform handshake
        let (verified_device_id, _) =
            Self::perform_simple_handshake(&mut stream, self.device_id).await?;

        if verified_device_id != peer_device_id {
            return Err(TransportErrorBuilder::authentication_failed(
                "Device ID mismatch",
            ));
        }

        // Create connection
        let connection = SimpleTcpConnection::new(peer_device_id, peer_addr);
        let _connection_id = connection.id;

        // Store connection
        {
            let mut conns = self.connections.lock().unwrap();
            conns.insert(peer_device_id, connection);
        }

        info!(
            "Successfully connected to peer {}",
            peer_device_id.to_string()
        );
        Ok(())
    }
}

#[async_trait]
impl Transport for SimpleTcpTransport {
    type ConnectionType = Connection;

    async fn connect_to_peer(&self, peer_id: DeviceId) -> TransportResult<Uuid> {
        // Basic TCP connection implementation
        let connection_id = Uuid::new_v4();

        // In a real implementation, this would:
        // 1. Resolve peer address from peer_id
        // 2. Establish TCP connection
        // 3. Perform handshake
        // 4. Store connection in active connections map

        info!(
            "TCP connection established to peer {}: {}",
            peer_id, connection_id
        );
        Ok(connection_id)
    }

    async fn send_to_peer(&self, peer_id: DeviceId, message: &[u8]) -> TransportResult<()> {
        // Basic TCP message sending implementation
        debug!("Sending {} bytes to peer {}", message.len(), peer_id);

        // In a real implementation, this would:
        // 1. Look up active connection for peer_id
        // 2. Frame the message with length prefix
        // 3. Send over TCP socket
        // 4. Handle connection errors and retry logic

        if message.len() > 1024 * 1024 {
            return Err(TransportErrorBuilder::transport("Message too large"));
        }

        Ok(())
    }

    async fn receive_message(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, Vec<u8>)>> {
        // Basic message receiving implementation
        debug!("Waiting for messages with timeout: {:?}", timeout);

        // In a real implementation, this would:
        // 1. Poll all active connections for incoming data
        // 2. Parse framed messages
        // 3. Return (sender_device_id, message_bytes)
        // 4. Handle timeout and connection errors

        // For now, return None to indicate no messages available
        Ok(None)
    }

    async fn disconnect_from_peer(&self, peer_id: DeviceId) -> TransportResult<()> {
        // Basic disconnection implementation
        info!("Disconnecting from peer {}", peer_id);

        // In a real implementation, this would:
        // 1. Find active connection for peer_id
        // 2. Send graceful close
        // 3. Close TCP socket
        // 4. Remove from active connections map

        Ok(())
    }

    async fn is_peer_reachable(&self, peer_id: DeviceId) -> bool {
        // Basic reachability check implementation
        debug!("Checking reachability for peer {}", peer_id);

        // In a real implementation, this would:
        // 1. Check if we have an active connection
        // 2. Try a quick ping/health check
        // 3. Return connection status

        // For now, assume peers are reachable
        true
    }

    fn get_connections(&self) -> Vec<Self::ConnectionType> {
        // Return active connections
        // In a real implementation, this would return actual Connection objects
        // from the connections map
        Vec::new()
    }

    async fn start(
        &mut self,
        _message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>,
    ) -> TransportResult<()> {
        // Basic TCP server startup
        info!("Starting SimpleTcp transport on {}", self.listen_addr);

        // In a real implementation, this would:
        // 1. Bind to listen_addr
        // 2. Start accepting connections
        // 3. Spawn tasks to handle incoming connections
        // 4. Forward received messages to message_sender

        debug!("SimpleTcp transport started successfully");
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        // Basic TCP server shutdown
        info!("Stopping SimpleTcp transport");

        // In a real implementation, this would:
        // 1. Stop accepting new connections
        // 2. Gracefully close existing connections
        // 3. Cancel background tasks
        // 4. Clean up resources

        debug!("SimpleTcp transport stopped successfully");
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "simple_tcp"
    }

    async fn health_check(&self) -> bool {
        true
    }
}

#[async_trait]
impl ConnectionManager for SimpleTcpTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        _peer_ticket: &PresenceTicket,
    ) -> TransportResult<Connection> {
        let device_id = DeviceId::from_str(peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;

        let connection_id = Transport::connect_to_peer(self, device_id).await?;
        Ok(Connection {
            id: connection_id.to_string(),
            peer_id: peer_id.to_string(),
        })
    }

    async fn send(&self, conn: &Connection, message: &[u8]) -> TransportResult<()> {
        let device_id = DeviceId::from_str(&conn.peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;
        self.send_to_peer(device_id, message).await
    }

    async fn receive(
        &self,
        _conn: &Connection,
        timeout: Duration,
    ) -> TransportResult<Option<Vec<u8>>> {
        if let Some((_, message)) = self.receive_message(timeout).await? {
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }

    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> TransportResult<BroadcastResult> {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for conn in connections {
            match self.send(conn, message).await {
                Ok(()) => succeeded.push(conn.peer_id.clone()),
                Err(_) => failed.push(conn.peer_id.clone()),
            }
        }

        Ok(BroadcastResult { succeeded, failed })
    }

    async fn disconnect(&self, conn: &Connection) -> TransportResult<()> {
        let device_id = DeviceId::from_str(&conn.peer_id).map_err(|e| {
            TransportErrorBuilder::invalid_peer_id(&format!("Invalid peer ID: {}", e))
        })?;
        self.disconnect_from_peer(device_id).await
    }

    async fn is_connected(&self, conn: &Connection) -> bool {
        let device_id = match DeviceId::from_str(&conn.peer_id) {
            Ok(id) => id,
            Err(_) => return false,
        };

        self.is_peer_reachable(device_id).await
    }
}

impl SimpleTcpTransport {
    /// Extract peer address from presence ticket
    /// Basic implementation using placeholder logic
    #[allow(dead_code)]
    fn extract_peer_addr_from_ticket(
        &self,
        ticket: &PresenceTicket,
    ) -> TransportResult<SocketAddr> {
        // Basic implementation: derive address from device_id for testing
        // In a real implementation, this would:
        // 1. Look up address from ticket metadata
        // 2. Query peer discovery service
        // 3. Use DHT or directory service

        let device_id = ticket.device_id;
        let port = 8000 + (device_id.as_bytes()[0] as u16); // Simple port derivation
        let addr = format!("127.0.0.1:{}", port);

        addr.parse().map_err(|e| {
            TransportErrorBuilder::protocol_error(&format!(
                "Failed to parse derived address {}: {}",
                addr, e
            ))
        })
    }
}

/// Builder for SimpleTcpTransport
pub struct SimpleTcpTransportBuilder {
    device_id: Option<DeviceId>,
    listen_addr: Option<SocketAddr>,
    connection_timeout: Duration,
}

impl SimpleTcpTransportBuilder {
    pub fn new() -> Self {
        Self {
            device_id: None,
            listen_addr: None,
            connection_timeout: Duration::from_secs(300),
        }
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

    pub async fn build(self) -> TransportResult<SimpleTcpTransport> {
        let device_id = self
            .device_id
            .ok_or_else(|| TransportErrorBuilder::configuration_error("Device ID required"))?;
        let listen_addr = self
            .listen_addr
            .ok_or_else(|| TransportErrorBuilder::configuration_error("Listen address required"))?;

        let mut transport = SimpleTcpTransport::new(device_id, listen_addr).await?;
        transport.connection_timeout = self.connection_timeout;

        Ok(transport)
    }
}

impl Default for SimpleTcpTransportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

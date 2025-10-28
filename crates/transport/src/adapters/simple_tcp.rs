//! Simple TCP transport without Noise protocol (for immediate testing)
//!
//! This is a simplified version of P2P transport that uses plain TCP
//! for immediate testing while the full Noise implementation is completed.

use crate::{
    Connection, PresenceTicket, Transport, TransportError, TransportResult,
};
use async_trait::async_trait;
use aura_types::{DeviceId, DeviceIdExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
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
            device_id.short_string(),
            listen_addr
        );

        let listener = TcpListener::bind(listen_addr)
            .await
            .map_err(|e| TransportError::connection_failed(&format!("Failed to bind TCP listener: {}", e)))?;

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
        let listener = self.listener.as_ref()
            .ok_or_else(|| TransportError::connection_failed("No listener available"))?
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
        let (remote_device_id, _) = Self::perform_simple_handshake(&mut stream, local_device_id).await?;

        info!(
            "Handshake completed with peer {} from {}",
            remote_device_id.short_string(),
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

        // Clean up
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

    /// Simple handshake: exchange device IDs
    async fn perform_simple_handshake(
        stream: &mut TcpStream,
        local_device_id: DeviceId,
    ) -> TransportResult<(DeviceId, ())> {
        // Send our device ID
        let local_id_bytes = local_device_id.to_bytes();
        stream.write_all(&(local_id_bytes.len() as u32).to_le_bytes()).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake write failed: {}", e)))?;
        stream.write_all(&local_id_bytes).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake write failed: {}", e)))?;

        // Read remote device ID
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake read failed: {}", e)))?;
        
        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 64 {
            return Err(TransportError::connection_failed("Invalid handshake message"));
        }

        let mut remote_id_bytes = vec![0u8; len];
        stream.read_exact(&mut remote_id_bytes).await
            .map_err(|e| TransportError::connection_failed(&format!("Handshake read failed: {}", e)))?;

        let remote_device_id = DeviceId::from_bytes(&remote_id_bytes);

        Ok((remote_device_id, ()))
    }

    /// Send message over TCP
    async fn send_message(stream: &mut TcpStream, message: &[u8]) -> TransportResult<()> {
        let len = message.len() as u32;
        stream.write_all(&len.to_le_bytes()).await
            .map_err(|e| TransportError::io_error(&format!("Failed to write length: {}", e)))?;

        stream.write_all(message).await
            .map_err(|e| TransportError::io_error(&format!("Failed to write message: {}", e)))?;

        Ok(())
    }

    /// Receive message from TCP
    async fn receive_message(stream: &mut TcpStream) -> TransportResult<Vec<u8>> {
        let mut len_bytes = [0u8; 4];
        stream.read_exact(&mut len_bytes).await
            .map_err(|e| TransportError::io_error(&format!("Failed to read length: {}", e)))?;

        let len = u32::from_le_bytes(len_bytes) as usize;
        if len > 1024 * 1024 {
            return Err(TransportError::protocol_error("Message too large"));
        }

        let mut message = vec![0u8; len];
        stream.read_exact(&mut message).await
            .map_err(|e| TransportError::io_error(&format!("Failed to read message: {}", e)))?;

        Ok(message)
    }

    /// Connect to remote peer
    pub async fn connect_to_peer(&self, peer_device_id: DeviceId, peer_addr: SocketAddr) -> TransportResult<()> {
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

        // Perform handshake
        let (verified_device_id, _) = Self::perform_simple_handshake(&mut stream, self.device_id).await?;

        if verified_device_id != peer_device_id {
            return Err(TransportError::authentication_failed("Device ID mismatch"));
        }

        // Create connection
        let connection = SimpleTcpConnection::new(peer_device_id, peer_addr);
        let connection_id = connection.id;

        // Store connection
        {
            let mut conns = self.connections.lock().unwrap();
            conns.insert(peer_device_id, connection);
        }

        info!("Successfully connected to peer {}", peer_device_id.short_string());
        Ok(())
    }
}

#[async_trait]
impl Transport for SimpleTcpTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        peer_ticket: &PresenceTicket,
    ) -> TransportResult<Connection> {
        let peer_device_id = DeviceId::from_string(peer_id);
        let peer_addr = self.extract_peer_addr_from_ticket(peer_ticket)?;

        self.connect_to_peer(peer_device_id, peer_addr).await?;

        let connection = Connection {
            id: Uuid::new_v4(),
            peer_id: peer_id.to_string(),
            established_at: std::time::SystemTime::now(),
        };

        Ok(connection)
    }

    async fn send(&self, conn: &Connection, message: &[u8]) -> TransportResult<()> {
        let peer_device_id = DeviceId::from_string(&conn.peer_id);

        let connection_id = {
            let conns = self.connections.lock().unwrap();
            conns.get(&peer_device_id)
                .map(|c| c.id)
                .ok_or_else(|| TransportError::connection_failed("Connection not found"))?
        };

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

    async fn receive(&self, _conn: &Connection, _timeout: Duration) -> TransportResult<Option<Vec<u8>>> {
        // Messages are handled by the connection loop
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

impl SimpleTcpTransport {
    /// Extract peer address from presence ticket
    fn extract_peer_addr_from_ticket(&self, ticket: &PresenceTicket) -> TransportResult<SocketAddr> {
        if let Some(addr_str) = ticket.metadata.get("tcp_addr") {
            addr_str.parse()
                .map_err(|e| TransportError::protocol_error(&format!("Invalid address: {}", e)))
        } else {
            Err(TransportError::protocol_error("No TCP address in presence ticket"))
        }
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
        let device_id = self.device_id
            .ok_or_else(|| TransportError::configuration_error("Device ID required"))?;
        let listen_addr = self.listen_addr
            .ok_or_else(|| TransportError::configuration_error("Listen address required"))?;

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
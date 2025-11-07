//! Transport Handler Trait
//!
//! Defines the core transport operations that can be wrapped with middleware.

use aura_protocol::effects::{AuraEffects, TimeEffects};
use aura_protocol::middleware::MiddlewareResult;
use std::collections::HashMap;
use std::net::SocketAddr;

/// Network address abstraction
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NetworkAddress {
    /// TCP socket address
    Tcp(SocketAddr),
    /// UDP socket address  
    Udp(SocketAddr),
    /// HTTP/HTTPS URL
    Http(String),
    /// In-memory address for testing
    Memory(String),
    /// Peer ID for P2P networks
    Peer(String),
}

impl NetworkAddress {
    pub fn as_string(&self) -> String {
        match self {
            NetworkAddress::Tcp(addr) => format!("tcp://{}", addr),
            NetworkAddress::Udp(addr) => format!("udp://{}", addr),
            NetworkAddress::Http(url) => url.clone(),
            NetworkAddress::Memory(id) => format!("memory://{}", id),
            NetworkAddress::Peer(id) => format!("peer://{}", id),
        }
    }
}

/// Core transport operations
#[derive(Debug, Clone)]
pub enum TransportOperation {
    /// Send data to a destination
    Send {
        destination: NetworkAddress,
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    },
    /// Receive data from a source
    Receive {
        source: Option<NetworkAddress>,
        timeout_ms: Option<u64>,
    },
    /// Connect to a remote peer
    Connect {
        address: NetworkAddress,
        options: ConnectionOptions,
    },
    /// Disconnect from a peer
    Disconnect { address: NetworkAddress },
    /// Listen for incoming connections
    Listen {
        address: NetworkAddress,
        options: ListenOptions,
    },
    /// Discover available peers
    Discover { criteria: DiscoveryCriteria },
    /// Get connection status
    Status { address: Option<NetworkAddress> },
}

/// Connection options
#[derive(Debug, Clone, Default)]
pub struct ConnectionOptions {
    pub timeout_ms: Option<u64>,
    pub keep_alive: bool,
    pub retry_attempts: u32,
    pub metadata: HashMap<String, String>,
}

/// Listen options
#[derive(Debug, Clone, Default)]
pub struct ListenOptions {
    pub max_connections: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub metadata: HashMap<String, String>,
}

/// Discovery criteria
#[derive(Debug, Clone, Default)]
pub struct DiscoveryCriteria {
    pub protocol: Option<String>,
    pub capabilities: Vec<String>,
    pub max_results: Option<u32>,
    pub timeout_ms: Option<u64>,
}

/// Transport operation results
#[derive(Debug, Clone)]
pub enum TransportResult {
    /// Send operation completed
    Sent {
        destination: NetworkAddress,
        bytes_sent: usize,
    },
    /// Receive operation completed
    Received {
        source: NetworkAddress,
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    },
    /// Connection established
    Connected {
        address: NetworkAddress,
        connection_id: String,
    },
    /// Disconnection completed
    Disconnected { address: NetworkAddress },
    /// Listening started
    Listening {
        address: NetworkAddress,
        listener_id: String,
    },
    /// Peers discovered
    Discovered { peers: Vec<PeerInfo> },
    /// Status retrieved
    Status { connections: Vec<ConnectionInfo> },
}

/// Information about a discovered peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub address: NetworkAddress,
    pub capabilities: Vec<String>,
    pub metadata: HashMap<String, String>,
    pub last_seen: u64,
}

/// Information about an active connection
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub address: NetworkAddress,
    pub connection_id: String,
    pub state: ConnectionState,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub created_at: u64,
    pub last_activity: u64,
}

/// Connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Disconnecting,
    Disconnected,
    Error(String),
}

/// Transport error types
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Connection failed: {address}")]
    ConnectionFailed { address: String },
    #[error("Connection timeout: {address}")]
    ConnectionTimeout { address: String },
    #[error("Send failed: {reason}")]
    SendFailed { reason: String },
    #[error("Receive failed: {reason}")]
    ReceiveFailed { reason: String },
    #[error("Network unreachable: {address}")]
    NetworkUnreachable { address: String },
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    #[error("Circuit breaker open")]
    CircuitBreakerOpen,
    #[error("Discovery failed: {reason}")]
    DiscoveryFailed { reason: String },
    #[error("Protocol error: {message}")]
    ProtocolError { message: String },
    #[error("Authentication failed")]
    AuthenticationFailed,
    #[error("Encryption error: {message}")]
    EncryptionError { message: String },
}

/// Core transport handler trait
pub trait TransportHandler: Send + Sync {
    /// Execute a transport operation
    fn execute(
        &mut self,
        operation: TransportOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<TransportResult>;

    /// Get handler metadata for observability
    fn handler_info(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

/// Base transport handler implementation
pub struct BaseTransportHandler {
    local_address: NetworkAddress,
    connections: HashMap<String, ConnectionInfo>,
    next_connection_id: u64,
}

impl BaseTransportHandler {
    pub fn new(local_address: NetworkAddress) -> Self {
        Self {
            local_address,
            connections: HashMap::new(),
            next_connection_id: 1,
        }
    }

    fn generate_connection_id(&mut self) -> String {
        let id = format!("conn_{}", self.next_connection_id);
        self.next_connection_id += 1;
        id
    }
}

impl TransportHandler for BaseTransportHandler {
    fn execute(
        &mut self,
        operation: TransportOperation,
        effects: &dyn AuraEffects,
    ) -> MiddlewareResult<TransportResult> {
        match operation {
            TransportOperation::Send {
                destination,
                data,
                metadata: _,
            } => {
                // Simulate sending data
                effects.log_info(
                    &format!(
                        "Sending {} bytes to {}",
                        data.len(),
                        destination.as_string()
                    ),
                    &[],
                );

                Ok(TransportResult::Sent {
                    destination,
                    bytes_sent: data.len(),
                })
            }

            TransportOperation::Receive {
                source: _,
                timeout_ms: _,
            } => {
                // Placeholder - return empty data
                Ok(TransportResult::Received {
                    source: self.local_address.clone(),
                    data: Vec::new(),
                    metadata: HashMap::new(),
                })
            }

            TransportOperation::Connect {
                address,
                options: _,
            } => {
                let connection_id = self.generate_connection_id();
                let connection_info = ConnectionInfo {
                    address: address.clone(),
                    connection_id: connection_id.clone(),
                    state: ConnectionState::Connected,
                    bytes_sent: 0,
                    bytes_received: 0,
                    created_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    last_activity: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };

                self.connections
                    .insert(connection_id.clone(), connection_info);

                Ok(TransportResult::Connected {
                    address,
                    connection_id,
                })
            }

            TransportOperation::Disconnect { address } => {
                // Find and remove connection
                self.connections.retain(|_, conn| conn.address != address);

                Ok(TransportResult::Disconnected { address })
            }

            TransportOperation::Listen {
                address,
                options: _,
            } => {
                let listener_id = format!("listener_{}", self.next_connection_id);
                self.next_connection_id += 1;

                Ok(TransportResult::Listening {
                    address,
                    listener_id,
                })
            }

            TransportOperation::Discover { criteria: _ } => {
                // Placeholder - return empty peer list
                Ok(TransportResult::Discovered { peers: Vec::new() })
            }

            TransportOperation::Status { address: _ } => {
                let connections: Vec<ConnectionInfo> = self.connections.values().cloned().collect();

                Ok(TransportResult::Status { connections })
            }
        }
    }

    fn handler_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert(
            "handler_type".to_string(),
            "BaseTransportHandler".to_string(),
        );
        info.insert("local_address".to_string(), self.local_address.as_string());
        info.insert(
            "active_connections".to_string(),
            self.connections.len().to_string(),
        );
        info
    }
}

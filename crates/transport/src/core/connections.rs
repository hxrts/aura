//! Connection management and presence ticket verification
//!
//! This module handles the core connection lifecycle including:
//! - Connection establishment and handshake
//! - Presence ticket verification
//! - Message broadcasting
//! - Connection state management

use crate::{PresenceTicket, TransportError, TransportErrorBuilder, TransportResult};
use async_trait::async_trait;
use std::time::Duration;
use uuid::Uuid;

/// Opaque connection handle
///
/// This type is opaque to protocols - they cannot inspect or modify it.
/// Only the transport implementation knows what this represents.
#[derive(Debug, Clone)]
pub struct Connection {
    /// Opaque connection identifier
    pub(crate) id: String,
    /// Peer device ID
    pub(crate) peer_id: String,
}

impl Connection {
    /// Get the peer device ID for this connection
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Get the connection ID
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Result of a broadcast operation
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Peers that successfully received the message
    pub succeeded: Vec<String>,
    /// Peers that failed to receive the message
    pub failed: Vec<String>,
}

/// Connection builder - helps construct connections with validation
pub struct ConnectionBuilder {
    peer_id: String,
}

impl ConnectionBuilder {
    pub fn new(peer_id: impl Into<String>) -> Self {
        ConnectionBuilder {
            peer_id: peer_id.into(),
        }
    }

    pub fn build(self) -> Connection {
        // Use deterministic ID based on peer_id for testing consistency
        let id = format!("conn_{}", self.peer_id);
        Connection {
            id,
            peer_id: self.peer_id,
        }
    }
}

/// Core transport interface for connection management
///
/// This trait defines the fundamental connection operations that all
/// transport implementations must support.
///
/// Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design
#[async_trait]
pub trait ConnectionManager: Send + Sync {
    /// Connect to a peer using their presence ticket
    ///
    /// The transport performs:
    /// 1. Handshake with peer
    /// 2. Exchange presence tickets
    /// 3. Verify tickets (threshold signature, epoch, expiry, revocation)
    /// 4. Establish encrypted channel
    ///
    /// Reference: 080 spec Part 5: Transport Handshake Specification
    async fn connect(
        &self,
        peer_id: &str,
        my_ticket: &PresenceTicket,
        peer_ticket: &PresenceTicket,
    ) -> TransportResult<Connection>;

    /// Send a message to a peer
    ///
    /// The message is sent over the encrypted channel established during connect().
    async fn send(&self, conn: &Connection, message: &[u8]) -> TransportResult<()>;

    /// Receive a message from a peer with timeout
    ///
    /// Returns None if timeout is reached without receiving a message.
    async fn receive(
        &self,
        conn: &Connection,
        timeout: Duration,
    ) -> TransportResult<Option<Vec<u8>>>;

    /// Broadcast a message to multiple peers
    ///
    /// Returns which peers successfully received the message and which failed.
    /// This is a convenience method - implementations may optimize it or simply
    /// call send() in a loop.
    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> TransportResult<BroadcastResult>;

    /// Disconnect from a peer
    ///
    /// Closes the connection and releases resources.
    async fn disconnect(&self, conn: &Connection) -> TransportResult<()>;

    /// Check if a connection is still active
    ///
    /// Returns false if the connection has been closed or is no longer valid.
    async fn is_connected(&self, conn: &Connection) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_builder() {
        let conn = ConnectionBuilder::new("device123").build();

        assert_eq!(conn.peer_id(), "device123");
        assert!(!conn.id().is_empty());
    }

    #[test]
    fn test_broadcast_result() {
        let result = BroadcastResult {
            succeeded: vec!["dev1".to_string(), "dev2".to_string()],
            failed: vec!["dev3".to_string()],
        };

        assert_eq!(result.succeeded.len(), 2);
        assert_eq!(result.failed.len(), 1);
    }
}
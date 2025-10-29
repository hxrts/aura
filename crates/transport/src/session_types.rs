//! Session Types for Transport Layer
//!
//! This module defines session types for the Transport layer, providing compile-time safety
//! for connection management, message exchange, and presence-based authentication.

use aura_types::session_core::{ChoreographicProtocol, RuntimeWitness, SessionState};
use aura_types::DeviceId;
use std::collections::BTreeMap;
use std::fmt;
use uuid::Uuid;

// Import types from main transport module
use crate::{Connection, PresenceTicket, TransportError};

// Import the trait from protocol-types to implement it
use aura_protocol_types::TransportSession;

// ========== Transport Protocol Core ==========

/// Core transport protocol data without session state
///
/// Contains the essential state for managing transport connections, presence authentication,
/// and message delivery across peers.
#[derive(Clone)]
pub struct TransportProtocolCore {
    /// Identifier for this device
    pub device_id: DeviceId,
    /// Active connections to peer devices
    pub active_connections: BTreeMap<String, Connection>,
    /// Presence tickets for authenticated sessions
    pub presence_tickets: BTreeMap<String, PresenceTicket>,
    /// Pending broadcast operations
    pub pending_broadcasts: Vec<BroadcastContext>,
    /// Queue of messages awaiting delivery
    pub message_queue: Vec<MessageContext>,
}

impl TransportProtocolCore {
    /// Create a new transport protocol core for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            active_connections: BTreeMap::new(),
            presence_tickets: BTreeMap::new(),
            pending_broadcasts: Vec::new(),
            message_queue: Vec::new(),
        }
    }
}

impl fmt::Debug for TransportProtocolCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransportProtocolCore")
            .field("device_id", &self.device_id)
            .field("active_connections_count", &self.active_connections.len())
            .field("presence_tickets_count", &self.presence_tickets.len())
            .field("pending_broadcasts_count", &self.pending_broadcasts.len())
            .field("message_queue_count", &self.message_queue.len())
            .finish()
    }
}

/// Broadcast operation context
///
/// Tracks a broadcast message being sent to multiple peers,
/// including delivery status to each recipient.
#[derive(Debug, Clone)]
pub struct BroadcastContext {
    /// Unique identifier for this broadcast operation
    pub broadcast_id: Uuid,
    /// Peer identifiers that should receive this broadcast
    pub target_peers: Vec<String>,
    /// Serialized message content to broadcast
    pub message: Vec<u8>,
    /// Delivery confirmation status per peer (peer_id -> delivered)
    pub delivery_confirmations: BTreeMap<String, bool>,
}

/// Message context information
///
/// Contains metadata about a message queued for delivery to a peer.
#[derive(Debug, Clone)]
pub struct MessageContext {
    /// Unique identifier for this message
    pub message_id: Uuid,
    /// Target peer identifier
    pub peer_id: String,
    /// Serialized message content
    pub content: Vec<u8>,
    /// Timestamp when message was created
    pub timestamp: u64,
}

// ========== Transport Session States ==========

/// Transport disconnected state
///
/// Initial state indicating no active transport connection.
#[derive(Debug, Clone)]
pub struct TransportDisconnected;

impl SessionState for TransportDisconnected {
    const NAME: &'static str = "TransportDisconnected";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Connection handshaking state
///
/// Transient state during connection establishment with a peer.
#[derive(Debug, Clone)]
pub struct ConnectionHandshaking;

impl SessionState for ConnectionHandshaking {
    const NAME: &'static str = "ConnectionHandshaking";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Transport connected state
///
/// Active state with an established connection to a peer.
#[derive(Debug, Clone)]
pub struct TransportConnected;

impl SessionState for TransportConnected {
    const NAME: &'static str = "TransportConnected";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Connection failed state
///
/// Terminal state indicating connection establishment or operation failed.
#[derive(Debug, Clone)]
pub struct ConnectionFailed;

impl SessionState for ConnectionFailed {
    const NAME: &'static str = "ConnectionFailed";
    const IS_FINAL: bool = true;
    const CAN_TERMINATE: bool = true;
}

// ========== Union Type for Transport Sessions ==========

/// Union type representing all possible transport session states
#[derive(Debug)]
pub enum TransportSessionState {
    /// Transport is disconnected
    TransportDisconnected(ChoreographicProtocol<TransportProtocolCore, TransportDisconnected>),
    /// Connection handshake in progress
    ConnectionHandshaking(ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking>),
    /// Transport is connected
    TransportConnected(ChoreographicProtocol<TransportProtocolCore, TransportConnected>),
    /// Connection failed (final state)
    ConnectionFailed(ChoreographicProtocol<TransportProtocolCore, ConnectionFailed>),
}

impl TransportSessionState {
    /// Get the current state name
    pub fn state_name(&self) -> &'static str {
        match self {
            TransportSessionState::TransportDisconnected(_) => TransportDisconnected::NAME,
            TransportSessionState::ConnectionHandshaking(_) => ConnectionHandshaking::NAME,
            TransportSessionState::TransportConnected(_) => TransportConnected::NAME,
            TransportSessionState::ConnectionFailed(_) => ConnectionFailed::NAME,
        }
    }

    /// Check if the session can terminate
    pub fn can_terminate(&self) -> bool {
        match self {
            TransportSessionState::TransportDisconnected(_) => TransportDisconnected::CAN_TERMINATE,
            TransportSessionState::ConnectionHandshaking(_) => ConnectionHandshaking::CAN_TERMINATE,
            TransportSessionState::TransportConnected(_) => TransportConnected::CAN_TERMINATE,
            TransportSessionState::ConnectionFailed(_) => ConnectionFailed::CAN_TERMINATE,
        }
    }

    /// Get the device ID from the underlying protocol
    pub fn device_id(&self) -> DeviceId {
        match self {
            TransportSessionState::TransportDisconnected(p) => p.inner.device_id,
            TransportSessionState::ConnectionHandshaking(p) => p.inner.device_id,
            TransportSessionState::TransportConnected(p) => p.inner.device_id,
            TransportSessionState::ConnectionFailed(p) => p.inner.device_id,
        }
    }

    /// Check if this is a final state
    pub fn is_final(&self) -> bool {
        match self {
            TransportSessionState::TransportDisconnected(_) => TransportDisconnected::IS_FINAL,
            TransportSessionState::ConnectionHandshaking(_) => ConnectionHandshaking::IS_FINAL,
            TransportSessionState::TransportConnected(_) => TransportConnected::IS_FINAL,
            TransportSessionState::ConnectionFailed(_) => ConnectionFailed::IS_FINAL,
        }
    }
}

// ========== TransportSession Trait Implementation ==========

impl TransportSession for TransportSessionState {
    fn state_name(&self) -> &'static str {
        self.state_name()
    }

    fn can_terminate(&self) -> bool {
        self.can_terminate()
    }

    fn device_id(&self) -> DeviceId {
        self.device_id()
    }

    fn is_final(&self) -> bool {
        self.is_final()
    }
}

// ========== Runtime Witnesses ==========

/// Witness that connection handshake has completed successfully
///
/// Represents evidence that a peer connection handshake succeeded,
/// including the peer identifier, established connection, and timestamp.
#[derive(Debug, Clone)]
pub struct HandshakeCompleted {
    /// Peer identifier for the completed handshake
    pub peer_id: String,
    /// The established connection to the peer
    pub connection: Connection,
    /// Timestamp when the connection was established
    pub established_at: u64,
}

impl RuntimeWitness for HandshakeCompleted {
    type Evidence = (String, Connection, u64);
    type Config = ();

    fn verify(evidence: (String, Connection, u64), _config: ()) -> Option<Self> {
        let (peer_id, connection, established_at) = evidence;
        Some(HandshakeCompleted {
            peer_id,
            connection,
            established_at,
        })
    }

    fn description(&self) -> &'static str {
        "Transport connection handshake completed successfully"
    }
}

/// Witness for connection failure
///
/// Represents evidence that a connection attempt to a peer failed,
/// including the peer identifier, error details, and failure timestamp.
#[derive(Debug, Clone)]
pub struct ConnectionFailure {
    /// Peer identifier for the failed connection
    pub peer_id: String,
    /// Error message describing the failure reason
    pub error: String,
    /// Timestamp when the failure occurred
    pub failed_at: u64,
}

impl RuntimeWitness for ConnectionFailure {
    type Evidence = (String, TransportError);
    type Config = u64; // Timestamp

    fn verify(evidence: (String, TransportError), timestamp: u64) -> Option<Self> {
        let (peer_id, error) = evidence;
        Some(ConnectionFailure {
            peer_id,
            error: error.to_string(),
            failed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "Transport connection failed"
    }
}

// ========== Factory Functions ==========

/// Create a new session-typed transport protocol in disconnected state
pub fn new_session_typed_transport(
    device_id: DeviceId,
) -> ChoreographicProtocol<TransportProtocolCore, TransportDisconnected> {
    let core = TransportProtocolCore::new(device_id);
    ChoreographicProtocol::new(core)
}

/// Rehydrate transport session from connection state
pub fn rehydrate_transport_session(
    device_id: DeviceId,
    has_connections: bool,
) -> TransportSessionState {
    let core = TransportProtocolCore::new(device_id);

    if has_connections {
        TransportSessionState::TransportConnected(ChoreographicProtocol::new(core))
    } else {
        TransportSessionState::TransportDisconnected(ChoreographicProtocol::new(core))
    }
}

/// Session-typed transport protocol wrapper
pub type SessionTypedTransport<S> = ChoreographicProtocol<TransportProtocolCore, S>;

// ========== Basic State Transitions (No Witnesses Required) ==========

/// Operations available on disconnected transport
pub struct TransportDisconnectedOps;

impl TransportDisconnectedOps {
    /// Begin connection handshake with peer
    pub fn begin_handshake(
        mut protocol: ChoreographicProtocol<TransportProtocolCore, TransportDisconnected>,
        peer_id: String,
        my_ticket: PresenceTicket,
    ) -> ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking> {
        protocol.inner.presence_tickets.insert(peer_id, my_ticket);
        protocol.transition_to()
    }
}

/// Operations available during handshaking
pub struct ConnectionHandshakingOps;

impl ConnectionHandshakingOps {
    /// Complete handshake and transition to connected state
    pub fn complete_handshake(
        protocol: ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking>,
        _peer_ticket: PresenceTicket,
    ) -> ChoreographicProtocol<TransportProtocolCore, TransportConnected> {
        // In reality, would validate peer ticket
        protocol.transition_to()
    }

    /// Handle handshake failure
    pub fn fail_handshake(
        protocol: ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking>,
    ) -> ChoreographicProtocol<TransportProtocolCore, ConnectionFailed> {
        protocol.transition_to()
    }
}

/// Operations available when connected
pub struct TransportConnectedOps;

impl TransportConnectedOps {
    /// Disconnect from peer
    pub fn disconnect(
        protocol: ChoreographicProtocol<TransportProtocolCore, TransportConnected>,
    ) -> ChoreographicProtocol<TransportProtocolCore, TransportDisconnected> {
        protocol.transition_to()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceIdExt;

    #[test]
    fn test_transport_session_creation() {
        let device_id =
            DeviceId::new_with_effects(&aura_crypto::Effects::for_test("transport_test"));
        let transport = new_session_typed_transport(device_id);

        // Check that we can access the inner state correctly
        assert_eq!(transport.inner.device_id, device_id);
        assert!(transport.inner.active_connections.is_empty());
    }

    #[test]
    fn test_session_state_properties() {
        assert_eq!(TransportDisconnected::NAME, "TransportDisconnected");
        assert!(!TransportDisconnected::IS_FINAL);
        assert!(!TransportDisconnected::CAN_TERMINATE);

        assert_eq!(ConnectionFailed::NAME, "ConnectionFailed");
        assert!(ConnectionFailed::IS_FINAL);
        assert!(ConnectionFailed::CAN_TERMINATE);
    }

    #[test]
    fn test_transport_session_state_enum() {
        let device_id = DeviceId::new_with_effects(&aura_crypto::Effects::for_test("state_test"));
        let session = rehydrate_transport_session(device_id, false);

        assert_eq!(session.state_name(), "TransportDisconnected");
        assert_eq!(session.device_id(), device_id);
        assert!(!session.can_terminate());
        assert!(!session.is_final());

        let connected_session = rehydrate_transport_session(device_id, true);
        assert_eq!(connected_session.state_name(), "TransportConnected");
    }
}

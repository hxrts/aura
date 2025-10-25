//! Session Type States for Transport Layer (Refactored with Macros)
//!
//! This module defines session types for the Transport layer, providing compile-time safety
//! for connection management, message exchange, and presence-based authentication.

use crate::{
    ChoreographicProtocol, RuntimeWitness, SessionProtocol, SessionState, WitnessedTransition,
};

// Temporary stub types until cycle is resolved
#[derive(Debug, Clone)]
pub struct Connection {
    pub id: String,
    pub peer_id: String,
}

#[derive(Debug, Clone)]
pub struct PresenceTicket {
    pub device_id: aura_journal::DeviceId,
    pub session_epoch: aura_journal::SessionEpoch,
    pub ticket: Vec<u8>,
    pub issued_at: u64,
    pub expires_at: u64,
    pub ticket_digest: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct BroadcastResult {
    pub succeeded: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TransportError(pub String);

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TransportError {}

use std::collections::BTreeMap;
use std::fmt;
use uuid::Uuid;

// ========== Transport Protocol Core ==========

/// Core transport protocol data without session state
#[derive(Clone)]
pub struct TransportProtocolCore {
    pub device_id: aura_journal::DeviceId,
    pub active_connections: BTreeMap<String, Connection>,
    pub presence_tickets: BTreeMap<String, PresenceTicket>,
    pub pending_broadcasts: Vec<BroadcastContext>,
    pub message_queue: Vec<MessageContext>,
}

impl TransportProtocolCore {
    pub fn new(device_id: aura_journal::DeviceId) -> Self {
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
#[derive(Debug, Clone)]
pub struct BroadcastContext {
    pub broadcast_id: Uuid,
    pub target_peers: Vec<String>,
    pub message: Vec<u8>,
    pub delivery_confirmations: BTreeMap<String, bool>,
}

/// Message context information
#[derive(Debug, Clone)]
pub struct MessageContext {
    pub message_id: Uuid,
    pub peer_id: String,
    pub content: Vec<u8>,
    pub timestamp: u64,
}

// ========== Error Type ==========

/// Errors that can occur in transport session operations
#[derive(Debug, thiserror::Error)]
pub enum TransportSessionError {
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("Invalid presence ticket: {0}")]
    InvalidTicket(String),
    #[error("Connection handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("Message delivery failed: {0}")]
    DeliveryFailed(String),
    #[error("Broadcast operation failed: {0}")]
    BroadcastFailed(String),
    #[error("Session error: {0}")]
    SessionError(String),
}

// ========== Protocol Definition using Macros ==========

define_protocol! {
    Protocol: TransportProtocol,
    Core: TransportProtocolCore,
    Error: TransportSessionError,
    Union: TransportSessionState,

    States {
        TransportDisconnected => (),
        ConnectionHandshaking => Connection,
        TicketValidating => (),
        TransportConnected => (),
        MessageSending => (),
        Broadcasting => BroadcastResult,
        AwaitingMessage => Vec<u8>,
        ProcessingMessage => (),
        RequestResponseActive => Vec<u8>,
        ConnectionFailed @ final => (),
    }

    Extract {
        session_id: |core| {
            // Use device_id hash as session identifier
            let device_hash = blake3::hash(core.device_id.to_string().as_bytes());
            Uuid::from_bytes(device_hash.as_bytes()[..16].try_into().unwrap_or([0u8; 16]))
        },
        device_id: |core| core.device_id,
    }
}

// ========== Protocol Type Alias ==========

/// Session-typed transport protocol wrapper
pub type SessionTypedTransport<S> = ChoreographicProtocol<TransportProtocolCore, S>;

// ========== Runtime Witnesses for Transport Operations ==========

/// Witness that connection handshake has completed successfully
#[derive(Debug, Clone)]
pub struct HandshakeCompleted {
    pub peer_id: String,
    pub connection: Connection,
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

/// Witness that presence tickets have been validated
#[derive(Debug, Clone)]
pub struct TicketsValidated {
    pub my_ticket: PresenceTicket,
    pub peer_ticket: PresenceTicket,
    pub validation_timestamp: u64,
}

impl RuntimeWitness for TicketsValidated {
    type Evidence = (PresenceTicket, PresenceTicket);
    type Config = u64; // Current timestamp

    fn verify(evidence: (PresenceTicket, PresenceTicket), timestamp: u64) -> Option<Self> {
        let (my_ticket, peer_ticket) = evidence;

        // Basic validation - in reality this would include cryptographic verification
        if timestamp < my_ticket.expires_at && timestamp < peer_ticket.expires_at {
            Some(TicketsValidated {
                my_ticket,
                peer_ticket,
                validation_timestamp: timestamp,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Presence tickets validated successfully"
    }
}

/// Witness that message has been delivered successfully
#[derive(Debug, Clone)]
pub struct MessageDelivered {
    pub message_id: Uuid,
    pub peer_id: String,
    pub delivered_at: u64,
}

impl RuntimeWitness for MessageDelivered {
    type Evidence = (Uuid, String);
    type Config = u64; // Timestamp

    fn verify(evidence: (Uuid, String), timestamp: u64) -> Option<Self> {
        let (message_id, peer_id) = evidence;
        Some(MessageDelivered {
            message_id,
            peer_id,
            delivered_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "Message delivered successfully to peer"
    }
}

/// Witness that broadcast operation has completed
#[derive(Debug, Clone)]
pub struct BroadcastCompleted {
    pub broadcast_id: Uuid,
    pub successful_deliveries: Vec<String>,
    pub failed_deliveries: Vec<String>,
    pub completed_at: u64,
}

impl RuntimeWitness for BroadcastCompleted {
    type Evidence = BroadcastResult;
    type Config = (Uuid, u64); // (broadcast_id, timestamp)

    fn verify(evidence: BroadcastResult, config: (Uuid, u64)) -> Option<Self> {
        let (broadcast_id, timestamp) = config;
        Some(BroadcastCompleted {
            broadcast_id,
            successful_deliveries: evidence.succeeded,
            failed_deliveries: evidence.failed,
            completed_at: timestamp,
        })
    }

    fn description(&self) -> &'static str {
        "Broadcast operation completed"
    }
}

/// Witness for connection failure
#[derive(Debug, Clone)]
pub struct ConnectionFailure {
    pub peer_id: String,
    pub error: String,
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

// ========== State Transitions ==========

/// Simple transitions that don't require runtime witnesses
impl ChoreographicProtocol<TransportProtocolCore, TransportDisconnected> {
    /// Begin connection handshake with peer
    pub fn begin_handshake(
        mut self,
        peer_id: String,
        my_ticket: PresenceTicket,
    ) -> ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking> {
        self.inner.presence_tickets.insert(peer_id, my_ticket);
        self.transition_to()
    }
}

impl ChoreographicProtocol<TransportProtocolCore, ConnectionHandshaking> {
    /// Receive peer ticket for validation
    pub fn receive_peer_ticket(
        self,
        _peer_ticket: PresenceTicket,
    ) -> ChoreographicProtocol<TransportProtocolCore, TicketValidating> {
        // In reality, would store peer ticket for validation
        self.transition_to()
    }
}

impl ChoreographicProtocol<TransportProtocolCore, TransportConnected> {
    /// Begin message sending operation
    pub fn send_message_transition(
        mut self,
        message: MessageContext,
    ) -> ChoreographicProtocol<TransportProtocolCore, MessageSending> {
        self.inner.message_queue.push(message);
        self.transition_to()
    }

    /// Begin broadcast operation
    pub fn broadcast_transition(
        mut self,
        broadcast: BroadcastContext,
    ) -> ChoreographicProtocol<TransportProtocolCore, Broadcasting> {
        self.inner.pending_broadcasts.push(broadcast);
        self.transition_to()
    }

    /// Begin waiting for incoming message
    pub fn await_message(self) -> ChoreographicProtocol<TransportProtocolCore, AwaitingMessage> {
        self.transition_to()
    }
}

impl ChoreographicProtocol<TransportProtocolCore, AwaitingMessage> {
    /// Process received message
    pub fn process_received_message(
        mut self,
        message: MessageContext,
    ) -> ChoreographicProtocol<TransportProtocolCore, ProcessingMessage> {
        self.inner.message_queue.push(message);
        self.transition_to()
    }
}

impl ChoreographicProtocol<TransportProtocolCore, ProcessingMessage> {
    /// Complete message processing
    pub fn complete_processing(
        self,
    ) -> ChoreographicProtocol<TransportProtocolCore, TransportConnected> {
        self.transition_to()
    }
}

/// Witnessed transitions that require runtime validation
impl WitnessedTransition<TicketValidating, TransportConnected>
    for ChoreographicProtocol<TransportProtocolCore, TicketValidating>
{
    type Witness = TicketsValidated;
    type Target = ChoreographicProtocol<TransportProtocolCore, TransportConnected>;

    /// Complete ticket validation and establish connection
    fn transition_with_witness(self, _witness: Self::Witness) -> Self::Target {
        self.transition_to()
    }
}

impl WitnessedTransition<MessageSending, TransportConnected>
    for ChoreographicProtocol<TransportProtocolCore, MessageSending>
{
    type Witness = MessageDelivered;
    type Target = ChoreographicProtocol<TransportProtocolCore, TransportConnected>;

    /// Complete message delivery
    fn transition_with_witness(mut self, _witness: Self::Witness) -> Self::Target {
        // Remove delivered message from queue
        if !self.inner.message_queue.is_empty() {
            self.inner.message_queue.remove(0);
        }
        self.transition_to()
    }
}

impl WitnessedTransition<Broadcasting, TransportConnected>
    for ChoreographicProtocol<TransportProtocolCore, Broadcasting>
{
    type Witness = BroadcastCompleted;
    type Target = ChoreographicProtocol<TransportProtocolCore, TransportConnected>;

    /// Complete broadcast operation
    fn transition_with_witness(mut self, _witness: Self::Witness) -> Self::Target {
        // Remove completed broadcast
        if !self.inner.pending_broadcasts.is_empty() {
            self.inner.pending_broadcasts.remove(0);
        }
        self.transition_to()
    }
}

/// Transition to ConnectionFailed from any connected state (requires ConnectionFailure witness)
impl<S: SessionState> WitnessedTransition<S, ConnectionFailed>
    for ChoreographicProtocol<TransportProtocolCore, S>
where
    Self: SessionProtocol<State = S, Error = TransportSessionError>,
{
    type Witness = ConnectionFailure;
    type Target = ChoreographicProtocol<TransportProtocolCore, ConnectionFailed>;

    /// Handle connection failure
    fn transition_with_witness(mut self, _witness: Self::Witness) -> Self::Target {
        // Clear connection state on failure
        self.inner.active_connections.clear();
        self.inner.message_queue.clear();
        self.inner.pending_broadcasts.clear();
        self.transition_to()
    }
}

// ========== State-Specific Operations ==========

/// Operations only available in TransportConnected state
impl ChoreographicProtocol<TransportProtocolCore, TransportConnected> {
    /// Send a message to a specific peer
    #[allow(clippy::disallowed_methods)]
    pub async fn send_message(
        &self,
        peer_id: &str,
        message: &[u8],
    ) -> Result<MessageContext, TransportSessionError> {
        let message_context = MessageContext {
            message_id: Uuid::new_v4(),
            peer_id: peer_id.to_string(),
            content: message.to_vec(),
            timestamp: 0, // Would use actual timestamp
        };

        Ok(message_context)
    }

    /// Initiate broadcast to multiple peers
    #[allow(clippy::disallowed_methods)]
    pub async fn initiate_broadcast(
        &self,
        target_peers: Vec<String>,
        message: &[u8],
    ) -> Result<BroadcastContext, TransportSessionError> {
        let broadcast_context = BroadcastContext {
            broadcast_id: Uuid::new_v4(),
            target_peers,
            message: message.to_vec(),
            delivery_confirmations: BTreeMap::new(),
        };

        Ok(broadcast_context)
    }

    /// Check connection health
    pub fn is_healthy(&self) -> bool {
        !self.inner.active_connections.is_empty()
    }

    /// Get active connection count
    pub fn connection_count(&self) -> usize {
        self.inner.active_connections.len()
    }
}

/// Operations only available in MessageSending state
impl ChoreographicProtocol<TransportProtocolCore, MessageSending> {
    /// Check message delivery status
    pub async fn check_delivery_status(&self) -> Option<MessageDelivered> {
        // In reality, this would check with the transport layer
        if let Some(message) = self.inner.message_queue.first() {
            MessageDelivered::verify((message.message_id, message.peer_id.clone()), 0)
        } else {
            None
        }
    }
}

/// Operations only available in Broadcasting state
impl ChoreographicProtocol<TransportProtocolCore, Broadcasting> {
    /// Check broadcast completion status
    pub async fn check_broadcast_status(&self) -> Option<BroadcastCompleted> {
        // In reality, this would aggregate delivery confirmations
        if let Some(broadcast) = self.inner.pending_broadcasts.first() {
            let result = BroadcastResult {
                succeeded: broadcast.target_peers.clone(),
                failed: vec![],
            };
            BroadcastCompleted::verify(result, (broadcast.broadcast_id, 0))
        } else {
            None
        }
    }

    /// Get broadcast progress
    pub fn broadcast_progress(&self) -> (usize, usize) {
        if let Some(broadcast) = self.inner.pending_broadcasts.first() {
            let confirmed = broadcast
                .delivery_confirmations
                .values()
                .filter(|&&v| v)
                .count();
            let total = broadcast.target_peers.len();
            (confirmed, total)
        } else {
            (0, 0)
        }
    }
}

// ========== Additional Union Type Methods ==========

// ========== Factory Functions ==========

/// Create a new session-typed transport protocol in disconnected state
pub fn new_session_typed_transport(
    device_id: aura_journal::DeviceId,
) -> ChoreographicProtocol<TransportProtocolCore, TransportDisconnected> {
    let core = TransportProtocolCore::new(device_id);
    ChoreographicProtocol::new(core)
}

/// Rehydrate transport session from connection state
pub fn rehydrate_transport_session(
    device_id: aura_journal::DeviceId,
    has_connections: bool,
) -> TransportSessionState {
    let core = TransportProtocolCore::new(device_id);

    if has_connections {
        TransportSessionState::TransportConnected(ChoreographicProtocol::new(core))
    } else {
        TransportSessionState::TransportDisconnected(ChoreographicProtocol::new(core))
    }
}

// ========== Tests ==========

#[allow(clippy::disallowed_methods, clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_transport_session_creation() {
        let device_id = aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test(
            "transport_test",
        ));
        let transport = new_session_typed_transport(device_id);

        assert_eq!(transport.state_name(), "TransportDisconnected");
        assert!(!transport.can_terminate());
        assert!(!transport.is_final());
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_handshake_transitions() {
        let device_id = aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test(
            "handshake_test",
        ));
        let transport = new_session_typed_transport(device_id);

        // Simulate handshake initiation
        let my_ticket = PresenceTicket {
            device_id,
            session_epoch: aura_journal::SessionEpoch::initial(),
            ticket: vec![1, 2, 3],
            issued_at: 0,
            expires_at: 3600,
            ticket_digest: [0u8; 32],
        };

        let handshaking = transport.begin_handshake("peer1".to_string(), my_ticket.clone());
        assert_eq!(handshaking.state_name(), "ConnectionHandshaking");

        let peer_ticket = PresenceTicket {
            device_id: aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test(
                "peer_test",
            )),
            session_epoch: aura_journal::SessionEpoch::initial(),
            ticket: vec![4, 5, 6],
            issued_at: 0,
            expires_at: 3600,
            ticket_digest: [1u8; 32],
        };

        let validating = handshaking.receive_peer_ticket(peer_ticket.clone());
        assert_eq!(validating.state_name(), "TicketValidating");

        let witness = TicketsValidated::verify((my_ticket, peer_ticket), 100).unwrap();
        let connected = <ChoreographicProtocol<TransportProtocolCore, TicketValidating> as WitnessedTransition<TicketValidating, TransportConnected>>::transition_with_witness(validating, witness);
        assert_eq!(connected.state_name(), "TransportConnected");
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_message_sending() {
        let device_id = aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test(
            "message_test",
        ));
        let core = TransportProtocolCore::new(device_id);
        let connected = ChoreographicProtocol::<_, TransportConnected>::new(core);

        let message = MessageContext {
            message_id: Uuid::new_v4(),
            peer_id: "peer1".to_string(),
            content: vec![1, 2, 3, 4],
            timestamp: 1000,
        };

        let sending = connected.send_message_transition(message.clone());
        assert_eq!(sending.state_name(), "MessageSending");

        let delivery_witness =
            MessageDelivered::verify((message.message_id, message.peer_id), 1100).unwrap();
        let back_to_connected =
            <ChoreographicProtocol<TransportProtocolCore, MessageSending> as WitnessedTransition<
                MessageSending,
                TransportConnected,
            >>::transition_with_witness(sending, delivery_witness);
        assert_eq!(back_to_connected.state_name(), "TransportConnected");
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_broadcast_operation() {
        let device_id = aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test(
            "broadcast_test",
        ));
        let core = TransportProtocolCore::new(device_id);
        let connected = ChoreographicProtocol::<_, TransportConnected>::new(core);

        let broadcast = BroadcastContext {
            broadcast_id: Uuid::new_v4(),
            target_peers: vec!["peer1".to_string(), "peer2".to_string()],
            message: vec![1, 2, 3],
            delivery_confirmations: BTreeMap::new(),
        };

        let broadcasting = connected.broadcast_transition(broadcast.clone());
        assert_eq!(broadcasting.state_name(), "Broadcasting");

        let result = BroadcastResult {
            succeeded: vec!["peer1".to_string(), "peer2".to_string()],
            failed: vec![],
        };
        let completion_witness =
            BroadcastCompleted::verify(result, (broadcast.broadcast_id, 2000)).unwrap();
        let completed =
            <ChoreographicProtocol<TransportProtocolCore, Broadcasting> as WitnessedTransition<
                Broadcasting,
                TransportConnected,
            >>::transition_with_witness(broadcasting, completion_witness);
        assert_eq!(completed.state_name(), "TransportConnected");
    }

    #[allow(clippy::disallowed_methods)]
    #[test]
    fn test_session_state_union() {
        let device_id =
            aura_journal::DeviceId::new_with_effects(&aura_crypto::Effects::for_test("union_test"));
        let session = rehydrate_transport_session(device_id, false);

        assert_eq!(session.state_name(), "TransportDisconnected");
        assert_eq!(session.device_id(), device_id);
        assert!(!session.can_terminate());
        assert!(!session.is_final());

        let connected_session = rehydrate_transport_session(device_id, true);
        assert_eq!(connected_session.state_name(), "TransportConnected");
    }
}

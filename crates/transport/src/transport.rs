//! Unified transport layer with session types and capability-driven messaging
//!
//! This module provides a comprehensive transport abstraction that includes:
//! - Protocol-agnostic Transport trait for basic communication
//! - Session-typed transport integration for compile-time safety
//! - Capability-driven messaging with authentication and authorization
//! - Connection management and presence ticket verification
//!
//! Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design

use crate::{PresenceTicket, Result, TransportError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

// Session types integration - now active
use aura_session_types::{
    TransportSessionState, new_session_typed_transport,
};

// Re-exports for capability-driven transport
use aura_crypto::{DeviceKeyManager, Effects};
use aura_groups::{events::KeyhiveCgkaOperation, state::CgkaState};
use aura_journal::{
    capability::{
        authority_graph::AuthorityGraph,
        events::{CapabilityDelegation, CapabilityRevocation},
        identity::IndividualId,
        types::{CapabilityResult, CapabilityScope},
    },
    events::{CgkaEpochTransitionEvent, CgkaStateSyncEvent},
    DeviceId,
};

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

/// Result of a broadcast operation
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Peers that successfully received the message
    pub succeeded: Vec<String>,
    /// Peers that failed to receive the message
    pub failed: Vec<String>,
}

/// Transport trait - defines protocol-agnostic communication
///
/// Reference: 080 spec Part 5: Transport Abstraction Design
///
/// All transport implementations must implement this trait.
/// The trait is designed to support:
/// - Point-to-point messaging
/// - Broadcast to multiple peers
/// - Presence ticket verification during handshake
/// - Connection lifecycle management
#[async_trait]
pub trait Transport: Send + Sync {
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
    ) -> Result<Connection>;

    /// Send a message to a peer
    ///
    /// The message is sent over the encrypted channel established during connect().
    async fn send(&self, conn: &Connection, message: &[u8]) -> Result<()>;

    /// Receive a message from a peer with timeout
    ///
    /// Returns None if timeout is reached without receiving a message.
    async fn receive(&self, conn: &Connection, timeout: Duration) -> Result<Option<Vec<u8>>>;

    /// Broadcast a message to multiple peers
    ///
    /// Returns which peers successfully received the message and which failed.
    /// This is a convenience method - implementations may optimize it or simply
    /// call send() in a loop.
    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> Result<BroadcastResult>;

    /// Disconnect from a peer
    ///
    /// Closes the connection and releases resources.
    async fn disconnect(&self, conn: &Connection) -> Result<()>;

    /// Check if a connection is still active
    ///
    /// Returns false if the connection has been closed or is no longer valid.
    async fn is_connected(&self, conn: &Connection) -> bool;
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

/// Capability-authenticated message for transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMessage {
    /// Message identifier
    pub message_id: Uuid,
    /// Sender identity
    pub sender: IndividualId,
    /// Target recipients (None for broadcast)
    pub recipients: Option<BTreeSet<IndividualId>>,
    /// Required capability scope for delivery
    pub required_scope: CapabilityScope,
    /// Message content
    pub content: MessageContent,
    /// Timestamp
    pub timestamp: u64,
    /// Cryptographic signature
    pub signature: Vec<u8>,
}

/// Message content types for capability transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    /// Capability delegation event
    CapabilityDelegation(CapabilityDelegation),
    /// Capability revocation event
    CapabilityRevocation(CapabilityRevocation),
    /// CGKA operation
    CgkaOperation(KeyhiveCgkaOperation),
    /// CGKA state synchronization
    CgkaStateSync(CgkaStateSyncEvent),
    /// CGKA epoch transition
    CgkaEpochTransition(CgkaEpochTransitionEvent),
    /// General data with capability requirements
    Data { data: Vec<u8>, context: String },
    /// Delivery confirmation for a message
    DeliveryConfirmation {
        original_message_id: Uuid,
        confirmed_by: IndividualId,
        timestamp: u64,
    },
}

/// Authenticated message wrapper - proves sender identity has been verified
#[derive(Debug)]
pub struct AuthenticatedMessage {
    /// Original message with verified signature
    pub original_message: CapabilityMessage,
    /// Whether sender identity was verified via signature
    pub verified_sender: bool,
}

/// Message handler for specific content types
pub type CapabilityMessageHandler =
    Box<dyn Fn(&CapabilityMessage) -> std::result::Result<(), TransportError> + Send + Sync>;

/// Session-typed transport adapter with compile-time safety
pub struct SessionTypedTransportAdapter {
    /// Device identifier
    _device_id: DeviceId,
    /// Underlying transport implementation
    transport: Arc<dyn Transport>,
    /// Current transport session state
    transport_session: Arc<RwLock<TransportSessionState>>,
    /// Active connections by peer ID
    connections: Arc<RwLock<BTreeMap<String, Connection>>>,
    /// Event sender to local session runtime
    event_sender: Option<tokio::sync::mpsc::UnboundedSender<TransportEvent>>,
}

/// Events that the transport sends to the session runtime
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Connection established with peer
    ConnectionEstablished { peer_id: String },
    /// Connection lost with peer
    ConnectionLost { peer_id: String },
    /// Message received from peer
    MessageReceived { peer_id: String, message: Vec<u8> },
    /// Message sent to peer
    MessageSent { peer_id: String, message_size: usize },
    /// Transport error occurred
    TransportError { error: String },
}

impl SessionTypedTransportAdapter {
    /// Create a new session-typed transport adapter
    pub fn new(
        device_id: DeviceId,
        transport: Arc<dyn Transport>,
    ) -> Self {
        info!(
            "Creating session-typed transport adapter for device {}",
            device_id
        );

        let transport_session = new_session_typed_transport(device_id);
        let session_state = TransportSessionState::TransportDisconnected(transport_session);

        Self {
            _device_id: device_id,
            transport,
            transport_session: Arc::new(RwLock::new(session_state)),
            connections: Arc::new(RwLock::new(BTreeMap::new())),
            event_sender: None,
        }
    }
    
    /// Create a new session-typed transport adapter with event sender
    pub fn with_event_sender(
        device_id: DeviceId,
        transport: Arc<dyn Transport>,
        event_sender: tokio::sync::mpsc::UnboundedSender<TransportEvent>,
    ) -> Self {
        info!(
            "Creating session-typed transport adapter with event sender for device {}",
            device_id
        );

        let transport_session = new_session_typed_transport(device_id);
        let session_state = TransportSessionState::TransportDisconnected(transport_session);

        Self {
            _device_id: device_id,
            transport,
            transport_session: Arc::new(RwLock::new(session_state)),
            connections: Arc::new(RwLock::new(BTreeMap::new())),
            event_sender: Some(event_sender),
        }
    }

    /// Connect to a peer with session type safety
    pub async fn connect_with_session_types(
        &self,
        peer_id: &str,
        my_ticket: &PresenceTicket,
        peer_ticket: &PresenceTicket,
    ) -> Result<SessionTypedConnection> {
        // Check current transport state
        let current_state = {
            let session = self.transport_session.read().await;
            session.state_name().to_string()
        };

        // Can only connect from disconnected state
        if current_state != "TransportDisconnected" {
            return Err(crate::TransportError::InvalidState(format!(
                "Cannot connect in state {}, must be disconnected",
                current_state
            ))
            .into());
        }

        debug!("Connecting to peer {} with session type safety", peer_id);

        // Perform actual connection
        let connection = self
            .transport
            .connect(peer_id, my_ticket, peer_ticket)
            .await?;

        // Store connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(peer_id.to_string(), connection.clone());
        }

        // Send connection established event to session runtime
        if let Some(event_sender) = &self.event_sender {
            let event = TransportEvent::ConnectionEstablished {
                peer_id: peer_id.to_string(),
            };
            let _ = event_sender.send(event);
        }

        // Update session state to reflect successful connection
        // Note: Real implementation would use session type transitions
        info!(
            "Successfully connected to peer {} with session types",
            peer_id
        );

        Ok(SessionTypedConnection {
            connection,
            peer_id: peer_id.to_string(),
            transport_session: self.transport_session.clone(),
            event_sender: self.event_sender.clone(),
        })
    }

    /// Get current transport session state
    pub async fn get_current_state(&self) -> String {
        let session = self.transport_session.read().await;
        session.state_name().to_string()
    }

    /// Check if transport can safely disconnect
    pub async fn can_disconnect(&self) -> bool {
        let session = self.transport_session.read().await;
        let current_state = session.state_name();
        current_state == "TransportConnected" || current_state == "ConnectionHandshaking"
    }

    /// Disconnect with session type safety
    pub async fn disconnect_with_session_types(&self, peer_id: &str) -> Result<()> {
        // Check if we can disconnect
        if !self.can_disconnect().await {
            return Err(crate::TransportError::InvalidState(
                "Cannot disconnect from current state".to_string(),
            )
            .into());
        }

        // Get and remove connection
        let connection = {
            let mut connections = self.connections.write().await;
            connections.remove(peer_id)
        };

        if let Some(conn) = connection {
            self.transport.disconnect(&conn).await?;
            
            // Send connection lost event to session runtime
            if let Some(event_sender) = &self.event_sender {
                let event = TransportEvent::ConnectionLost {
                    peer_id: peer_id.to_string(),
                };
                let _ = event_sender.send(event);
            }
        }

        info!("Disconnected from peer {} with session types", peer_id);
        Ok(())
    }
}

/// Session-typed connection with compile-time state safety
pub struct SessionTypedConnection {
    /// Underlying connection
    connection: Connection,
    /// Peer identifier
    peer_id: String,
    /// Transport session state
    transport_session: Arc<RwLock<TransportSessionState>>,
    /// Event sender to local session runtime
    event_sender: Option<tokio::sync::mpsc::UnboundedSender<TransportEvent>>,
}

impl SessionTypedConnection {
    /// Send message with session type safety
    pub async fn send_with_session_types(&self, message: &[u8]) -> Result<()> {
        // Verify we're in connected state
        let current_state = {
            let session = self.transport_session.read().await;
            session.state_name().to_string()
        };

        if current_state != "TransportConnected" {
            return Err(crate::TransportError::InvalidState(format!(
                "Cannot send message in state {}, must be connected",
                current_state
            ))
            .into());
        }

        // TODO: Use actual transport send - this is a placeholder
        debug!(
            "Sending {} bytes to peer {} with session types",
            message.len(),
            self.peer_id
        );
        
        // Send message sent event to session runtime
        if let Some(event_sender) = &self.event_sender {
            let event = TransportEvent::MessageSent {
                peer_id: self.peer_id.clone(),
                message_size: message.len(),
            };
            let _ = event_sender.send(event);
        }
        
        Ok(())
    }

    /// Receive message with session type safety
    pub async fn receive_with_session_types(&self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        // Verify we're in connected state
        let current_state = {
            let session = self.transport_session.read().await;
            session.state_name().to_string()
        };

        if current_state != "TransportConnected" {
            return Err(crate::TransportError::InvalidState(format!(
                "Cannot receive message in state {}, must be connected",
                current_state
            ))
            .into());
        }

        // TODO: Use actual transport receive - this is a placeholder
        debug!(
            "Receiving message from peer {} with session types (timeout: {:?})",
            self.peer_id, timeout
        );
        
        // For demonstration, simulate receiving a message
        let mock_message = vec![1, 2, 3, 4]; // Mock message
        
        // Send message received event to session runtime
        if let Some(event_sender) = &self.event_sender {
            let event = TransportEvent::MessageReceived {
                peer_id: self.peer_id.clone(),
                message: mock_message.clone(),
            };
            let _ = event_sender.send(event);
        }
        
        Ok(Some(mock_message))
    }

    /// Get peer ID
    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }

    /// Get connection ID
    pub fn connection_id(&self) -> &str {
        self.connection.id()
    }
}

/// Capability transport adapter that wraps any Transport implementation
pub struct CapabilityTransportAdapter<T: Transport> {
    /// Underlying transport implementation
    transport: Arc<T>,
    /// Individual identity
    individual_id: IndividualId,
    /// Device key manager for message signing and verification
    device_key_manager: Arc<RwLock<DeviceKeyManager>>,
    /// Authority graph for capability evaluation
    authority_graph: RwLock<AuthorityGraph>,
    /// Message handlers by content type
    _handlers: RwLock<BTreeMap<String, CapabilityMessageHandler>>,
    /// Pending outbound messages
    outbound_queue: RwLock<BTreeMap<Uuid, CapabilityMessage>>,
    /// Active connections by peer ID
    connections: RwLock<BTreeMap<String, Connection>>,
    /// Connected peers mapped to their individual IDs
    connected_peers: RwLock<BTreeMap<String, IndividualId>>,
    /// Message delivery confirmations
    _delivery_confirmations: RwLock<BTreeMap<Uuid, BTreeSet<IndividualId>>>,
    /// CGKA state manager for group operations
    _cgka_states: RwLock<BTreeMap<String, CgkaState>>,
    /// Injectable effects for deterministic testing
    effects: Effects,
}

impl<T: Transport> CapabilityTransportAdapter<T> {
    /// Create new capability transport adapter
    pub fn new(
        transport: Arc<T>,
        individual_id: IndividualId,
        device_key_manager: DeviceKeyManager,
        effects: Effects,
    ) -> Self {
        info!(
            "Creating capability transport adapter for individual: {}",
            individual_id.0
        );

        Self {
            transport,
            individual_id,
            device_key_manager: Arc::new(RwLock::new(device_key_manager)),
            authority_graph: RwLock::new(AuthorityGraph::new()),
            _handlers: RwLock::new(BTreeMap::new()),
            outbound_queue: RwLock::new(BTreeMap::new()),
            connections: RwLock::new(BTreeMap::new()),
            connected_peers: RwLock::new(BTreeMap::new()),
            _delivery_confirmations: RwLock::new(BTreeMap::new()),
            _cgka_states: RwLock::new(BTreeMap::new()),
            effects,
        }
    }

    /// Send capability delegation to peers
    pub async fn send_capability_delegation(
        &self,
        delegation: CapabilityDelegation,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<Uuid> {
        info!(
            "Sending capability delegation: {}",
            delegation.capability_id.as_hex()
        );

        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: recipients.clone(),
            required_scope: CapabilityScope::simple("capability", "propagate"),
            content: MessageContent::CapabilityDelegation(delegation),
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };

        // Sign message with device key
        message.signature = self.sign_message(&message)?;

        self.send_message(message).await
    }

    /// Send capability revocation to peers
    pub async fn send_capability_revocation(
        &self,
        revocation: CapabilityRevocation,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<Uuid> {
        info!(
            "Sending capability revocation: {}",
            revocation.capability_id.as_hex()
        );

        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: recipients.clone(),
            required_scope: CapabilityScope::simple("capability", "revoke"),
            content: MessageContent::CapabilityRevocation(revocation),
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };

        // Sign message with device key
        message.signature = self.sign_message(&message)?;

        self.send_message(message).await
    }

    /// Send data with capability requirements
    pub async fn send_data(
        &self,
        data: Vec<u8>,
        context: String,
        required_scope: CapabilityScope,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<Uuid> {
        info!(
            "Sending data message with context: {} (scope: {}:{})",
            context, required_scope.namespace, required_scope.operation
        );

        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: recipients.clone(),
            required_scope,
            content: MessageContent::Data { data, context },
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };

        // Sign message with device key
        message.signature = self.sign_message(&message)?;

        self.send_message(message).await
    }

    /// Update the authority graph used for capability evaluation
    pub async fn update_authority_graph(&self, authority_graph: AuthorityGraph) {
        info!("Updating authority graph");
        let mut graph = self.authority_graph.write().await;
        *graph = authority_graph;
    }

    /// Get count of pending outbound messages
    pub async fn pending_messages_count(&self) -> usize {
        let queue = self.outbound_queue.read().await;
        queue.len()
    }

    /// Flush the outbound message queue
    pub async fn flush_outbound_queue(&self) {
        info!("Flushing outbound message queue");
        let mut queue = self.outbound_queue.write().await;
        queue.clear();
    }

    /// Send message with capability authentication
    async fn send_message(&self, message: CapabilityMessage) -> Result<Uuid> {
        // Verify sender has required capability
        let graph = self.authority_graph.read().await;
        let sender_subject = message.sender.to_subject();
        let result =
            graph.evaluate_capability(&sender_subject, &message.required_scope, &self.effects);

        if !matches!(result, CapabilityResult::Granted) {
            return Err(crate::TransportError::InsufficientCapability(format!(
                "Sender {} lacks required capability {}:{}",
                message.sender.0,
                message.required_scope.namespace,
                message.required_scope.operation
            ))
            .into());
        }

        let message_id = message.message_id;

        // Serialize message for transport
        let message_bytes = bincode::serialize(&message)
            .map_err(|e| crate::TransportError::Transport(format!("Serialization error: {}", e)))?;

        // Send to recipients via underlying transport
        if let Some(recipients) = &message.recipients {
            // Send to specific recipients
            for recipient in recipients {
                let device_id = self.individual_to_device_id(recipient);
                if let Some(connection) = self.get_connection(&device_id).await {
                    if let Err(e) = self.transport.send(&connection, &message_bytes).await {
                        warn!("Failed to send message to {}: {:?}", recipient.0, e);
                    }
                }
            }
        } else {
            // Broadcast to all connected peers
            let connections: Vec<Connection> = {
                let connections_map = self.connections.read().await;
                connections_map.values().cloned().collect()
            };

            if !connections.is_empty() {
                let _result = self
                    .transport
                    .broadcast(&connections, &message_bytes)
                    .await?;
            }
        }

        // Add to outbound queue for tracking
        {
            let mut queue = self.outbound_queue.write().await;
            queue.insert(message_id, message.clone());
        }

        debug!("Message {} sent via underlying transport", message_id);
        Ok(message_id)
    }

    /// Get connection for a peer device ID
    async fn get_connection(&self, peer_device_id: &DeviceId) -> Option<Connection> {
        let connections = self.connections.read().await;
        connections.get(&peer_device_id.0.to_string()).cloned()
    }

    /// Convert IndividualId to peer device ID for transport
    fn individual_to_device_id(&self, individual_id: &IndividualId) -> DeviceId {
        // For now, assume 1:1 mapping between individual and device
        DeviceId(Uuid::parse_str(&individual_id.0).unwrap_or_else(|_| self.effects.gen_uuid()))
    }

    /// Sign a capability message with device key
    fn sign_message(&self, message: &CapabilityMessage) -> Result<Vec<u8>> {
        let rt = tokio::runtime::Handle::current();

        let signature = rt.block_on(async {
            // Create message hash for signing (excluding signature field)
            let signable_content = bincode::serialize(&(
                &message.message_id,
                &message.sender,
                &message.recipients,
                &message.required_scope,
                &message.content,
                message.timestamp,
            ))
            .map_err(|e| crate::TransportError::Transport(format!("Serialization error: {}", e)))?;

            // Use actual device key manager for signing
            let device_key_manager = self.device_key_manager.read().await;
            device_key_manager
                .sign_message(&signable_content)
                .map_err(|e| {
                    crate::TransportError::Transport(format!("Device key signing failed: {:?}", e))
                })
        })?;

        Ok(signature)
    }
    
    /// Remove a peer from transport and cleanup connections
    pub async fn remove_peer(&self, peer: &IndividualId) {
        info!("Removing peer: {}", peer.0);
        
        // Remove from connected peers
        {
            let mut peers = self.connected_peers.write().await;
            peers.remove(&peer.0);
        }
        
        // Remove connection
        {
            let mut connections = self.connections.write().await;
            connections.remove(&peer.0);
        }
        
        info!("Peer {} removed successfully", peer.0);
    }
    
    /// Get list of connected peers
    pub async fn get_peers(&self) -> Vec<IndividualId> {
        let peers = self.connected_peers.read().await;
        peers.values().cloned().collect()
    }
}

/// Factory for creating session-typed transport adapters
pub struct SessionTransportFactory;

impl SessionTransportFactory {
    /// Create a new session-typed transport adapter
    pub fn create_session_transport(
        device_id: DeviceId,
        transport: Arc<dyn Transport>,
    ) -> SessionTypedTransportAdapter {
        SessionTypedTransportAdapter::new(device_id, transport)
    }
    
    /// Create a new session-typed transport adapter with event sender
    pub fn create_session_transport_with_events(
        device_id: DeviceId,
        transport: Arc<dyn Transport>,
        event_sender: tokio::sync::mpsc::UnboundedSender<TransportEvent>,
    ) -> SessionTypedTransportAdapter {
        SessionTypedTransportAdapter::with_event_sender(device_id, transport, event_sender)
    }

    /// Create a capability transport adapter
    pub fn create_capability_transport<T: Transport>(
        transport: Arc<T>,
        individual_id: IndividualId,
        device_key_manager: DeviceKeyManager,
        effects: Effects,
    ) -> CapabilityTransportAdapter<T> {
        CapabilityTransportAdapter::new(transport, individual_id, device_key_manager, effects)
    }
}

// Type alias for convenience
pub type CapabilityTransport = CapabilityTransportAdapter<crate::StubTransport>;

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

    #[tokio::test]
    async fn test_session_typed_transport_states() {
        let device_id = DeviceId(Uuid::new_v4());
        let transport = Arc::new(crate::StubTransport::new());

        let session_transport = SessionTypedTransportAdapter::new(device_id, transport);

        // Should start in disconnected state
        assert_eq!(session_transport.get_current_state().await, "TransportDisconnected");
    }
}

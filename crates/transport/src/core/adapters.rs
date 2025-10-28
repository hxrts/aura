//! Transport adapters for capability-driven messaging and session types
//!
//! This module provides higher-level transport abstractions that wrap
//! the core transport interfaces with additional functionality:
//! - Capability-driven messaging with authentication and authorization
//! - Session-typed transport for compile-time safety (when enabled)

use crate::{
    ConnectionManager, TransportError, TransportErrorBuilder, TransportResult,
    Connection, BroadcastResult,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

// Re-exports for capability-driven transport
use aura_crypto::{DeviceKeyManager, Effects};
use aura_journal::capability::{
    authority_graph::AuthorityGraph,
    events::{CapabilityDelegation, CapabilityRevocation},
    identity::IndividualId,
    types::{CapabilityResult, CapabilityScope},
};
use aura_types::DeviceId;

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

/// Capability transport adapter that wraps any ConnectionManager implementation
pub struct CapabilityTransportAdapter<T: ConnectionManager> {
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
    /// Injectable effects for deterministic testing
    effects: Effects,
}

impl<T: ConnectionManager> CapabilityTransportAdapter<T> {
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
            effects,
        }
    }

    /// Send capability delegation to peers
    pub async fn send_capability_delegation(
        &self,
        delegation: CapabilityDelegation,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> TransportResult<Uuid> {
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
    ) -> TransportResult<Uuid> {
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
    ) -> TransportResult<Uuid> {
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
    async fn send_message(&self, message: CapabilityMessage) -> TransportResult<Uuid> {
        // Verify sender has required capability
        let graph = self.authority_graph.read().await;
        let sender_subject = message.sender.to_subject();
        let result =
            graph.evaluate_capability(&sender_subject, &message.required_scope, &self.effects);

        if !matches!(result, CapabilityResult::Granted) {
            return Err(
                TransportErrorBuilder::insufficient_capability(format!(
                    "Sender {} lacks required capability {}:{}",
                    message.sender.0,
                    message.required_scope.namespace,
                    message.required_scope.operation
                ))
                .into(),
            );
        }

        let message_id = message.message_id;

        // Serialize message for transport
        let message_bytes = bincode::serialize(&message).map_err(|e| {
            TransportErrorBuilder::transport(format!("Serialization error: {}", e))
        })?;

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
    fn sign_message(&self, message: &CapabilityMessage) -> TransportResult<Vec<u8>> {
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
            .map_err(|e| {
                TransportErrorBuilder::transport(format!("Serialization error: {}", e))
            })?;

            // Use actual device key manager for signing
            let device_key_manager = self.device_key_manager.read().await;
            device_key_manager
                .sign_message(&signable_content)
                .map_err(|e| {
                    TransportErrorBuilder::transport(format!(
                        "Device key signing failed: {:?}",
                        e
                    ))
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

/// Factory for creating transport adapters
pub struct TransportAdapterFactory;

impl TransportAdapterFactory {
    /// Create a capability transport adapter
    pub fn create_capability_transport<T: ConnectionManager>(
        transport: Arc<T>,
        individual_id: IndividualId,
        device_key_manager: DeviceKeyManager,
        effects: Effects,
    ) -> CapabilityTransportAdapter<T> {
        CapabilityTransportAdapter::new(transport, individual_id, device_key_manager, effects)
    }
}

// Type alias for convenience with MemoryTransport
pub type CapabilityTransport = CapabilityTransportAdapter<crate::MemoryTransport>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_message_serialization() {
        let message = CapabilityMessage {
            message_id: Uuid::new_v4(),
            sender: IndividualId("test_sender".to_string()),
            recipients: None,
            required_scope: CapabilityScope::simple("test", "read"),
            content: MessageContent::Data {
                data: b"test data".to_vec(),
                context: "test context".to_string(),
            },
            timestamp: 1234567890,
            signature: vec![1, 2, 3, 4],
        };

        let serialized = bincode::serialize(&message).unwrap();
        let deserialized: CapabilityMessage = bincode::deserialize(&serialized).unwrap();

        assert_eq!(message.message_id, deserialized.message_id);
        assert_eq!(message.sender, deserialized.sender);
    }
}
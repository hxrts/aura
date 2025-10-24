// Capability-driven transport adapter that wraps any Transport implementation

use crate::{transport::Transport, Connection, PresenceTicket};
use aura_journal::{
    capability::{
        events::{CapabilityDelegation, CapabilityRevocation},
        identity::IndividualId,
        types::{CapabilityScope, CapabilityResult},
        authority_graph::AuthorityGraph,
    },
    events::{CgkaStateSyncEvent, CgkaEpochTransitionEvent},
    DeviceId,
};
use aura_cgka::{
    events::{KeyhiveCgkaOperation, OperationId},
    state::CgkaState,
};
use aura_crypto::DeviceKeyManager;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;
use std::sync::Arc;

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
    Data {
        data: Vec<u8>,
        context: String,
    },
    /// Delivery confirmation for a message
    DeliveryConfirmation {
        original_message_id: Uuid,
        confirmed_by: IndividualId,
        timestamp: u64,
    },
}

/// Capability transport adapter that wraps any Transport implementation
pub struct CapabilityTransportAdapter<T: Transport> {
    /// Underlying transport implementation
    transport: Arc<T>,
    /// Device identity
    device_id: DeviceId,
    /// Individual identity
    individual_id: IndividualId,
    /// Device key manager for message signing and verification
    device_key_manager: Arc<RwLock<DeviceKeyManager>>,
    /// Authority graph for capability evaluation
    authority_graph: RwLock<AuthorityGraph>,
    /// Message handlers by content type
    handlers: RwLock<BTreeMap<String, CapabilityMessageHandler>>,
    /// Pending outbound messages
    outbound_queue: RwLock<BTreeMap<Uuid, CapabilityMessage>>,
    /// Active connections by peer ID
    connections: RwLock<BTreeMap<String, Connection>>,
    /// Connected peers mapped to their individual IDs
    connected_peers: RwLock<BTreeMap<String, IndividualId>>,
    /// Message delivery confirmations
    delivery_confirmations: RwLock<BTreeMap<Uuid, BTreeSet<IndividualId>>>,
    /// CGKA state manager for group operations
    cgka_states: RwLock<BTreeMap<String, CgkaState>>,
    /// Injectable effects for deterministic testing
    effects: aura_crypto::Effects,
}

impl<T: Transport> CapabilityTransportAdapter<T> {
    /// Create new capability transport adapter
    pub fn new(
        transport: Arc<T>, 
        device_id: DeviceId, 
        individual_id: IndividualId,
        device_key_manager: DeviceKeyManager,
        effects: aura_crypto::Effects,
    ) -> Self {
        info!("Creating capability transport adapter for device {} (individual: {})", 
              device_id.0, individual_id.0);
        
        Self {
            transport,
            device_id,
            individual_id,
            device_key_manager: Arc::new(RwLock::new(device_key_manager)),
            authority_graph: RwLock::new(AuthorityGraph::new()),
            handlers: RwLock::new(BTreeMap::new()),
            outbound_queue: RwLock::new(BTreeMap::new()),
            connections: RwLock::new(BTreeMap::new()),
            connected_peers: RwLock::new(BTreeMap::new()),
            delivery_confirmations: RwLock::new(BTreeMap::new()),
            cgka_states: RwLock::new(BTreeMap::new()),
            effects,
        }
    }
    
    /// Update authority graph
    pub async fn update_authority_graph(&self, authority_graph: AuthorityGraph) {
        let mut graph = self.authority_graph.write().await;
        *graph = authority_graph;
        debug!("Updated authority graph in transport");
    }
    
    /// Register message handler for specific content type
    pub async fn register_message_handler(
        &self,
        content_type: String,
        handler: CapabilityMessageHandler,
    ) {
        let mut handlers = self.handlers.write().await;
        handlers.insert(content_type.clone(), handler);
        debug!("Registered message handler for content type: {}", content_type);
    }
    
    /// Unregister message handler for content type
    pub async fn unregister_message_handler(&self, content_type: &str) {
        let mut handlers = self.handlers.write().await;
        if handlers.remove(content_type).is_some() {
            debug!("Unregistered message handler for content type: {}", content_type);
        }
    }
    
    /// Invoke custom message handler for content
    async fn invoke_message_handler(&self, message: &CapabilityMessage) -> Result<bool, TransportError> {
        let content_type = self.get_message_content_type(&message.content);
        let handlers = self.handlers.read().await;
        
        if let Some(handler) = handlers.get(&content_type) {
            debug!("Invoking custom handler for content type: {}", content_type);
            handler(message)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    /// Get content type string for message content
    fn get_message_content_type(&self, content: &MessageContent) -> String {
        match content {
            MessageContent::CapabilityDelegation(_) => "capability_delegation".to_string(),
            MessageContent::CapabilityRevocation(_) => "capability_revocation".to_string(),
            MessageContent::CgkaOperation(_) => "cgka_operation".to_string(),
            MessageContent::CgkaStateSync(_) => "cgka_state_sync".to_string(),
            MessageContent::CgkaEpochTransition(_) => "cgka_epoch_transition".to_string(),
            MessageContent::Data { context, .. } => format!("data:{}", context),
            MessageContent::DeliveryConfirmation { .. } => "delivery_confirmation".to_string(),
        }
    }
    
    /// Connect to a peer with capability authentication
    pub async fn connect_peer(
        &self,
        peer_device_id: &DeviceId,
        peer_individual_id: &IndividualId,
        my_ticket: &PresenceTicket,
        peer_ticket: &PresenceTicket,
    ) -> Result<(), TransportError> {
        let peer_id_str = peer_device_id.0.to_string();
        
        let connection = self.transport
            .connect(&peer_id_str, my_ticket, peer_ticket)
            .await
            .map_err(|e| TransportError::NetworkError(format!("Connection failed: {:?}", e)))?;
            
        // Store connection and peer mapping
        {
            let mut connections = self.connections.write().await;
            connections.insert(peer_id_str.clone(), connection);
        }
        
        {
            let mut peers = self.connected_peers.write().await;
            peers.insert(peer_id_str, peer_individual_id.clone());
        }
        
        info!("Connected to peer: {} ({})", peer_device_id.0, peer_individual_id.0);
        Ok(())
    }
    
    /// Disconnect from a peer
    pub async fn disconnect_peer(&self, peer_device_id: &DeviceId) -> Result<(), TransportError> {
        let peer_id_str = peer_device_id.0.to_string();
        
        // Get and remove connection
        let connection = {
            let mut connections = self.connections.write().await;
            connections.remove(&peer_id_str)
        };
        
        if let Some(conn) = connection {
            self.transport
                .disconnect(&conn)
                .await
                .map_err(|e| TransportError::NetworkError(e.to_string()))?;
        }
        
        // Remove peer mapping
        {
            let mut peers = self.connected_peers.write().await;
            peers.remove(&peer_id_str);
        }
        
        info!("Disconnected from peer: {}", peer_device_id.0);
        Ok(())
    }
    
    /// Get connection for a peer device ID
    async fn get_connection(&self, peer_device_id: &DeviceId) -> Option<Connection> {
        let connections = self.connections.read().await;
        connections.get(&peer_device_id.0.to_string()).cloned()
    }
    
    /// Convert IndividualId to peer device ID for transport
    fn individual_to_device_id(&self, individual_id: &IndividualId) -> DeviceId {
        // For now, assume 1:1 mapping between individual and device
        // In practice, you'd need a lookup mechanism
        DeviceId(Uuid::parse_str(&individual_id.0).unwrap_or_else(|_| self.effects.gen_uuid()))
    }
    
    /// Send capability delegation to peers
    pub async fn send_capability_delegation(
        &self,
        delegation: CapabilityDelegation,
        recipients: Option<BTreeSet<IndividualId>>,
    ) -> Result<Uuid, TransportError> {
        info!("Sending capability delegation: {}", delegation.capability_id.as_hex());
        
        // Create capability-authenticated message
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
    ) -> Result<Uuid, TransportError> {
        info!("Sending capability revocation: {}", revocation.capability_id.as_hex());
        
        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: recipients.clone(),
            required_scope: CapabilityScope::simple("capability", "propagate"),
            content: MessageContent::CapabilityRevocation(revocation),
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };
        
        // Sign message with device key
        message.signature = self.sign_message(&message)?;
        
        self.send_message(message).await
    }
    
    /// Send CGKA operation to group members
    pub async fn send_cgka_operation(
        &self,
        operation: KeyhiveCgkaOperation,
        group_id: &str,
    ) -> Result<Uuid, TransportError> {
        info!("Sending CGKA operation {} for group {}", 
              operation.operation_id.0, group_id);
        
        // Get group members from authority graph
        let graph = self.authority_graph.read().await;
        let member_scope = CapabilityScope::with_resource("mls", "member", group_id);
        let authorized_subjects = graph.get_subjects_with_scope(&member_scope, &self.effects);
        
        let recipients: BTreeSet<IndividualId> = authorized_subjects
            .into_iter()
            .map(|subject| IndividualId::new(&subject.0))
            .collect();
        
        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: Some(recipients),
            required_scope: member_scope,
            content: MessageContent::CgkaOperation(operation),
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
    ) -> Result<Uuid, TransportError> {
        debug!("Sending {} bytes with context '{}' and scope {}:{}", 
               data.len(), context, required_scope.namespace, required_scope.operation);
        
        let mut message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients,
            required_scope,
            content: MessageContent::Data { data, context },
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };
        
        // Sign message with device key
        message.signature = self.sign_message(&message)?;
        
        self.send_message(message).await
    }
    
    /// Send message with capability authentication
    async fn send_message(&self, message: CapabilityMessage) -> Result<Uuid, TransportError> {
        // Verify sender has required capability
        let graph = self.authority_graph.read().await;
        let sender_subject = message.sender.to_subject();
        let result = graph.evaluate_capability(&sender_subject, &message.required_scope, &self.effects);
        
        if !matches!(result, CapabilityResult::Granted) {
            return Err(TransportError::InsufficientCapability(format!(
                "Sender {} lacks required capability {}:{}",
                message.sender.0,
                message.required_scope.namespace,
                message.required_scope.operation
            )));
        }
        
        let message_id = message.message_id;
        
        // Serialize message for transport
        let message_bytes = bincode::serialize(&message)
            .map_err(|e| TransportError::SerializationError(e.to_string()))?;
        
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
                let _result = self.transport.broadcast(&connections, &message_bytes).await
                    .map_err(|e| TransportError::NetworkError(e.to_string()))?;
            }
        }
        
        // Add to outbound queue for tracking
        {
            let mut queue = self.outbound_queue.write().await;
            queue.insert(message_id, message.clone());
        }
        
        // Track delivery confirmations if message has specific recipients
        if let Some(recipients) = &message.recipients {
            self.track_delivery_confirmations(message_id, recipients.clone()).await;
        }
        
        debug!("Message {} sent via underlying transport", message_id);
        
        Ok(message_id)
    }
    
    /// Listen for incoming messages on a connection
    pub async fn listen_on_connection(
        &self, 
        connection: &Connection,
        timeout: std::time::Duration
    ) -> Result<Option<CapabilityMessage>, TransportError> {
        let message_bytes = self.transport
            .receive(connection, timeout)
            .await
            .map_err(|e| TransportError::NetworkError(e.to_string()))?;
            
        if let Some(bytes) = message_bytes {
            let message: CapabilityMessage = bincode::deserialize(&bytes)
                .map_err(|e| TransportError::SerializationError(e.to_string()))?;
            Ok(Some(message))
        } else {
            Ok(None)
        }
    }
    
    /// Receive and process incoming message
    pub async fn receive_message(&self, message: CapabilityMessage) -> Result<(), TransportError> {
        debug!("Receiving message {} from {}", message.message_id, message.sender.0);
        
        // Verify message signature
        self.verify_message_signature(&message)?;
        
        // Verify recipient authorization
        if let Some(recipients) = &message.recipients {
            if !recipients.contains(&self.individual_id) {
                return Err(TransportError::NotAuthorized(
                    "Not in recipient list".to_string()
                ));
            }
        }
        
        // Verify sender has required capability
        let graph = self.authority_graph.read().await;
        let sender_subject = message.sender.to_subject();
        let result = graph.evaluate_capability(&sender_subject, &message.required_scope, &self.effects);
        
        if !matches!(result, CapabilityResult::Granted) {
            warn!("Rejecting message from {} - insufficient capability", message.sender.0);
            return Err(TransportError::InsufficientCapability(format!(
                "Sender {} lacks required capability",
                message.sender.0
            )));
        }
        
        // Try custom message handler first
        let handled_by_custom = self.invoke_message_handler(&message).await?;
        
        // If no custom handler, use default processing
        if !handled_by_custom {
            self.process_message_content(&message).await?;
        }
        
        // Send delivery confirmation
        self.send_delivery_confirmation(message.message_id, &message.sender).await?;
        
        Ok(())
    }
    
    /// Process message content based on type
    async fn process_message_content(&self, message: &CapabilityMessage) -> Result<(), TransportError> {
        match &message.content {
            MessageContent::CapabilityDelegation(delegation) => {
                info!("Processing capability delegation: {}", delegation.capability_id.as_hex());
                
                // Apply to local authority graph
                let mut graph = self.authority_graph.write().await;
                graph.apply_delegation(delegation.clone(), &self.effects)
                    .map_err(|e| TransportError::ProcessingError(e.to_string()))?;
                
                debug!("Applied capability delegation to local authority graph");
            }
            
            MessageContent::CapabilityRevocation(revocation) => {
                info!("Processing capability revocation: {}", revocation.capability_id.as_hex());
                
                // Apply to local authority graph
                let mut graph = self.authority_graph.write().await;
                graph.apply_revocation(revocation.clone(), &self.effects)
                    .map_err(|e| TransportError::ProcessingError(e.to_string()))?;
                
                debug!("Applied capability revocation to local authority graph");
            }
            
            MessageContent::CgkaOperation(operation) => {
                info!("Processing CGKA operation: {:?}", operation.operation_id);
                
                // Forward to CGKA manager for processing
                self.process_cgka_operation(operation.clone()).await.map_err(|e| {
                    TransportError::ProcessingError(format!("CGKA operation failed: {:?}", e))
                })?;
                
                debug!("CGKA operation processed");
            }
            
            MessageContent::CgkaStateSync(sync) => {
                info!("Processing CGKA state sync for group {} at epoch {}", 
                      sync.group_id, sync.epoch);
                
                // Forward to CGKA manager for state synchronization
                self.process_cgka_state_sync(sync.clone()).await.map_err(|e| {
                    TransportError::ProcessingError(format!("CGKA state sync failed: {:?}", e))
                })?;
                
                debug!("CGKA state sync processed");
            }
            
            MessageContent::CgkaEpochTransition(transition) => {
                info!("Processing CGKA epoch transition for group {} (epoch {} -> {})", 
                      transition.group_id, transition.previous_epoch, transition.new_epoch);
                
                // Forward to CGKA manager for epoch transition
                self.process_cgka_epoch_transition(transition.clone()).await.map_err(|e| {
                    TransportError::ProcessingError(format!("CGKA epoch transition failed: {:?}", e))
                })?;
                
                debug!("CGKA epoch transition processed");
            }
            
            MessageContent::Data { data, context } => {
                debug!("Processing data message: {} bytes for context '{}'", 
                       data.len(), context);
                
                // TODO: Forward to appropriate handler based on context
                debug!("Data message processed");
            }
            
            MessageContent::DeliveryConfirmation { original_message_id, confirmed_by, .. } => {
                info!("Processing delivery confirmation for message {} from {}", 
                      original_message_id, confirmed_by.0);
                
                // Record the delivery confirmation
                self.record_delivery_confirmation(*original_message_id, confirmed_by.clone()).await;
                
                debug!("Delivery confirmation recorded");
            }
        }
        
        Ok(())
    }
    
    /// Send delivery confirmation
    async fn send_delivery_confirmation(
        &self,
        message_id: Uuid,
        sender: &IndividualId,
    ) -> Result<(), TransportError> {
        debug!("Sending delivery confirmation for message {} to {}", 
               message_id, sender.0);
        
        // Create delivery confirmation message
        let mut confirmation_message = CapabilityMessage {
            message_id: self.effects.gen_uuid(),
            sender: self.individual_id.clone(),
            recipients: Some([sender.clone()].into_iter().collect()),
            required_scope: CapabilityScope::simple("transport", "confirm"),
            content: MessageContent::DeliveryConfirmation {
                original_message_id: message_id,
                confirmed_by: self.individual_id.clone(),
                timestamp: self.effects.now().unwrap_or(0),
            },
            timestamp: self.effects.now().unwrap_or(0),
            signature: Vec::new(),
        };
        
        // Sign the confirmation message
        confirmation_message.signature = self.sign_message(&confirmation_message)?;
        
        // Send confirmation directly to sender
        let device_id = self.individual_to_device_id(sender);
        if let Some(connection) = self.get_connection(&device_id).await {
            let confirmation_bytes = bincode::serialize(&confirmation_message)
                .map_err(|e| TransportError::SerializationError(e.to_string()))?;
                
            self.transport.send(&connection, &confirmation_bytes)
                .await
                .map_err(|e| TransportError::NetworkError(e.to_string()))?;
                
            debug!("Delivery confirmation sent to {}", sender.0);
        } else {
            warn!("Could not send delivery confirmation to {} - no connection", sender.0);
        }
        
        Ok(())
    }
    
    
    /// Remove connected peer
    pub async fn remove_peer(&self, peer: &IndividualId) {
        let device_id = self.individual_to_device_id(peer);
        let peer_id_str = device_id.0.to_string();
        
        let mut peers = self.connected_peers.write().await;
        peers.remove(&peer_id_str);
        info!("Removed peer: {}", peer.0);
    }
    
    /// Get connected peers
    pub async fn get_peers(&self) -> BTreeSet<IndividualId> {
        let peers = self.connected_peers.read().await;
        peers.values().cloned().collect()
    }
    
    /// Track expected delivery confirmations for a message
    pub async fn track_delivery_confirmations(
        &self,
        message_id: Uuid,
        expected_recipients: BTreeSet<IndividualId>,
    ) {
        let mut confirmations = self.delivery_confirmations.write().await;
        confirmations.insert(message_id, BTreeSet::new());
        debug!("Tracking delivery confirmations for message {} from {} recipients", 
               message_id, expected_recipients.len());
    }
    
    /// Record delivery confirmation from a peer
    pub async fn record_delivery_confirmation(
        &self,
        message_id: Uuid,
        confirming_peer: IndividualId,
    ) -> bool {
        let mut confirmations = self.delivery_confirmations.write().await;
        if let Some(confirmed_set) = confirmations.get_mut(&message_id) {
            let newly_confirmed = confirmed_set.insert(confirming_peer.clone());
            if newly_confirmed {
                debug!("Recorded delivery confirmation for message {} from {}", 
                       message_id, confirming_peer.0);
            }
            return newly_confirmed;
        }
        false
    }
    
    /// Check if message has been confirmed by all expected recipients
    pub async fn is_fully_confirmed(&self, message_id: Uuid, expected_recipients: &BTreeSet<IndividualId>) -> bool {
        let confirmations = self.delivery_confirmations.read().await;
        if let Some(confirmed_set) = confirmations.get(&message_id) {
            return expected_recipients.is_subset(confirmed_set);
        }
        false
    }
    
    /// Get delivery confirmation status for a message
    pub async fn get_delivery_status(&self, message_id: Uuid) -> Option<BTreeSet<IndividualId>> {
        let confirmations = self.delivery_confirmations.read().await;
        confirmations.get(&message_id).cloned()
    }
    
    /// Clean up old delivery confirmations
    pub async fn cleanup_delivery_confirmations(&self, max_age_messages: usize) {
        let mut confirmations = self.delivery_confirmations.write().await;
        let current_count = confirmations.len();
        
        if current_count > max_age_messages {
            // Keep only the most recent messages (simplified cleanup)
            // In production, this would use timestamp-based cleanup
            let to_remove = current_count - max_age_messages;
            let keys_to_remove: Vec<Uuid> = confirmations.keys().take(to_remove).cloned().collect();
            
            for key in keys_to_remove {
                confirmations.remove(&key);
            }
            
            debug!("Cleaned up {} old delivery confirmation records", to_remove);
        }
    }
    
    /// Get all active connections
    pub async fn get_connections(&self) -> Vec<Connection> {
        let connections = self.connections.read().await;
        connections.values().cloned().collect()
    }
    
    /// Get pending messages count
    pub async fn pending_messages_count(&self) -> usize {
        self.outbound_queue.read().await.len()
    }
    
    /// Flush outbound queue (for testing)
    pub async fn flush_outbound_queue(&self) {
        let mut queue = self.outbound_queue.write().await;
        let count = queue.len();
        queue.clear();
        debug!("Flushed {} messages from outbound queue", count);
    }
    
    /// Sign a capability message with device key
    ///
    /// Creates a signature over the message content for authentication using
    /// the actual device signing key from the DeviceKeyManager.
    fn sign_message(&self, message: &CapabilityMessage) -> Result<Vec<u8>, TransportError> {
        let rt = tokio::runtime::Handle::current();
        
        // Sign using the actual device key manager
        let signature = rt.block_on(async {
            // Create message hash for signing (excluding signature field)
            let signable_content = bincode::serialize(&(
                &message.message_id,
                &message.sender,
                &message.recipients,
                &message.required_scope,
                &message.content,
                message.timestamp,
            )).map_err(|e| TransportError::SerializationError(e.to_string()))?;
            
            // Use actual device key manager for signing
            let device_key_manager = self.device_key_manager.read().await;
            device_key_manager.sign_message(&signable_content)
                .map_err(|e| TransportError::InvalidMessage(format!("Device key signing failed: {:?}", e)))
        })?;
        
        Ok(signature)
    }
    
    /// Process CGKA operation by applying it to the appropriate group state
    async fn process_cgka_operation(&self, operation: KeyhiveCgkaOperation) -> Result<(), aura_cgka::CgkaError> {
        let group_id = operation.group_id.clone();
        let mut states = self.cgka_states.write().await;
        
        // Get or create group state with proper error handling
        let group_state = match states.entry(group_id.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                // Create new group state for first operation
                debug!("Creating new CGKA state for group {}", group_id);
                
                match CgkaState::new(group_id.clone(), Vec::new(), &self.effects) {
                    Ok(state) => entry.insert(state),
                    Err(e) => {
                        warn!("Failed to create CGKA state for group {}: {:?}", group_id, e);
                        return Err(aura_cgka::CgkaError::InvalidOperation(
                            format!("Failed to initialize group state: {:?}", e)
                        ));
                    }
                }
            }
        };
        
        // Apply the operation to the group state
        group_state.apply_operation(operation, &self.effects)?;
        
        Ok(())
    }
    
    /// Process CGKA state synchronization event
    async fn process_cgka_state_sync(&self, _sync: CgkaStateSyncEvent) -> Result<(), aura_cgka::CgkaError> {
        // For MVP, just acknowledge the sync event
        // In production, this would synchronize state with peer and resolve conflicts
        debug!("CGKA state sync acknowledged");
        Ok(())
    }
    
    /// Process CGKA epoch transition event  
    async fn process_cgka_epoch_transition(&self, _transition: CgkaEpochTransitionEvent) -> Result<(), aura_cgka::CgkaError> {
        // For MVP, just acknowledge the epoch transition
        // In production, this would update group state to new epoch
        debug!("CGKA epoch transition acknowledged");
        Ok(())
    }
    
    /// Verify a capability message signature
    ///
    /// Verifies the signature against the message content using the sender's device key.
    /// Looks up the sender's public key from the device key manager's known keys.
    fn verify_message_signature(&self, message: &CapabilityMessage) -> Result<(), TransportError> {
        let rt = tokio::runtime::Handle::current();
        
        rt.block_on(async {
            // Recreate the signable content (same as in sign_message)
            let signable_content = bincode::serialize(&(
                &message.message_id,
                &message.sender,
                &message.recipients,
                &message.required_scope,
                &message.content,
                message.timestamp,
            )).map_err(|e| TransportError::SerializationError(e.to_string()))?;
            
            // Parse signature
            if message.signature.len() != 64 {
                return Err(TransportError::InvalidMessage(
                    "Invalid signature length".to_string()
                ));
            }
            
            // Convert sender to device ID for key lookup
            let sender_device_id = self.individual_to_device_id(&message.sender);
            
            // Verify using device key manager
            let device_key_manager = self.device_key_manager.read().await;
            let is_valid = device_key_manager.verify_message_signature(
                sender_device_id.0,
                &signable_content,
                &message.signature,
            ).map_err(|e| TransportError::InvalidMessage(format!("Signature verification error: {:?}", e)))?;
            
            if !is_valid {
                return Err(TransportError::InvalidMessage("Signature verification failed".to_string()));
            }
            
            Ok(())
        })
    }
}

/// Message handler for specific content types
pub type CapabilityMessageHandler = Box<dyn Fn(&CapabilityMessage) -> Result<(), TransportError> + Send + Sync>;

/// Transport layer errors
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Insufficient capability: {0}")]
    InsufficientCapability(String),
    
    #[error("Not authorized: {0}")]
    NotAuthorized(String),
    
    #[error("Processing error: {0}")]
    ProcessingError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Invalid message: {0}")]
    InvalidMessage(String),
}

/// CGKA operation delivery manager
pub struct CgkaOperationDelivery<T: Transport> {
    /// Transport layer
    transport: Arc<CapabilityTransportAdapter<T>>,
    /// Operation confirmations by group
    confirmations: RwLock<BTreeMap<String, BTreeMap<OperationId, BTreeSet<IndividualId>>>>,
    /// Required confirmation threshold by group
    confirmation_thresholds: RwLock<BTreeMap<String, usize>>,
}

impl<T: Transport> CgkaOperationDelivery<T> {
    /// Create new CGKA operation delivery manager
    pub fn new(transport: Arc<CapabilityTransportAdapter<T>>) -> Self {
        Self {
            transport,
            confirmations: RwLock::new(BTreeMap::new()),
            confirmation_thresholds: RwLock::new(BTreeMap::new()),
        }
    }
    
    /// Deliver CGKA operation to group members
    pub async fn deliver_operation(
        &self,
        group_id: &str,
        operation: KeyhiveCgkaOperation,
    ) -> Result<(), TransportError> {
        info!("Delivering CGKA operation {:?} to group {}", 
              operation.operation_id, group_id);
        
        // Send to group members
        let message_id = self.transport.send_cgka_operation(operation.clone(), group_id).await?;
        
        // Initialize confirmation tracking
        let mut confirmations = self.confirmations.write().await;
        let group_confirmations = confirmations.entry(group_id.to_string()).or_insert_with(BTreeMap::new);
        group_confirmations.insert(operation.operation_id, BTreeSet::new());
        
        debug!("CGKA operation delivery initiated with message ID: {}", message_id);
        
        Ok(())
    }
    
    /// Check if operation has sufficient confirmations
    pub async fn has_sufficient_confirmations(
        &self,
        group_id: &str,
        operation_id: &OperationId,
    ) -> bool {
        let confirmations = self.confirmations.read().await;
        let thresholds = self.confirmation_thresholds.read().await;
        
        if let (Some(group_confirmations), Some(&threshold)) = 
            (confirmations.get(group_id), thresholds.get(group_id)) {
            if let Some(op_confirmations) = group_confirmations.get(operation_id) {
                return op_confirmations.len() >= threshold;
            }
        }
        
        false
    }
    
    /// Set confirmation threshold for group
    pub async fn set_confirmation_threshold(&self, group_id: &str, threshold: usize) {
        let mut thresholds = self.confirmation_thresholds.write().await;
        thresholds.insert(group_id.to_string(), threshold);
        debug!("Set confirmation threshold for group '{}' to {}", group_id, threshold);
    }
}

// Type alias for convenience
pub type CapabilityTransport = CapabilityTransportAdapter<crate::StubTransport>;
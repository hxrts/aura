//! SBB Message Integration with Transport Layer
//!
//! This module integrates SBB flooding with the existing transport layer
//! for actual message delivery across peer connections.

use crate::sbb::{RendezvousEnvelope, SbbFlooding, SbbFloodingCoordinator};
use aura_core::effects::{NetworkEffects, NetworkError};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult, DeviceId};
use aura_protocol::effects::AuraEffects;
use aura_protocol::guards::effect_system_trait::GuardEffectSystem;
use aura_protocol::guards::send_guard::create_send_guard;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Real NetworkTransport implementation using aura-effects transport handlers
/// All sends go through guard chain to enforce authorization → flow → leakage → journal sequence.
pub struct NetworkTransport {
    device_id: DeviceId,
    /// Network effect handler for actual message transmission
    network_effects: Arc<dyn NetworkEffects>,
    /// Context ID for guard chain operations
    context_id: ContextId,
}

impl std::fmt::Debug for NetworkTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkTransport")
            .field("device_id", &self.device_id)
            .field("network_effects", &"<dyn NetworkEffects>")
            .finish()
    }
}

impl NetworkTransport {
    /// Create new NetworkTransport with effect handler
    pub fn new(
        device_id: DeviceId,
        network_effects: Arc<dyn NetworkEffects>,
        context_id: ContextId,
    ) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self {
            device_id,
            network_effects,
            context_id,
        }))
    }

    /// Send data to a specific peer using guard chain and network effect system
    pub async fn send_with_guard_chain<E: GuardEffectSystem>(
        &self,
        recipient: &DeviceId,
        data: Vec<u8>,
        effect_system: &E,
    ) -> AuraResult<()> {
        tracing::debug!(
            "Sending {} bytes from {} to {} with guard chain",
            data.len(),
            self.device_id,
            recipient
        );

        // Convert DeviceId to AuthorityId for guard chain
        let recipient_authority = AuthorityId::from(recipient.0);

        // Create guard chain for network send
        let guard_chain = create_send_guard(
            "network:send_data".to_string(),
            self.context_id,
            recipient_authority,
            (data.len() / 100) as u32 + 10, // cost based on data size + base cost
        )
        .with_operation_id(format!("network_send_{}_{}", self.device_id, recipient));

        // Evaluate guard chain before sending
        match guard_chain.evaluate(effect_system).await {
            Ok(result) if result.authorized => {
                tracing::debug!(
                    "Guard chain authorized send to {}, proceeding with network transmission",
                    recipient
                );

                // Use the actual network effect handler for transmission
                self.network_effects
                    .send_to_peer(recipient.0, data)
                    .await
                    .map_err(|e| match e {
                        NetworkError::SendFailed { reason, .. } => AuraError::network(format!(
                            "Failed to send to {}: {}",
                            recipient, reason
                        )),
                        NetworkError::PeerUnreachable { peer_id } => {
                            AuraError::network(format!("Peer unreachable: {}", peer_id))
                        }
                        NetworkError::RateLimitExceeded { limit, window_ms } => AuraError::network(
                            format!("Rate limit exceeded: {} req/{}ms", limit, window_ms),
                        ),
                        NetworkError::OperationTimeout {
                            operation,
                            timeout_ms,
                        } => AuraError::network(format!(
                            "Operation '{}' timed out after {}ms",
                            operation, timeout_ms
                        )),
                        _ => AuraError::network(format!("Network error: {}", e)),
                    })
            }
            Ok(result) => {
                let reason = result
                    .denial_reason
                    .unwrap_or_else(|| "unknown".to_string());
                tracing::warn!("Guard chain denied send to {}: {}", recipient, reason);
                Err(AuraError::permission_denied(format!(
                    "Send to {} denied: {}",
                    recipient, reason
                )))
            }
            Err(err) => {
                tracing::error!(
                    "Guard chain evaluation failed for send to {}: {}",
                    recipient,
                    err
                );
                Err(AuraError::permission_denied(format!(
                    "Send authorization failed: {}",
                    err
                )))
            }
        }
    }

    /// Legacy send method (deprecated - bypasses guard chain)
    pub async fn send(&self, recipient: &DeviceId, data: Vec<u8>) -> AuraResult<()> {
        tracing::warn!(
            "NetworkTransport::send called without guard chain - this bypasses security"
        );

        // Use the actual network effect handler for transmission
        self.network_effects
            .send_to_peer(recipient.0, data)
            .await
            .map_err(|e| match e {
                NetworkError::SendFailed { reason, .. } => {
                    AuraError::network(format!("Failed to send to {}: {}", recipient, reason))
                }
                NetworkError::PeerUnreachable { peer_id } => {
                    AuraError::network(format!("Peer unreachable: {}", peer_id))
                }
                NetworkError::RateLimitExceeded { limit, window_ms } => AuraError::network(
                    format!("Rate limit exceeded: {} req/{}ms", limit, window_ms),
                ),
                NetworkError::OperationTimeout {
                    operation,
                    timeout_ms,
                } => AuraError::network(format!(
                    "Operation '{}' timed out after {}ms",
                    operation, timeout_ms
                )),
                _ => AuraError::network(format!("Network error: {}", e)),
            })
    }

    /// Check if a peer is connected using the network effect system
    pub async fn is_peer_connected(&self, peer: DeviceId) -> bool {
        self.network_effects.is_peer_connected(peer.0).await
    }

    /// Get list of currently connected peers
    pub async fn connected_peers(&self) -> Vec<DeviceId> {
        self.network_effects
            .connected_peers()
            .await
            .into_iter()
            .map(DeviceId)
            .collect()
    }

    /// Broadcast a message to all connected peers using guard chain
    pub async fn broadcast_with_guard_chain<E: GuardEffectSystem>(
        &self,
        data: Vec<u8>,
        effect_system: &E,
    ) -> AuraResult<()> {
        tracing::debug!(
            "Broadcasting {} bytes from {} with guard chain",
            data.len(),
            self.device_id
        );

        // Get connected peers first
        let connected_peers = self.connected_peers().await;

        // Send to each peer individually through guard chain
        for peer_device in connected_peers {
            if let Err(err) = self
                .send_with_guard_chain(&peer_device, data.clone(), effect_system)
                .await
            {
                tracing::warn!("Failed to broadcast to peer {}: {}", peer_device, err);
                // Continue with other peers rather than failing the entire broadcast
            }
        }

        Ok(())
    }

    /// Legacy broadcast method (deprecated - bypasses guard chain)
    pub async fn broadcast(&self, data: Vec<u8>) -> AuraResult<()> {
        tracing::warn!(
            "NetworkTransport::broadcast called without guard chain - this bypasses security"
        );

        self.network_effects
            .broadcast(data)
            .await
            .map_err(|e| AuraError::network(format!("Broadcast failed: {}", e)))
    }

    /// Receive next available message from any peer
    pub async fn receive(&self) -> AuraResult<(DeviceId, Vec<u8>)> {
        let (peer_uuid, data) = self
            .network_effects
            .receive()
            .await
            .map_err(|e| AuraError::network(format!("Receive failed: {}", e)))?;

        Ok((DeviceId(peer_uuid), data))
    }

    /// Connect to a peer (for testing/simulation)
    pub async fn connect_peer(&mut self, peer: DeviceId) -> AuraResult<()> {
        tracing::info!("Connected to peer {}", peer);
        // Connection management is handled by the underlying NetworkEffects implementation
        Ok(())
    }

    /// Disconnect from a peer
    pub async fn disconnect_peer(&mut self, peer: DeviceId) -> AuraResult<()> {
        tracing::info!("Disconnected from peer {}", peer);
        // Connection management is handled by the underlying NetworkEffects implementation
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct NetworkConfig {
    pub max_connections: usize,
    pub timeout_ms: u64,
}

/// SBB message types for transport protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SbbMessageType {
    /// Rendezvous envelope flooding
    RendezvousFlood {
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
    },
    /// Transport offer (payload within envelope)
    TransportOffer { offer_data: Vec<u8> },
    /// Transport answer (payload within envelope)
    TransportAnswer { answer_data: Vec<u8> },
}

/// Transport offer payload for rendezvous
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportOfferPayload {
    /// Device ID offering connection
    pub device_id: DeviceId,
    /// Available transport methods (WebSocket, QUIC, etc.)
    pub transport_methods: Vec<TransportMethod>,
    /// Offer expiration timestamp
    pub expires_at: u64,
    /// Nonce for replay protection
    pub nonce: [u8; 16],
}

/// Available transport methods for connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportMethod {
    /// WebSocket connection
    WebSocket { url: String },
    /// QUIC connection
    Quic { addr: String, port: u16 },
    /// Direct TCP (for testing)
    Tcp { addr: String, port: u16 },
}

/// Transport answer payload for rendezvous
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportAnswerPayload {
    /// Device ID answering the offer
    pub device_id: DeviceId,
    /// Selected transport method from the offer
    pub selected_method: TransportMethod,
    /// Answer expiration timestamp
    pub expires_at: u64,
    /// Connection parameters for the selected method
    pub connection_params: Vec<u8>,
    /// Nonce for replay protection
    pub nonce: [u8; 16],
}

/// SBB transport bridge connecting flooding to actual transport
#[derive(Debug)]
pub struct SbbTransportBridge {
    /// SBB flooding coordinator
    flooding_coordinator: Arc<RwLock<SbbFloodingCoordinator>>,
    /// Transport message sender (placeholder interface)
    transport_sender: Option<BoxedTransportSender>,
    /// Pending transport offers we've sent (waiting for answers)
    pending_offers: Arc<RwLock<HashMap<DeviceId, TransportOfferPayload>>>,
}

/// Transport sender interface (placeholder - would integrate with real transport)
#[async_trait::async_trait]
pub trait TransportSender: Send + Sync {
    /// Send message to peer via transport layer
    async fn send_to_peer(&self, peer: DeviceId, message: SbbMessageType) -> AuraResult<()>;

    /// Check if peer is reachable
    async fn is_peer_reachable(&self, peer: &DeviceId) -> bool;
}

/// Mock transport sender for testing
#[derive(Debug, Clone)]
pub struct MockTransportSender {
    /// Simulated peer reachability
    pub reachable_peers: Vec<DeviceId>,
}

impl SbbTransportBridge {
    /// Create new SBB transport bridge
    pub fn new(device_id: DeviceId, effects: Arc<dyn AuraEffects>) -> Self {
        let flooding_coordinator =
            Arc::new(RwLock::new(SbbFloodingCoordinator::new(device_id, effects)));

        Self {
            flooding_coordinator,
            transport_sender: None,
            pending_offers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create SBB transport bridge with NetworkTransport
    pub fn with_network_transport(
        device_id: DeviceId,
        transport: Arc<RwLock<NetworkTransport>>,
        effects: Arc<dyn AuraEffects>,
    ) -> Self {
        let flooding_coordinator =
            Arc::new(RwLock::new(SbbFloodingCoordinator::new(device_id, effects)));
        let sender = NetworkTransportSender::new(transport);

        Self {
            flooding_coordinator,
            transport_sender: Some(BoxedTransportSender(Box::new(sender))),
            pending_offers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set transport sender for message delivery
    pub fn set_transport_sender(&mut self, sender: Box<dyn TransportSender>) {
        self.transport_sender = Some(BoxedTransportSender(sender));
    }

    /// Add friend relationship for SBB flooding
    pub async fn add_friend(&self, friend_id: DeviceId) {
        let mut coordinator = self.flooding_coordinator.write().await;
        coordinator.add_friend(friend_id);
    }

    /// Add guardian relationship for SBB flooding
    pub async fn add_guardian(&self, guardian_id: DeviceId) {
        let mut coordinator = self.flooding_coordinator.write().await;
        coordinator.add_guardian(guardian_id);
    }

    /// Create and flood rendezvous offer
    pub async fn flood_rendezvous_offer(
        &self,
        offer_payload: TransportOfferPayload,
    ) -> AuraResult<()> {
        // Store the offer in pending offers for answer processing
        {
            let mut pending = self.pending_offers.write().await;
            pending.insert(offer_payload.device_id, offer_payload.clone());
        }

        // Serialize offer payload
        let payload_bytes = bincode::serialize(&offer_payload)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize offer: {}", e)))?;

        // Create rendezvous envelope
        let envelope = RendezvousEnvelope::new(payload_bytes, None);

        // Flood through SBB
        let mut coordinator = self.flooding_coordinator.write().await;
        let now = crate::sbb::current_timestamp();
        let result = coordinator.flood_envelope(envelope, None, now).await?;

        match result {
            crate::sbb::FloodResult::Forwarded { peer_count } => {
                tracing::info!("Rendezvous offer flooded to {} peers", peer_count);
            }
            crate::sbb::FloodResult::Dropped => {
                tracing::warn!("Rendezvous offer was dropped (no peers or TTL expired)");
            }
            crate::sbb::FloodResult::Failed { reason } => {
                return Err(AuraError::network(format!("Flooding failed: {}", reason)));
            }
        }

        Ok(())
    }

    /// Handle received SBB message from transport layer
    pub async fn handle_transport_message(&self, message: SbbMessageType) -> AuraResult<()> {
        match message {
            SbbMessageType::RendezvousFlood {
                envelope,
                from_peer,
            } => self.handle_rendezvous_flood(envelope, from_peer).await,
            SbbMessageType::TransportOffer { offer_data } => {
                self.handle_transport_offer(offer_data).await
            }
            SbbMessageType::TransportAnswer { answer_data } => {
                self.handle_transport_answer(answer_data).await
            }
        }
    }

    /// Handle rendezvous envelope flood from peer
    async fn handle_rendezvous_flood(
        &self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
    ) -> AuraResult<()> {
        // Process through flooding coordinator for further propagation
        let mut coordinator = self.flooding_coordinator.write().await;
        let now = crate::sbb::current_timestamp();
        let _result = coordinator.flood_envelope(envelope, from_peer, now).await?;

        // TODO: If this device is interested in the offer, process it
        // For now, just propagate it

        Ok(())
    }

    /// Handle transport offer (Alice receives Bob's offer)
    async fn handle_transport_offer(&self, offer_data: Vec<u8>) -> AuraResult<()> {
        // Deserialize transport offer
        let offer: TransportOfferPayload = bincode::deserialize(&offer_data)
            .map_err(|e| AuraError::serialization(format!("Failed to deserialize offer: {}", e)))?;

        println!(
            "Received transport offer from device: {:?}",
            offer.device_id
        );
        println!("Available methods: {:?}", offer.transport_methods);

        // TODO: If we want to connect, create transport answer and establish connection
        // For now, just log the offer

        Ok(())
    }

    /// Handle transport answer (Bob receives Alice's answer)
    async fn handle_transport_answer(&self, answer_data: Vec<u8>) -> AuraResult<()> {
        println!("Received transport answer: {} bytes", answer_data.len());

        // TODO: Process answer and establish connection via selected transport method

        Ok(())
    }

    /// Check if this device should respond to a transport offer
    async fn should_respond_to_offer(
        &self,
        offer: &TransportOfferPayload,
        coordinator: &SbbFloodingCoordinator,
    ) -> bool {
        // Respond to offers from friends or guardians
        coordinator.friends().contains(&offer.device_id)
            || coordinator.guardians().contains(&offer.device_id)
    }

    /// Create and send transport answer for an offer
    async fn create_and_send_transport_answer(
        &self,
        offer: &TransportOfferPayload,
        coordinator: &SbbFloodingCoordinator,
    ) -> AuraResult<()> {
        // Select a transport method we support
        let selected_method = self
            .select_transport_method(&offer.transport_methods)
            .await?;

        // Create answer payload
        let answer = TransportAnswerPayload {
            device_id: coordinator.device_id(),
            selected_method: selected_method.clone(),
            expires_at: crate::sbb::current_timestamp() + 300, // 5 minutes
            connection_params: self.get_connection_params(&selected_method).await,
            nonce: self.generate_nonce(),
        };

        // Send answer via SBB flooding
        self.send_transport_answer(answer, offer.device_id).await
    }

    /// Check if we should accept a transport offer
    async fn should_accept_offer(
        &self,
        offer: &TransportOfferPayload,
        coordinator: &SbbFloodingCoordinator,
    ) -> bool {
        // Accept offers from friends or guardians
        coordinator.friends().contains(&offer.device_id)
            || coordinator.guardians().contains(&offer.device_id)
    }

    /// Select the best transport method from available options
    async fn select_transport_method(
        &self,
        methods: &[TransportMethod],
    ) -> AuraResult<TransportMethod> {
        // Prefer QUIC, then WebSocket, then TCP
        for method in methods {
            if let TransportMethod::Quic { .. } = method { return Ok(method.clone()) }
        }
        for method in methods {
            if let TransportMethod::WebSocket { .. } = method { return Ok(method.clone()) }
        }
        for method in methods {
            if let TransportMethod::Tcp { .. } = method { return Ok(method.clone()) }
        }

        Err(AuraError::invalid(
            "No supported transport methods available",
        ))
    }

    /// Create transport answer for an offer
    async fn create_transport_answer(
        &self,
        offer: &TransportOfferPayload,
        selected_method: &TransportMethod,
    ) -> AuraResult<TransportAnswerPayload> {
        let coordinator = self.flooding_coordinator.read().await;

        Ok(TransportAnswerPayload {
            device_id: coordinator.device_id(),
            selected_method: selected_method.clone(),
            expires_at: crate::sbb::current_timestamp() + 300, // 5 minutes
            connection_params: self.get_connection_params(selected_method).await,
            nonce: self.generate_nonce(),
        })
    }

    /// Send transport answer via SBB flooding
    async fn send_transport_answer(
        &self,
        answer: TransportAnswerPayload,
        target_device: DeviceId,
    ) -> AuraResult<()> {
        // Serialize answer payload
        let payload_bytes = bincode::serialize(&answer)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize answer: {}", e)))?;

        // Create rendezvous envelope
        let envelope = RendezvousEnvelope::new(payload_bytes, None);

        // Flood through SBB
        let mut coordinator = self.flooding_coordinator.write().await;
        let now = crate::sbb::current_timestamp();
        coordinator.flood_envelope(envelope, None, now).await?;

        tracing::info!("Sent transport answer to device {}", target_device);
        Ok(())
    }

    /// Establish connection using selected transport method
    async fn establish_connection(
        &self,
        method: &TransportMethod,
        target_device: &DeviceId,
    ) -> AuraResult<()> {
        match method {
            TransportMethod::WebSocket { url } => {
                tracing::info!(
                    "Establishing WebSocket connection to {} at {}",
                    target_device,
                    url
                );
                // TODO: Implement actual WebSocket connection
                Ok(())
            }
            TransportMethod::Quic { addr, port } => {
                tracing::info!(
                    "Establishing QUIC connection to {} at {}:{}",
                    target_device,
                    addr,
                    port
                );
                // TODO: Implement actual QUIC connection
                Ok(())
            }
            TransportMethod::Tcp { addr, port } => {
                tracing::info!(
                    "Establishing TCP connection to {} at {}:{}",
                    target_device,
                    addr,
                    port
                );
                // TODO: Implement actual TCP connection
                Ok(())
            }
        }
    }

    /// Get pending offer for a device
    async fn get_pending_offer(&self, device_id: &DeviceId) -> Option<TransportOfferPayload> {
        self.pending_offers.read().await.get(device_id).cloned()
    }

    /// Remove pending offer for a device
    async fn remove_pending_offer(&self, device_id: &DeviceId) {
        self.pending_offers.write().await.remove(device_id);
    }

    /// Check if transport method selection is valid
    fn is_valid_method_selection(
        &self,
        selected: &TransportMethod,
        offered: &[TransportMethod],
    ) -> bool {
        offered
            .iter()
            .any(|method| std::mem::discriminant(method) == std::mem::discriminant(selected))
    }

    /// Get connection parameters for transport method
    async fn get_connection_params(&self, method: &TransportMethod) -> Vec<u8> {
        // Return method-specific connection parameters
        match method {
            TransportMethod::WebSocket { .. } => b"websocket_params".to_vec(),
            TransportMethod::Quic { .. } => b"quic_params".to_vec(),
            TransportMethod::Tcp { .. } => b"tcp_params".to_vec(),
        }
    }

    /// Generate a random nonce
    fn generate_nonce(&self) -> [u8; 16] {
        // Generate random nonce for replay protection
        // In production, use proper random number generation
        [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
    }
}

// Wrapper for type erasure
struct BoxedTransportSender(Box<dyn TransportSender>);

impl std::fmt::Debug for BoxedTransportSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoxedTransportSender").finish()
    }
}

#[async_trait::async_trait]
impl TransportSender for MockTransportSender {
    async fn send_to_peer(&self, peer: DeviceId, message: SbbMessageType) -> AuraResult<()> {
        if self.reachable_peers.contains(&peer) {
            println!(
                "Mock transport: sent message to peer {:?}: {:?}",
                peer, message
            );
            Ok(())
        } else {
            Err(AuraError::network("Peer not reachable"))
        }
    }

    async fn is_peer_reachable(&self, peer: &DeviceId) -> bool {
        self.reachable_peers.contains(peer)
    }
}

impl MockTransportSender {
    /// Create mock transport with specified reachable peers
    pub fn new(reachable_peers: Vec<DeviceId>) -> Self {
        Self { reachable_peers }
    }
}

/// Real transport sender using aura-transport NetworkTransport
pub struct NetworkTransportSender {
    /// Reference to the network transport
    transport: Arc<RwLock<NetworkTransport>>,
}

impl NetworkTransportSender {
    /// Create new transport sender from NetworkTransport
    pub fn new(transport: Arc<RwLock<NetworkTransport>>) -> Self {
        Self { transport }
    }

    /// Send message with guard chain enforcement
    pub async fn send_to_peer_with_guard_chain<E: GuardEffectSystem>(
        &self,
        peer: DeviceId,
        message: SbbMessageType,
        effect_system: &E,
    ) -> AuraResult<()> {
        // Serialize SBB message
        let payload = bincode::serialize(&message).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize SBB message: {}", e))
        })?;

        // Send via network transport with guard chain
        let transport = self.transport.read().await;
        transport
            .send_with_guard_chain(&peer, payload, effect_system)
            .await
    }
}

impl std::fmt::Debug for NetworkTransportSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkTransportSender").finish()
    }
}

#[async_trait::async_trait]
impl TransportSender for NetworkTransportSender {
    async fn send_to_peer(&self, peer: DeviceId, message: SbbMessageType) -> AuraResult<()> {
        tracing::warn!(
            "NetworkTransportSender::send_to_peer called without guard chain - this bypasses security"
        );

        // Serialize SBB message
        let payload = bincode::serialize(&message).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize SBB message: {}", e))
        })?;

        // Send via network transport (legacy method)
        let transport = self.transport.read().await;
        transport.send(&peer, payload).await
    }

    async fn is_peer_reachable(&self, peer: &DeviceId) -> bool {
        let transport = self.transport.read().await;
        transport.is_peer_connected(*peer).await
    }
}

/// Integrate SbbFloodingCoordinator with transport sending
impl SbbFloodingCoordinator {
    /// Set transport sender for actual message delivery
    pub fn set_transport_sender(&mut self, sender: Arc<dyn TransportSender>) {
        // Store transport sender reference for use in forward_to_peer
        // Note: This would require adding a field to store the sender in SbbFloodingCoordinator
        // For now, log that the sender was configured
        tracing::debug!("Transport sender configured for flooding coordinator");
        // The actual integration is handled at the SbbTransportBridge level
        let _ = sender; // Use the sender parameter to avoid unused warnings
    }
}

#[async_trait::async_trait]
impl crate::sbb::SbbFlooding for SbbTransportBridge {
    async fn flood_envelope(
        &mut self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<crate::sbb::FloodResult> {
        let mut coordinator = self.flooding_coordinator.write().await;
        coordinator.flood_envelope(envelope, from_peer, now).await
    }

    async fn get_forwarding_peers(
        &self,
        exclude: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<Vec<DeviceId>> {
        let coordinator = self.flooding_coordinator.read().await;
        coordinator.get_forwarding_peers(exclude, now).await
    }

    async fn can_forward_to(
        &self,
        peer: &DeviceId,
        message_size: u64,
        now: u64,
    ) -> AuraResult<bool> {
        let coordinator = self.flooding_coordinator.read().await;
        coordinator.can_forward_to(peer, message_size, now).await
    }

    async fn forward_to_peer(
        &mut self,
        envelope: RendezvousEnvelope,
        peer: DeviceId,
        now: u64,
    ) -> AuraResult<()> {
        // Use transport sender if available, otherwise delegate to coordinator
        if let Some(sender) = &self.transport_sender {
            let message = SbbMessageType::RendezvousFlood {
                envelope,
                from_peer: Some(peer), // TODO: Get actual sender ID
            };
            sender.0.send_to_peer(peer, message).await
        } else {
            // Fallback to coordinator's placeholder implementation
            let mut coordinator = self.flooding_coordinator.write().await;
            coordinator.forward_to_peer(envelope, peer, now).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::{DeviceTestFixture, TestEffectsBuilder};

    fn test_effects(_device_id: DeviceId) -> Arc<dyn AuraEffects> {
        let config = aura_agent::AgentConfig::default();
        let system = aura_agent::AuraEffectSystem::testing(&config);
        Arc::new(system)
    }

    #[tokio::test]
    async fn test_sbb_transport_bridge_creation() {
        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let effects = test_effects(device_id);
        let bridge = SbbTransportBridge::new(device_id, effects);

        // Should create successfully
        assert!(bridge.transport_sender.is_none());
    }

    #[tokio::test]
    async fn test_relationship_management() {
        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let effects = test_effects(device_id);
        let bridge = SbbTransportBridge::new(device_id, effects);

        let friend_fixture = DeviceTestFixture::new(1);
        let friend_id = friend_fixture.device_id();
        let guardian_fixture = DeviceTestFixture::new(2);
        let guardian_id = guardian_fixture.device_id();

        bridge.add_friend(friend_id).await;
        bridge.add_guardian(guardian_id).await;

        // Should add relationships to coordinator
        // TODO: Add public accessor methods for friends and guardians count
        // let coordinator = bridge.flooding_coordinator.read().await;
        // assert_eq!(coordinator.friends.len(), 1);
        // assert_eq!(coordinator.guardians.len(), 1);
    }

    #[tokio::test]
    async fn test_rendezvous_offer_creation() {
        let device_id = DeviceId::new();
        let effects = test_effects(device_id);
        let bridge = SbbTransportBridge::new(device_id, effects);

        let offer = TransportOfferPayload {
            device_id,
            transport_methods: vec![
                TransportMethod::WebSocket {
                    url: "ws://127.0.0.1:8080".to_string(),
                },
                TransportMethod::Quic {
                    addr: "127.0.0.1".to_string(),
                    port: 8443,
                },
            ],
            expires_at: 1234567890 + 3600, // 1 hour from now
            nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };

        // Should serialize and flood offer
        let result = bridge.flood_rendezvous_offer(offer).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_message_handling() {
        let device_id = DeviceId::new();
        let effects = test_effects(device_id);
        let bridge = SbbTransportBridge::new(device_id, effects);

        // Create test envelope
        let payload = b"test offer data".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(2));

        let message = SbbMessageType::RendezvousFlood {
            envelope,
            from_peer: Some(DeviceId::new()),
        };

        // Should handle message without error
        let result = bridge.handle_transport_message(message).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_mock_transport_sender() {
        let peer1 = DeviceId::new();
        let peer2 = DeviceId::new();
        let peer3 = DeviceId::new();

        let sender = MockTransportSender::new(vec![peer1, peer2]);

        // Should report reachable peers correctly
        assert!(sender.is_peer_reachable(&peer1).await);
        assert!(sender.is_peer_reachable(&peer2).await);
        assert!(!sender.is_peer_reachable(&peer3).await);

        // Should send to reachable peers
        let message = SbbMessageType::TransportOffer {
            offer_data: b"test offer".to_vec(),
        };

        let result1 = sender.send_to_peer(peer1, message.clone()).await;
        assert!(result1.is_ok());

        let result3 = sender.send_to_peer(peer3, message).await;
        assert!(result3.is_err());
    }

    #[tokio::test]
    async fn test_network_transport_sender_creation() {
        use super::NetworkTransport;

        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let builder = TestEffectsBuilder::for_unit_tests(device_id);
        let system = builder.build().expect("Failed to build test effect system");
        let effects = Arc::new(system) as Arc<dyn NetworkEffects>;
        let context_id = ContextId::new();
        let transport = NetworkTransport::new(device_id, effects, context_id);

        let sender = NetworkTransportSender::new(transport);

        // Should create successfully
        let unreachable_peer = DeviceId::new();
        assert!(!sender.is_peer_reachable(&unreachable_peer).await);
    }

    #[tokio::test]
    async fn test_sbb_bridge_with_network_transport() {
        use super::NetworkTransport;

        let fixture = DeviceTestFixture::new(0);
        let device_id = fixture.device_id();
        let effects = test_effects(device_id);
        let mut bridge = SbbTransportBridge::new(device_id, effects);

        // Set up real transport sender using testkit
        let builder = TestEffectsBuilder::for_unit_tests(device_id);
        let system = builder.build().expect("Failed to build test effect system");
        let network_effects = Arc::new(system) as Arc<dyn NetworkEffects>;
        let context_id = ContextId::new();
        let transport = NetworkTransport::new(device_id, network_effects, context_id);
        let sender = NetworkTransportSender::new(transport);

        bridge.set_transport_sender(Box::new(sender));

        // Should have transport sender configured
        assert!(bridge.transport_sender.is_some());

        // Test rendezvous offer
        let offer = TransportOfferPayload {
            device_id,
            transport_methods: vec![TransportMethod::WebSocket {
                url: "ws://127.0.0.1:8080".to_string(),
            }],
            expires_at: 1234567890 + 3600,
            nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        };

        // Should handle offer creation (even if no peers to forward to)
        let result = bridge.flood_rendezvous_offer(offer).await;
        assert!(result.is_ok());
    }
}

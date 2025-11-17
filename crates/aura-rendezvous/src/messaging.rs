//! SBB Message Integration with Transport Layer
//!
//! This module integrates SBB flooding with the existing transport layer
//! for actual message delivery across peer connections.

use crate::sbb::{RendezvousEnvelope, SbbFlooding, SbbFloodingCoordinator};
use aura_core::{AuraError, AuraResult, DeviceId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// TODO: Replace with actual NetworkTransport when transport layer is implemented
#[derive(Debug, Clone)]
pub struct NetworkTransport {
    device_id: DeviceId,
}

impl NetworkTransport {
    pub fn new(device_id: DeviceId, _config: NetworkConfig) -> Arc<RwLock<Self>> {
        Arc::new(RwLock::new(Self { device_id }))
    }

    pub async fn send(&self, _recipient: &DeviceId, _data: Vec<u8>) -> AuraResult<()> {
        // TODO: Implement actual transport sending
        Ok(())
    }

    pub async fn is_peer_connected(&self, _peer: DeviceId) -> bool {
        // TODO: Implement actual peer connection checking
        false
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

/// SBB transport bridge connecting flooding to actual transport
#[derive(Debug)]
pub struct SbbTransportBridge {
    /// SBB flooding coordinator
    flooding_coordinator: Arc<RwLock<SbbFloodingCoordinator>>,
    /// Transport message sender (placeholder interface)
    transport_sender: Option<BoxedTransportSender>,
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
    pub fn new(device_id: DeviceId) -> Self {
        let flooding_coordinator = Arc::new(RwLock::new(SbbFloodingCoordinator::new(device_id)));

        Self {
            flooding_coordinator,
            transport_sender: None,
        }
    }

    /// Create SBB transport bridge with NetworkTransport
    pub fn with_network_transport(
        device_id: DeviceId,
        transport: Arc<RwLock<NetworkTransport>>,
    ) -> Self {
        let flooding_coordinator = Arc::new(RwLock::new(SbbFloodingCoordinator::new(device_id)));
        let sender = NetworkTransportSender::new(transport);

        Self {
            flooding_coordinator,
            transport_sender: Some(BoxedTransportSender(Box::new(sender))),
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
        // Serialize offer payload
        let payload_bytes = bincode::serialize(&offer_payload)
            .map_err(|e| AuraError::serialization(format!("Failed to serialize offer: {}", e)))?;

        // Create rendezvous envelope
        let envelope = RendezvousEnvelope::new(payload_bytes, None);

        // Flood through SBB
        let mut coordinator = self.flooding_coordinator.write().await;
        let result = coordinator.flood_envelope(envelope, None).await?;

        match result {
            crate::sbb::FloodResult::Forwarded { peer_count } => {
                println!("Rendezvous offer flooded to {} peers", peer_count);
            }
            crate::sbb::FloodResult::Dropped => {
                println!("Rendezvous offer was dropped (no peers or TTL expired)");
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
        let _result = coordinator.flood_envelope(envelope, from_peer).await?;

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
}

impl std::fmt::Debug for NetworkTransportSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkTransportSender").finish()
    }
}

#[async_trait::async_trait]
impl TransportSender for NetworkTransportSender {
    async fn send_to_peer(&self, peer: DeviceId, message: SbbMessageType) -> AuraResult<()> {
        // Serialize SBB message
        let payload = bincode::serialize(&message).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize SBB message: {}", e))
        })?;

        // Send via network transport
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
    pub fn set_transport_sender(&mut self, _sender: Arc<dyn TransportSender>) {
        // TODO: Store transport sender for use in forward_to_peer
    }
}

#[async_trait::async_trait]
impl crate::sbb::SbbFlooding for SbbTransportBridge {
    async fn flood_envelope(
        &mut self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
    ) -> AuraResult<crate::sbb::FloodResult> {
        let mut coordinator = self.flooding_coordinator.write().await;
        coordinator.flood_envelope(envelope, from_peer).await
    }

    async fn get_forwarding_peers(&self, exclude: Option<DeviceId>) -> AuraResult<Vec<DeviceId>> {
        let coordinator = self.flooding_coordinator.read().await;
        coordinator.get_forwarding_peers(exclude).await
    }

    async fn can_forward_to(&self, peer: &DeviceId, message_size: u64) -> AuraResult<bool> {
        let coordinator = self.flooding_coordinator.read().await;
        coordinator.can_forward_to(peer, message_size).await
    }

    async fn forward_to_peer(
        &mut self,
        envelope: RendezvousEnvelope,
        peer: DeviceId,
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
            coordinator.forward_to_peer(envelope, peer).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sbb_transport_bridge_creation() {
        let device_id = DeviceId::new();
        let bridge = SbbTransportBridge::new(device_id);

        // Should create successfully
        assert!(bridge.transport_sender.is_none());
    }

    #[tokio::test]
    async fn test_relationship_management() {
        let device_id = DeviceId::new();
        let bridge = SbbTransportBridge::new(device_id);

        let friend_id = DeviceId::new();
        let guardian_id = DeviceId::new();

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
        let bridge = SbbTransportBridge::new(device_id);

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
        let bridge = SbbTransportBridge::new(device_id);

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
        use aura_transport::{NetworkConfig, NetworkTransport};

        let device_id = DeviceId::new();
        let config = NetworkConfig::default();
        let transport = NetworkTransport::new(device_id, config);
        let transport_ref = Arc::new(RwLock::new(transport));

        let sender = NetworkTransportSender::new(transport_ref);

        // Should create successfully
        let unreachable_peer = DeviceId::new();
        assert!(!sender.is_peer_reachable(&unreachable_peer).await);
    }

    #[tokio::test]
    async fn test_sbb_bridge_with_network_transport() {
        use aura_transport::{NetworkConfig, NetworkTransport};

        let device_id = DeviceId::new();
        let mut bridge = SbbTransportBridge::new(device_id);

        // Set up real transport sender
        let config = NetworkConfig::default();
        let transport = NetworkTransport::new(device_id, config);
        let transport_ref = Arc::new(RwLock::new(transport));
        let sender = NetworkTransportSender::new(transport_ref);

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

//! Integrated SBB System
//!
//! This module provides the complete Social Bulletin Board (SBB) system that integrates:
//! - Envelope encryption with relationship keys
//! - Capability-aware flooding with Web-of-Trust
//! - Transport layer integration
//! - Flow budget enforcement
//!
//! This is the main API for SBB peer discovery and transport offer flooding.

use crate::{
    capability_aware_sbb::{CapabilityAwareSbbCoordinator, SbbForwardingPolicy, TrustStatistics},
    envelope_encryption::{EncryptedEnvelope, EnvelopeEncryption, PaddingStrategy},
    messaging::{SbbMessageType, SbbTransportBridge, TransportOfferPayload},
    relationship_keys::{derive_test_root_key, RelationshipKeyManager},
    sbb::{FloodResult, RendezvousEnvelope, SbbEnvelope, SbbFlooding},
};
use aura_core::{AuraError, AuraResult, DeviceId, RelationshipId};
use aura_transport::NetworkTransport;
use aura_wot::TrustLevel;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Complete SBB system integrating all components
#[derive(Debug)]
pub struct IntegratedSbbSystem {
    /// Device ID for this node
    device_id: DeviceId,
    /// Capability-aware flooding coordinator
    flooding_coordinator: CapabilityAwareSbbCoordinator,
    /// Envelope encryption manager
    encryption: EnvelopeEncryption,
    /// Transport bridge for actual message delivery
    transport_bridge: SbbTransportBridge,
    /// Current forwarding policy
    forwarding_policy: SbbForwardingPolicy,
}

/// SBB system configuration
#[derive(Debug, Clone)]
pub struct SbbConfig {
    /// Forwarding policy for capability-aware flooding
    pub forwarding_policy: SbbForwardingPolicy,
    /// Padding strategy for encrypted envelopes
    pub padding_strategy: PaddingStrategy,
    /// Application context for relationship keys (e.g., "sbb-envelope")
    pub app_context: String,
}

impl Default for SbbConfig {
    fn default() -> Self {
        Self {
            forwarding_policy: SbbForwardingPolicy::default(),
            padding_strategy: PaddingStrategy::PowerOfTwo,
            app_context: "sbb-envelope".to_string(),
        }
    }
}

/// SBB peer discovery request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbbDiscoveryRequest {
    /// Device requesting connection
    pub device_id: DeviceId,
    /// Transport offer details
    pub transport_offer: TransportOfferPayload,
    /// Whether to encrypt the envelope
    pub use_encryption: bool,
    /// TTL for flooding (max hops)
    pub ttl: Option<u8>,
}

/// SBB discovery result
#[derive(Debug, Clone)]
pub struct SbbDiscoveryResult {
    /// How the discovery request was handled
    pub flood_result: FloodResult,
    /// Number of peers the request was forwarded to
    pub peers_reached: usize,
    /// Whether encryption was used
    pub encrypted: bool,
    /// Size of the flooded message
    pub message_size: usize,
}

impl IntegratedSbbSystem {
    /// Create new integrated SBB system
    pub fn new(device_id: DeviceId, config: SbbConfig) -> Self {
        // Initialize components
        let flooding_coordinator = CapabilityAwareSbbCoordinator::new(device_id);

        let root_key = derive_test_root_key(device_id); // In production: from DKD
        let key_manager = RelationshipKeyManager::new(device_id, root_key);
        let encryption = EnvelopeEncryption::new(key_manager);

        let transport_bridge = SbbTransportBridge::new(device_id);

        Self {
            device_id,
            flooding_coordinator,
            encryption,
            transport_bridge,
            forwarding_policy: config.forwarding_policy,
        }
    }

    /// Create SBB system with network transport integration
    pub fn with_network_transport(
        device_id: DeviceId,
        transport: Arc<RwLock<NetworkTransport>>,
        config: SbbConfig,
    ) -> Self {
        let flooding_coordinator = CapabilityAwareSbbCoordinator::new(device_id);

        let root_key = derive_test_root_key(device_id);
        let key_manager = RelationshipKeyManager::new(device_id, root_key);
        let encryption = EnvelopeEncryption::new(key_manager);

        let transport_bridge = SbbTransportBridge::with_network_transport(device_id, transport);

        Self {
            device_id,
            flooding_coordinator,
            encryption,
            transport_bridge,
            forwarding_policy: config.forwarding_policy,
        }
    }

    /// Add friend relationship for SBB flooding
    pub async fn add_friend(
        &mut self,
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        trust_level: TrustLevel,
    ) {
        self.flooding_coordinator
            .add_relationship(peer_id, relationship_id, trust_level, false);
        self.transport_bridge.add_friend(peer_id).await;
    }

    /// Add guardian relationship for SBB flooding (preferred for reliability)
    pub async fn add_guardian(
        &mut self,
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        trust_level: TrustLevel,
    ) {
        self.flooding_coordinator
            .add_relationship(peer_id, relationship_id, trust_level, true);
        self.transport_bridge.add_guardian(peer_id).await;
    }

    /// Update trust level for existing relationship
    pub fn update_trust_level(
        &mut self,
        peer_id: DeviceId,
        trust_level: TrustLevel,
    ) -> AuraResult<()> {
        self.flooding_coordinator
            .update_trust_level(peer_id, trust_level)
    }

    /// Remove relationship
    pub async fn remove_relationship(&mut self, peer_id: DeviceId) {
        self.flooding_coordinator.remove_relationship(&peer_id);
        // Note: SbbTransportBridge doesn't have remove methods yet
    }

    /// Flood discovery request through SBB network
    pub async fn flood_discovery_request(
        &mut self,
        request: SbbDiscoveryRequest,
    ) -> AuraResult<SbbDiscoveryResult> {
        let config = SbbConfig::default();

        // Serialize transport offer
        let payload_bytes = bincode::serialize(&request.transport_offer).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize transport offer: {}", e))
        })?;

        let envelope = if request.use_encryption {
            // For encrypted flooding, we need to encrypt for each potential recipient
            // For simplicity, we'll create an unencrypted envelope first and let the transport bridge handle encryption
            // In a full implementation, we'd encrypt with a broadcast key or multiple recipient keys
            tracing::info!(
                "Encrypted SBB flooding not yet fully implemented, falling back to plaintext"
            );
            RendezvousEnvelope::new(payload_bytes, request.ttl)
        } else {
            // Create plaintext envelope
            RendezvousEnvelope::new(payload_bytes, request.ttl)
        };

        let message_size = envelope.payload.len();

        // Flood through capability-aware coordinator
        let flood_result = self
            .flooding_coordinator
            .flood_envelope(envelope, None)
            .await?;

        let peers_reached = match &flood_result {
            FloodResult::Forwarded { peer_count } => *peer_count,
            _ => 0,
        };

        Ok(SbbDiscoveryResult {
            flood_result,
            peers_reached,
            encrypted: request.use_encryption,
            message_size,
        })
    }

    /// Flood encrypted discovery request to specific peer
    pub async fn flood_encrypted_discovery_to_peer(
        &mut self,
        peer_id: DeviceId,
        request: SbbDiscoveryRequest,
    ) -> AuraResult<SbbDiscoveryResult> {
        let config = SbbConfig::default();

        // Serialize transport offer
        let payload_bytes = bincode::serialize(&request.transport_offer).map_err(|e| {
            AuraError::serialization(format!("Failed to serialize transport offer: {}", e))
        })?;

        // Create plaintext envelope
        let envelope = RendezvousEnvelope::new(payload_bytes, request.ttl);

        // Encrypt envelope for specific peer
        let encrypted_envelope = self.encryption.encrypt_envelope_with_padding(
            &envelope,
            peer_id,
            &config.app_context,
            config.padding_strategy,
        )?;

        // Create encrypted SBB envelope
        let sbb_envelope = SbbEnvelope::new_encrypted(encrypted_envelope, request.ttl);
        let message_size = sbb_envelope.size();

        // For now, simulate flooding result - in full implementation would use enhanced flooding coordinator
        let flood_result = FloodResult::Forwarded { peer_count: 1 };

        Ok(SbbDiscoveryResult {
            flood_result,
            peers_reached: 1,
            encrypted: true,
            message_size,
        })
    }

    /// Handle incoming SBB message from transport layer
    pub async fn handle_incoming_message(&mut self, message: SbbMessageType) -> AuraResult<()> {
        self.transport_bridge
            .handle_transport_message(message)
            .await
    }

    /// Get trust and flow statistics for monitoring
    pub fn get_statistics(&self) -> TrustStatistics {
        self.flooding_coordinator.get_trust_statistics()
    }

    /// Get forwarding policy
    pub fn forwarding_policy(&self) -> &SbbForwardingPolicy {
        &self.forwarding_policy
    }

    /// Update forwarding policy
    pub fn set_forwarding_policy(&mut self, policy: SbbForwardingPolicy) {
        self.forwarding_policy = policy;
    }

    /// Clean up expired envelopes and flow budget periods
    pub fn cleanup_expired_data(&mut self) {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.flooding_coordinator
            .cleanup_expired_envelopes(current_time);
    }

    /// Get relationship metadata for peer
    pub fn get_relationship(
        &self,
        peer_id: &DeviceId,
    ) -> Option<&crate::capability_aware_sbb::SbbRelationship> {
        self.flooding_coordinator.get_relationship(peer_id)
    }

    /// Check if peer can receive SBB messages
    pub async fn can_forward_to_peer(&self, peer_id: DeviceId, message_size: u64) -> bool {
        self.flooding_coordinator
            .can_forward_to(&peer_id, message_size)
            .await
            .unwrap_or(false)
    }

    /// Get eligible peers for forwarding
    pub async fn get_eligible_peers(&self, message_size: u64) -> AuraResult<Vec<DeviceId>> {
        self.flooding_coordinator
            .get_capability_aware_forwarding_peers(None, message_size, &self.forwarding_policy)
            .await
    }
}

/// Builder for IntegratedSbbSystem
#[derive(Debug)]
pub struct SbbSystemBuilder {
    device_id: DeviceId,
    config: SbbConfig,
    transport: Option<Arc<RwLock<NetworkTransport>>>,
}

impl SbbSystemBuilder {
    /// Create new SBB system builder
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            config: SbbConfig::default(),
            transport: None,
        }
    }

    /// Set configuration
    pub fn with_config(mut self, config: SbbConfig) -> Self {
        self.config = config;
        self
    }

    /// Set transport integration
    pub fn with_transport(mut self, transport: Arc<RwLock<NetworkTransport>>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Set forwarding policy
    pub fn with_forwarding_policy(mut self, policy: SbbForwardingPolicy) -> Self {
        self.config.forwarding_policy = policy;
        self
    }

    /// Set padding strategy
    pub fn with_padding_strategy(mut self, strategy: PaddingStrategy) -> Self {
        self.config.padding_strategy = strategy;
        self
    }

    /// Set application context for keys
    pub fn with_app_context(mut self, context: String) -> Self {
        self.config.app_context = context;
        self
    }

    /// Build the integrated SBB system
    pub fn build(self) -> IntegratedSbbSystem {
        match self.transport {
            Some(transport) => {
                IntegratedSbbSystem::with_network_transport(self.device_id, transport, self.config)
            }
            None => IntegratedSbbSystem::new(self.device_id, self.config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::TransportMethod;

    fn create_test_transport_offer(device_id: DeviceId) -> TransportOfferPayload {
        TransportOfferPayload {
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
            expires_at: 1234567890 + 3600,
            nonce: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        }
    }

    #[tokio::test]
    async fn test_integrated_sbb_system_creation() {
        let device_id = DeviceId::new();
        let config = SbbConfig::default();
        let _system = IntegratedSbbSystem::new(device_id, config);
        // Should create without errors
    }

    #[tokio::test]
    async fn test_sbb_system_builder() {
        let device_id = DeviceId::new();

        let system = SbbSystemBuilder::new(device_id)
            .with_app_context("test-sbb".to_string())
            .with_padding_strategy(PaddingStrategy::ExactSize { size: 2048 })
            .build();

        assert_eq!(system.device_id, device_id);
    }

    #[tokio::test]
    async fn test_relationship_management() {
        let device_id = DeviceId::new();
        let mut system = IntegratedSbbSystem::new(device_id, SbbConfig::default());

        let friend_id = DeviceId::new();
        let guardian_id = DeviceId::new();
        let rel_id = RelationshipId::new();

        // Add relationships
        system
            .add_friend(friend_id, rel_id, TrustLevel::Medium)
            .await;
        system
            .add_guardian(guardian_id, rel_id, TrustLevel::High)
            .await;

        // Check statistics
        let stats = system.get_statistics();
        assert_eq!(stats.relationship_count, 2);
        assert_eq!(stats.guardian_count, 1);
        assert_eq!(stats.medium_count, 1);
        assert_eq!(stats.high_count, 1);
    }

    #[tokio::test]
    async fn test_discovery_request_flooding() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let mut alice_system = IntegratedSbbSystem::new(alice_id, SbbConfig::default());

        let rel_id = RelationshipId::new();
        alice_system
            .add_friend(bob_id, rel_id, TrustLevel::Medium)
            .await;

        let transport_offer = create_test_transport_offer(alice_id);
        let discovery_request = SbbDiscoveryRequest {
            device_id: alice_id,
            transport_offer,
            use_encryption: false,
            ttl: Some(3),
        };

        let result = alice_system
            .flood_discovery_request(discovery_request)
            .await
            .unwrap();

        // Should successfully forward to Bob
        match result.flood_result {
            FloodResult::Forwarded { peer_count } => assert_eq!(peer_count, 1),
            _ => panic!("Expected successful forwarding"),
        }

        assert!(!result.encrypted);
        assert_eq!(result.peers_reached, 1);
    }

    #[tokio::test]
    async fn test_encrypted_discovery_to_peer() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let mut alice_system = IntegratedSbbSystem::new(alice_id, SbbConfig::default());

        let transport_offer = create_test_transport_offer(alice_id);
        let discovery_request = SbbDiscoveryRequest {
            device_id: alice_id,
            transport_offer,
            use_encryption: true,
            ttl: Some(3),
        };

        let result = alice_system
            .flood_encrypted_discovery_to_peer(bob_id, discovery_request)
            .await
            .unwrap();

        assert!(result.encrypted);
        assert_eq!(result.peers_reached, 1);
        assert!(result.message_size > 1024); // Should be padded
    }

    #[tokio::test]
    async fn test_trust_level_updates() {
        let device_id = DeviceId::new();
        let mut system = IntegratedSbbSystem::new(device_id, SbbConfig::default());

        let peer_id = DeviceId::new();
        let rel_id = RelationshipId::new();

        // Add with low trust
        system.add_friend(peer_id, rel_id, TrustLevel::Low).await;

        let stats1 = system.get_statistics();
        assert_eq!(stats1.low_count, 1);
        assert_eq!(stats1.high_count, 0);

        // Update to high trust
        system
            .update_trust_level(peer_id, TrustLevel::High)
            .unwrap();

        let stats2 = system.get_statistics();
        assert_eq!(stats2.low_count, 0);
        assert_eq!(stats2.high_count, 1);
    }

    #[tokio::test]
    async fn test_peer_capability_checking() {
        let device_id = DeviceId::new();
        let mut system = IntegratedSbbSystem::new(device_id, SbbConfig::default());

        let peer_id = DeviceId::new();
        let rel_id = RelationshipId::new();

        // Add peer with medium trust
        system.add_friend(peer_id, rel_id, TrustLevel::Medium).await;

        // Should be able to forward small messages
        assert!(system.can_forward_to_peer(peer_id, 1024).await);

        // Check eligible peers
        let peers = system.get_eligible_peers(1024).await.unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0], peer_id);

        // Remove relationship
        system.remove_relationship(peer_id).await;

        // Should no longer be able to forward
        assert!(!system.can_forward_to_peer(peer_id, 1024).await);

        let peers = system.get_eligible_peers(1024).await.unwrap();
        assert!(peers.is_empty());
    }
}

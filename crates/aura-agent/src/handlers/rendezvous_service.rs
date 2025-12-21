//! Rendezvous Service - Public API for Rendezvous Operations
//!
//! Provides a clean public interface for rendezvous operations.
//! Wraps `RendezvousHandler` with ergonomic methods and proper error handling.

use super::rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::sync::Arc;

/// Rendezvous service
///
/// Provides rendezvous operations through a clean public API.
pub struct RendezvousServiceApi {
    handler: RendezvousHandler,
    effects: Arc<AuraEffectSystem>,
}

impl RendezvousServiceApi {
    /// Create a new rendezvous service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = RendezvousHandler::new(authority_context)?;
        Ok(Self { handler, effects })
    }

    // ========================================================================
    // Descriptor Operations
    // ========================================================================

    /// Publish a transport descriptor for the local authority
    ///
    /// # Arguments
    /// * `context_id` - Context to publish descriptor for
    /// * `transport_hints` - How peers can connect to us
    /// * `psk_commitment` - Pre-shared key commitment for handshake
    /// * `validity_duration_ms` - How long the descriptor should be valid
    ///
    /// # Returns
    /// Result of the publication
    pub async fn publish_descriptor(
        &self,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        psk_commitment: [u8; 32],
        validity_duration_ms: u64,
    ) -> AgentResult<RendezvousResult> {
        self.handler
            .publish_descriptor(
                &self.effects,
                context_id,
                transport_hints,
                psk_commitment,
                validity_duration_ms,
            )
            .await
    }

    /// Publish a descriptor with default QUIC transport
    ///
    /// # Arguments
    /// * `context_id` - Context to publish descriptor for
    /// * `listen_addr` - Address to listen on (e.g., "0.0.0.0:8443")
    /// * `psk_commitment` - Pre-shared key commitment for handshake
    ///
    /// # Returns
    /// Result of the publication
    pub async fn publish_quic_descriptor(
        &self,
        context_id: ContextId,
        listen_addr: String,
        psk_commitment: [u8; 32],
    ) -> AgentResult<RendezvousResult> {
        self.publish_descriptor(
            context_id,
            vec![TransportHint::QuicDirect { addr: listen_addr }],
            psk_commitment,
            3600000, // 1 hour default
        )
        .await
    }

    /// Cache a peer's descriptor received via journal sync
    ///
    /// # Arguments
    /// * `descriptor` - The peer's transport descriptor
    pub async fn cache_peer_descriptor(&self, descriptor: RendezvousDescriptor) {
        self.handler.cache_peer_descriptor(descriptor).await;
    }

    /// Get a peer's cached descriptor
    ///
    /// # Arguments
    /// * `context_id` - Context to look up
    /// * `peer` - Peer authority ID
    ///
    /// # Returns
    /// The descriptor if cached
    pub async fn get_peer_descriptor(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<RendezvousDescriptor> {
        self.handler.get_peer_descriptor(context_id, peer).await
    }

    /// Check if our descriptor needs refresh
    ///
    /// # Arguments
    /// * `context_id` - Context to check
    /// * `refresh_window_ms` - How long before expiry to trigger refresh
    ///
    /// # Returns
    /// True if descriptor should be refreshed
    pub async fn needs_refresh(
        &self,
        context_id: ContextId,
        refresh_window_ms: u64,
    ) -> AgentResult<bool> {
        let now_ms = self.effects.current_timestamp().await.unwrap_or(0);
        Ok(self
            .handler
            .needs_descriptor_refresh(context_id, now_ms, refresh_window_ms)
            .await)
    }

    // ========================================================================
    // Channel Operations
    // ========================================================================

    /// Initiate a secure channel with a peer
    ///
    /// # Arguments
    /// * `context_id` - Context for the channel
    /// * `peer` - Peer to connect to
    ///
    /// # Returns
    /// Result of channel initiation (transport info)
    pub async fn initiate_channel(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<ChannelResult> {
        self.handler
            .initiate_channel(&self.effects, context_id, peer)
            .await
    }

    /// Complete channel establishment after handshake
    ///
    /// # Arguments
    /// * `context_id` - Context for the channel
    /// * `peer` - Peer at other end
    /// * `channel_id` - Established channel identifier
    /// * `epoch` - Channel epoch
    ///
    /// # Returns
    /// Result of channel completion
    pub async fn complete_channel(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> AgentResult<ChannelResult> {
        self.handler
            .complete_channel(&self.effects, context_id, peer, channel_id, epoch)
            .await
    }

    // ========================================================================
    // Relay Operations
    // ========================================================================

    /// Request relay assistance to reach a peer
    ///
    /// # Arguments
    /// * `context_id` - Context for the relay
    /// * `relay` - Authority to act as relay
    /// * `target` - Authority we want to reach
    ///
    /// # Returns
    /// Result of relay request
    pub async fn request_relay(
        &self,
        context_id: ContextId,
        relay: AuthorityId,
        target: AuthorityId,
    ) -> AgentResult<RendezvousResult> {
        self.handler
            .request_relay(&self.effects, context_id, relay, target)
            .await
    }

    // ========================================================================
    // Maintenance Operations
    // ========================================================================

    /// Clean up expired descriptors
    pub async fn cleanup_expired(&self) -> AgentResult<()> {
        let now_ms = self.effects.current_timestamp().await.unwrap_or(0);
        self.handler.cleanup_expired(now_ms).await;
        Ok(())
    }

    /// Get the authority context
    pub fn authority_context(&self) -> &AuthorityContext {
        self.handler.authority_context()
    }
}

use aura_protocol::effects::EffectApiEffects;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::context::RelationalContext;
    use crate::core::AgentConfig;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(RelationalContext {
            context_id: ContextId::new_from_entropy([seed + 100; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        authority_context
    }

    #[tokio::test]
    async fn test_service_creation() {
        let authority_context = create_test_authority(60);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_publish_quic_descriptor() {
        let authority_context = create_test_authority(61);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([161u8; 32]);
        let result = service
            .publish_quic_descriptor(context_id, "0.0.0.0:8443".to_string(), [0u8; 32])
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_cache_and_get_descriptor() {
        let authority_context = create_test_authority(62);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([162u8; 32]);
        let peer = AuthorityId::new_from_entropy([63u8; 32]);

        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "10.0.0.1:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            display_name: None,
        };

        service.cache_peer_descriptor(descriptor.clone()).await;

        let cached = service.get_peer_descriptor(context_id, peer).await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().authority_id, peer);
    }

    #[tokio::test]
    async fn test_channel_workflow() {
        let authority_context = create_test_authority(64);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([164u8; 32]);
        let peer = AuthorityId::new_from_entropy([65u8; 32]);

        // Cache peer descriptor first
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "10.0.0.2:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            display_name: None,
        };
        service.cache_peer_descriptor(descriptor).await;

        // Initiate channel
        let init_result = service.initiate_channel(context_id, peer).await.unwrap();
        assert!(init_result.success);

        // Complete channel
        let channel_id = [77u8; 32];
        let complete_result = service
            .complete_channel(context_id, peer, channel_id, 1)
            .await
            .unwrap();
        assert!(complete_result.success);
        assert_eq!(complete_result.channel_id, Some(channel_id));
    }

    #[tokio::test]
    async fn test_relay_request() {
        let authority_context = create_test_authority(66);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([166u8; 32]);
        let relay = AuthorityId::new_from_entropy([67u8; 32]);
        let target = AuthorityId::new_from_entropy([68u8; 32]);

        let result = service
            .request_relay(context_id, relay, target)
            .await
            .unwrap();
        // Note: Relay support is not yet implemented in Phase 1,
        // so this returns success=false with an error message
        assert!(!result.success);
        assert!(result.error.is_some());
    }
}

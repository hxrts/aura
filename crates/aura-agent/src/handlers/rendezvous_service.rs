//! Rendezvous Service - Public API for Rendezvous Operations
//!
//! Provides a clean public interface for rendezvous operations.
//! Wraps `RendezvousHandler` with ergonomic methods and proper error handling.

use super::rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::choreography_adapter::AuraProtocolAdapter;
use crate::runtime::AuraEffectSystem;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_rendezvous::protocol::exchange_runners::{
    execute_as as exchange_execute_as, RendezvousExchangeRole,
};
use aura_rendezvous::protocol::relayed_runners::{
    execute_as as relayed_execute_as, RelayedRendezvousRole,
};
use aura_rendezvous::protocol::{
    DescriptorAnswer, DescriptorOffer, HandshakeComplete, HandshakeInit, RelayComplete,
    RelayForward, RelayRequest, RelayResponse,
};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

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
        let hint = TransportHint::quic_direct(&listen_addr)
            .map_err(|e| crate::core::AgentError::invalid(format!("Invalid address: {}", e)))?;
        self.publish_descriptor(context_id, vec![hint], psk_commitment, 3600000)
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
    // Choreography Wiring (execute_as)
    // ========================================================================

    /// Execute direct rendezvous exchange as initiator.
    pub async fn execute_rendezvous_exchange_initiator(
        &self,
        context_id: ContextId,
        responder: AuthorityId,
        offer_descriptor: RendezvousDescriptor,
        handshake_init: HandshakeInit,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(RendezvousExchangeRole::Initiator, authority_id);
        role_map.insert(RendezvousExchangeRole::Responder, responder);

        let offer_type = std::any::type_name::<DescriptorOffer>();
        let handshake_type = std::any::type_name::<HandshakeInit>();

        let mut offers = VecDeque::new();
        offers.push_back(DescriptorOffer {
            descriptor: offer_descriptor,
        });
        let mut handshakes = VecDeque::new();
        handshakes.push_back(handshake_init);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            RendezvousExchangeRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if request.type_name == offer_type {
                return offers
                    .pop_front()
                    .map(|offer| Box::new(offer) as Box<dyn std::any::Any + Send>);
            }
            if request.type_name == handshake_type {
                return handshakes
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_id = rendezvous_exchange_session_id(context_id, authority_id, responder);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("rendezvous exchange start failed: {e}")))?;

        let result = exchange_execute_as(RendezvousExchangeRole::Initiator, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("rendezvous exchange failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Execute direct rendezvous exchange as responder.
    pub async fn execute_rendezvous_exchange_responder(
        &self,
        context_id: ContextId,
        initiator: AuthorityId,
        answer_descriptor: RendezvousDescriptor,
        handshake_complete: HandshakeComplete,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(RendezvousExchangeRole::Initiator, initiator);
        role_map.insert(RendezvousExchangeRole::Responder, authority_id);

        let answer_type = std::any::type_name::<DescriptorAnswer>();
        let completion_type = std::any::type_name::<HandshakeComplete>();

        let mut answers = VecDeque::new();
        answers.push_back(DescriptorAnswer {
            descriptor: answer_descriptor,
        });
        let mut completions = VecDeque::new();
        completions.push_back(handshake_complete);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            RendezvousExchangeRole::Responder,
            role_map,
        )
        .with_message_provider(move |request, _received| {
            if request.type_name == answer_type {
                return answers
                    .pop_front()
                    .map(|answer| Box::new(answer) as Box<dyn std::any::Any + Send>);
            }
            if request.type_name == completion_type {
                return completions
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_id = rendezvous_exchange_session_id(context_id, initiator, authority_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("rendezvous exchange start failed: {e}")))?;

        let result = exchange_execute_as(RendezvousExchangeRole::Responder, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("rendezvous exchange failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Execute relayed rendezvous as initiator.
    pub async fn execute_relayed_rendezvous_initiator(
        &self,
        context_id: ContextId,
        relay: AuthorityId,
        responder: AuthorityId,
        request: RelayRequest,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(RelayedRendezvousRole::Initiator, authority_id);
        role_map.insert(RelayedRendezvousRole::Relay, relay);
        role_map.insert(RelayedRendezvousRole::Responder, responder);

        let request_type = std::any::type_name::<RelayRequest>();
        let mut requests = VecDeque::new();
        requests.push_back(request);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            RelayedRendezvousRole::Initiator,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if request_ctx.type_name == request_type {
                return requests
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_id = relayed_rendezvous_session_id(context_id, authority_id, relay, responder);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous start failed: {e}")))?;

        let result = relayed_execute_as(RelayedRendezvousRole::Initiator, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Execute relayed rendezvous as relay.
    pub async fn execute_relayed_rendezvous_relay(
        &self,
        context_id: ContextId,
        initiator: AuthorityId,
        responder: AuthorityId,
        forward: RelayForward,
        complete: RelayComplete,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(RelayedRendezvousRole::Initiator, initiator);
        role_map.insert(RelayedRendezvousRole::Relay, authority_id);
        role_map.insert(RelayedRendezvousRole::Responder, responder);

        let forward_type = std::any::type_name::<RelayForward>();
        let complete_type = std::any::type_name::<RelayComplete>();

        let mut forwards = VecDeque::new();
        forwards.push_back(forward);
        let mut completes = VecDeque::new();
        completes.push_back(complete);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            RelayedRendezvousRole::Relay,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if request_ctx.type_name == forward_type {
                return forwards
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            if request_ctx.type_name == complete_type {
                return completes
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_id =
            relayed_rendezvous_session_id(context_id, initiator, authority_id, responder);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous start failed: {e}")))?;

        let result = relayed_execute_as(RelayedRendezvousRole::Relay, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous failed: {e}")));

        let _ = adapter.end_session().await;
        result
    }

    /// Execute relayed rendezvous as responder.
    pub async fn execute_relayed_rendezvous_responder(
        &self,
        context_id: ContextId,
        initiator: AuthorityId,
        relay: AuthorityId,
        response: RelayResponse,
    ) -> AgentResult<()> {
        use crate::core::AgentError;

        let authority_id = self.handler.authority_context().authority_id();

        let mut role_map = HashMap::new();
        role_map.insert(RelayedRendezvousRole::Initiator, initiator);
        role_map.insert(RelayedRendezvousRole::Relay, relay);
        role_map.insert(RelayedRendezvousRole::Responder, authority_id);

        let response_type = std::any::type_name::<RelayResponse>();
        let mut responses = VecDeque::new();
        responses.push_back(response);

        let mut adapter = AuraProtocolAdapter::new(
            self.effects.clone(),
            authority_id,
            RelayedRendezvousRole::Responder,
            role_map,
        )
        .with_message_provider(move |request_ctx, _received| {
            if request_ctx.type_name == response_type {
                return responses
                    .pop_front()
                    .map(|msg| Box::new(msg) as Box<dyn std::any::Any + Send>);
            }
            None
        });

        let session_id = relayed_rendezvous_session_id(context_id, initiator, relay, authority_id);
        adapter
            .start_session(session_id)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous start failed: {e}")))?;

        let result = relayed_execute_as(RelayedRendezvousRole::Responder, &mut adapter)
            .await
            .map_err(|e| AgentError::internal(format!("relayed rendezvous failed: {e}")));

        let _ = adapter.end_session().await;
        result
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

fn rendezvous_exchange_session_id(
    context_id: ContextId,
    initiator: AuthorityId,
    responder: AuthorityId,
) -> Uuid {
    let mut material = Vec::new();
    material.extend_from_slice(context_id.as_bytes());
    material.extend_from_slice(&initiator.to_bytes());
    material.extend_from_slice(&responder.to_bytes());
    material.extend_from_slice(b"rendezvous-exchange");
    let digest = hash(&material);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

fn relayed_rendezvous_session_id(
    context_id: ContextId,
    initiator: AuthorityId,
    relay: AuthorityId,
    responder: AuthorityId,
) -> Uuid {
    let mut material = Vec::new();
    material.extend_from_slice(context_id.as_bytes());
    material.extend_from_slice(&initiator.to_bytes());
    material.extend_from_slice(&relay.to_bytes());
    material.extend_from_slice(&responder.to_bytes());
    material.extend_from_slice(b"rendezvous-relayed");
    let digest = hash(&material);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

use aura_protocol::effects::EffectApiEffects;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;

    fn create_test_authority(seed: u8) -> AuthorityContext {
        let authority_id = AuthorityId::new_from_entropy([seed; 32]);
        AuthorityContext::new(authority_id)
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
            transport_hints: vec![TransportHint::quic_direct("10.0.0.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
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
            transport_hints: vec![TransportHint::quic_direct("10.0.0.2:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: u64::MAX,
            nonce: [0u8; 32],
            nickname_suggestion: None,
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

//! Rendezvous Service - Public API for Rendezvous Operations
//!
//! Provides a clean public interface for rendezvous operations.
//! Wraps `RendezvousHandler` with ergonomic methods and proper error handling.

use super::rendezvous::{ChannelResult, RendezvousHandler, RendezvousResult};
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::runtime::services::ceremony_runner::{
    CeremonyCommitMetadata, CeremonyInitRequest, CeremonyRunner,
};
use crate::runtime::vm_host_bridge::{
    close_and_reap_vm_session, flush_pending_vm_sends, inject_vm_receive,
    open_manifest_vm_session_admitted, receive_blocked_vm_message,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, CeremonyId, ContextId};
use aura_core::util::serialization::to_vec;
use aura_core::Hash32;
use aura_mpst::CompositionManifest;
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole, RoleIndex};
use aura_rendezvous::protocol::{
    DescriptorAnswer, DescriptorOffer, HandshakeComplete, HandshakeInit, RelayComplete,
    RelayForward, RelayRequest, RelayResponse,
};
use aura_rendezvous::{RendezvousDescriptor, TransportHint};
use std::collections::BTreeMap;
use std::sync::Arc;
use telltale_vm::vm::StepResult;
use uuid::Uuid;

/// Rendezvous service
///
/// Provides rendezvous operations through a clean public API.
pub struct RendezvousServiceApi {
    handler: RendezvousHandler,
    effects: Arc<AuraEffectSystem>,
    ceremony_runner: CeremonyRunner,
}

impl RendezvousServiceApi {
    /// Create a new rendezvous service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
    ) -> AgentResult<Self> {
        let handler = RendezvousHandler::new(authority_context)?;
        let time_effects: Arc<dyn PhysicalTimeEffects> = Arc::new(effects.time_effects().clone());
        let ceremony_runner =
            CeremonyRunner::new(crate::runtime::services::CeremonyTracker::new(time_effects));
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    /// Create a new rendezvous service with a shared ceremony runner.
    pub fn new_with_runner(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        ceremony_runner: CeremonyRunner,
    ) -> AgentResult<Self> {
        let handler = RendezvousHandler::new(authority_context)?;
        Ok(Self {
            handler,
            effects,
            ceremony_runner,
        })
    }

    fn rendezvous_ceremony_id(&self, context_id: ContextId, peer: AuthorityId) -> CeremonyId {
        let mut material = Vec::with_capacity(32 * 3);
        material.extend_from_slice(context_id.as_bytes());
        material.extend_from_slice(&self.handler.authority_context().authority_id().to_bytes());
        material.extend_from_slice(&peer.to_bytes());
        let digest = hash(&material);
        CeremonyId::new(format!("rendezvous-{}", hex::encode(digest)))
    }

    async fn ensure_rendezvous_ceremony(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<CeremonyId> {
        let ceremony_id = self.rendezvous_ceremony_id(context_id, peer);
        if self.ceremony_runner.status(&ceremony_id).await.is_ok() {
            return Ok(ceremony_id);
        }

        let prestate_hash = Hash32(hash(context_id.as_bytes()));
        let initiator_id = self.handler.authority_context().authority_id();
        let participants = vec![aura_core::threshold::ParticipantIdentity::guardian(
            initiator_id,
        )];

        self.ceremony_runner
            .start(CeremonyInitRequest {
                ceremony_id: ceremony_id.clone(),
                kind: aura_app::runtime_bridge::CeremonyKind::RendezvousSecureChannel,
                initiator_id,
                threshold_k: 1,
                total_n: 1,
                participants,
                new_epoch: 0,
                enrollment_device_id: None,
                enrollment_nickname_suggestion: None,
                prestate_hash: Some(prestate_hash),
            })
            .await
            .map_err(|e| AgentError::internal(format!("Failed to register ceremony: {e}")))?;

        Ok(ceremony_id)
    }

    fn rendezvous_role(authority_id: AuthorityId) -> ChoreographicRole {
        ChoreographicRole::new(
            aura_core::DeviceId::from_uuid(authority_id.0),
            RoleIndex::new(0).expect("role index"),
        )
    }

    async fn run_vm_protocol(
        &self,
        session_uuid: Uuid,
        roles: Vec<ChoreographicRole>,
        peer_roles: BTreeMap<String, ChoreographicRole>,
        active_role: &str,
        manifest: &CompositionManifest,
        global_type: &aura_mpst::telltale_types::GlobalType,
        local_types: &BTreeMap<String, aura_mpst::telltale_types::LocalTypeR>,
        initial_payloads: Vec<Vec<u8>>,
    ) -> AgentResult<()> {
        self.effects
            .start_session(session_uuid, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("rendezvous VM session start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                manifest,
                active_role,
                global_type,
                local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;

            for payload in initial_payloads {
                handler.push_send_bytes(payload);
            }

            let loop_result = loop {
                let step = engine.step().map_err(|error| {
                    AgentError::internal(format!(
                        "rendezvous {active_role} VM step failed: {error}"
                    ))
                })?;
                flush_pending_vm_sends(self.effects.as_ref(), handler.as_ref(), &peer_roles)
                    .await
                    .map_err(AgentError::internal)?;

                if let Some(blocked) = receive_blocked_vm_message(
                    self.effects.as_ref(),
                    engine.vm(),
                    vm_sid,
                    active_role,
                    &peer_roles,
                )
                .await
                .map_err(|error| {
                    AgentError::internal(format!(
                        "rendezvous {active_role} VM receive failed: {error}"
                    ))
                })? {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(format!(
                            "rendezvous {active_role} VM became stuck without a pending receive"
                        )));
                    }
                }
            };

            let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            loop_result
        }
        .await;

        let _ = self.effects.end_session().await;
        result
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
        let _ = self.ensure_rendezvous_ceremony(context_id, peer).await?;
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
        let result = self
            .handler
            .complete_channel(&self.effects, context_id, peer, channel_id, epoch)
            .await?;

        let ceremony_id = self.ensure_rendezvous_ceremony(context_id, peer).await?;
        let _ = self
            .ceremony_runner
            .record_response(
                &ceremony_id,
                aura_core::threshold::ParticipantIdentity::guardian(
                    self.handler.authority_context().authority_id(),
                ),
            )
            .await
            .map_err(|e| {
                AgentError::internal(format!("Failed to record rendezvous completion: {e}"))
            })?;
        let _ = self
            .ceremony_runner
            .commit(&ceremony_id, CeremonyCommitMetadata::default())
            .await;

        Ok(result)
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
        let authority_id = self.handler.authority_context().authority_id();
        let session_id = rendezvous_exchange_session_id(context_id, authority_id, responder);
        let manifest =
            aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::composition_manifest();
        let global_type = aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::global_type();
        let local_types = aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::local_types();

        self.run_vm_protocol(
            session_id,
            vec![
                Self::rendezvous_role(authority_id),
                Self::rendezvous_role(responder),
            ],
            BTreeMap::from([("Responder".to_string(), Self::rendezvous_role(responder))]),
            "Initiator",
            &manifest,
            &global_type,
            &local_types,
            vec![
                to_vec(&DescriptorOffer {
                    descriptor: offer_descriptor,
                })
                .map_err(|error| {
                    AgentError::internal(format!("rendezvous offer encode failed: {error}"))
                })?,
                to_vec(&handshake_init).map_err(|error| {
                    AgentError::internal(format!("rendezvous handshake encode failed: {error}"))
                })?,
            ],
        )
        .await
    }

    /// Execute direct rendezvous exchange as responder.
    pub async fn execute_rendezvous_exchange_responder(
        &self,
        context_id: ContextId,
        initiator: AuthorityId,
        answer_descriptor: RendezvousDescriptor,
        handshake_complete: HandshakeComplete,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let session_id = rendezvous_exchange_session_id(context_id, initiator, authority_id);
        let manifest =
            aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::composition_manifest();
        let global_type = aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::global_type();
        let local_types = aura_rendezvous::protocol::exchange::telltale_session_types_rendezvous::vm_artifacts::local_types();

        self.run_vm_protocol(
            session_id,
            vec![
                Self::rendezvous_role(initiator),
                Self::rendezvous_role(authority_id),
            ],
            BTreeMap::from([("Initiator".to_string(), Self::rendezvous_role(initiator))]),
            "Responder",
            &manifest,
            &global_type,
            &local_types,
            vec![
                to_vec(&DescriptorAnswer {
                    descriptor: answer_descriptor,
                })
                .map_err(|error| {
                    AgentError::internal(format!("rendezvous answer encode failed: {error}"))
                })?,
                to_vec(&handshake_complete).map_err(|error| {
                    AgentError::internal(format!(
                        "rendezvous handshake completion encode failed: {error}"
                    ))
                })?,
            ],
        )
        .await
    }

    /// Execute relayed rendezvous as initiator.
    pub async fn execute_relayed_rendezvous_initiator(
        &self,
        context_id: ContextId,
        relay: AuthorityId,
        responder: AuthorityId,
        request: RelayRequest,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let session_id = relayed_rendezvous_session_id(context_id, authority_id, relay, responder);
        let manifest = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::composition_manifest();
        let global_type = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::global_type();
        let local_types = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::local_types();

        self.run_vm_protocol(
            session_id,
            vec![
                Self::rendezvous_role(authority_id),
                Self::rendezvous_role(relay),
                Self::rendezvous_role(responder),
            ],
            BTreeMap::from([
                ("Relay".to_string(), Self::rendezvous_role(relay)),
                ("Responder".to_string(), Self::rendezvous_role(responder)),
            ]),
            "Initiator",
            &manifest,
            &global_type,
            &local_types,
            vec![to_vec(&request).map_err(|error| {
                AgentError::internal(format!("relayed rendezvous request encode failed: {error}"))
            })?],
        )
        .await
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
        let authority_id = self.handler.authority_context().authority_id();
        let session_id =
            relayed_rendezvous_session_id(context_id, initiator, authority_id, responder);
        let manifest = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::composition_manifest();
        let global_type = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::global_type();
        let local_types = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::local_types();

        self.run_vm_protocol(
            session_id,
            vec![
                Self::rendezvous_role(initiator),
                Self::rendezvous_role(authority_id),
                Self::rendezvous_role(responder),
            ],
            BTreeMap::from([
                ("Initiator".to_string(), Self::rendezvous_role(initiator)),
                ("Responder".to_string(), Self::rendezvous_role(responder)),
            ]),
            "Relay",
            &manifest,
            &global_type,
            &local_types,
            vec![
                to_vec(&forward).map_err(|error| {
                    AgentError::internal(format!("relay forward encode failed: {error}"))
                })?,
                to_vec(&complete).map_err(|error| {
                    AgentError::internal(format!("relay complete encode failed: {error}"))
                })?,
            ],
        )
        .await
    }

    /// Execute relayed rendezvous as responder.
    pub async fn execute_relayed_rendezvous_responder(
        &self,
        context_id: ContextId,
        initiator: AuthorityId,
        relay: AuthorityId,
        response: RelayResponse,
    ) -> AgentResult<()> {
        let authority_id = self.handler.authority_context().authority_id();
        let session_id = relayed_rendezvous_session_id(context_id, initiator, relay, authority_id);
        let manifest = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::composition_manifest();
        let global_type = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::global_type();
        let local_types = aura_rendezvous::protocol::relayed::telltale_session_types_rendezvous_relay::vm_artifacts::local_types();

        self.run_vm_protocol(
            session_id,
            vec![
                Self::rendezvous_role(initiator),
                Self::rendezvous_role(relay),
                Self::rendezvous_role(authority_id),
            ],
            BTreeMap::from([("Relay".to_string(), Self::rendezvous_role(relay))]),
            "Responder",
            &manifest,
            &global_type,
            &local_types,
            vec![to_vec(&response).map_err(|error| {
                AgentError::internal(format!("relay response encode failed: {error}"))
            })?],
        )
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context);
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_publish_quic_descriptor() {
        let authority_context = create_test_authority(61);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([162u8; 32]);
        let peer = AuthorityId::new_from_entropy([63u8; 32]);

        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("10.0.0.1:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

        let service = RendezvousServiceApi::new(effects, authority_context).unwrap();

        let context_id = ContextId::new_from_entropy([164u8; 32]);
        let peer = AuthorityId::new_from_entropy([65u8; 32]);

        // Cache peer descriptor first
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id,
            transport_hints: vec![TransportHint::quic_direct("10.0.0.2:8443").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
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
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

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

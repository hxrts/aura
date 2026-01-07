//! Rendezvous Service
//!
//! Main coordinator for peer discovery and channel establishment.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.

use crate::descriptor::{DescriptorBuilder, SelectedTransport, TransportSelector};
use crate::facts::{RendezvousDescriptor, RendezvousFact, TransportHint};
use crate::new_channel::{HandshakeConfig, Handshaker, SecureChannel};
use crate::protocol::{guards, HandshakeComplete, HandshakeInit, NoiseHandshake};
use aura_core::effects::noise::NoiseEffects;
use aura_core::effects::CryptoEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_core::{AuraError, AuraResult};
use aura_guards::types;
use std::collections::HashMap;
use tokio::sync::RwLock;

// =============================================================================
// Service Configuration
// =============================================================================

/// Configuration for the rendezvous service
#[derive(Debug, Clone)]
pub struct RendezvousConfig {
    /// Default descriptor validity duration in milliseconds
    pub descriptor_validity_ms: u64,
    /// STUN server for reflexive address discovery
    pub stun_server: Option<String>,
    /// Probe timeout in milliseconds
    pub probe_timeout_ms: u64,
    /// Maximum relay hops
    pub max_relay_hops: u8,
}

impl Default for RendezvousConfig {
    fn default() -> Self {
        Self {
            descriptor_validity_ms: 3_600_000, // 1 hour
            stun_server: None,
            probe_timeout_ms: 5000, // 5 seconds
            max_relay_hops: 3,
        }
    }
}

// =============================================================================
// Guard Types
// =============================================================================

/// Snapshot of guard-relevant state for evaluation
#[derive(Debug, Clone)]
pub struct GuardSnapshot {
    /// Authority performing the operation
    pub authority_id: AuthorityId,
    /// Context for the operation
    pub context_id: ContextId,
    /// Current flow budget remaining
    pub flow_budget_remaining: FlowCost,
    /// Capabilities held by the authority
    pub capabilities: Vec<types::CapabilityId>,
    /// Current epoch
    pub epoch: u64,
}

impl types::CapabilitySnapshot for GuardSnapshot {
    fn has_capability(&self, cap: &types::CapabilityId) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

impl types::FlowBudgetSnapshot for GuardSnapshot {
    fn flow_budget_remaining(&self) -> FlowCost {
        self.flow_budget_remaining
    }
}

/// Request to be evaluated by guards
#[derive(Debug, Clone)]
pub enum GuardRequest {
    /// Publishing a descriptor to the journal
    PublishDescriptor { descriptor: RendezvousDescriptor },
    /// Establishing a channel with a peer
    EstablishChannel {
        peer: AuthorityId,
        transport: SelectedTransport,
    },
    /// Handling an incoming handshake
    IncomingHandshake {
        initiator: AuthorityId,
        handshake: NoiseHandshake,
    },
    /// Sending data on an established channel
    ChannelSend { peer: AuthorityId, size: usize },
}

/// Decision type shared across Layer 5 feature crates.
pub type GuardDecision = types::GuardDecision;

/// Effect command to be executed after guard approval
#[derive(Debug, Clone)]
pub enum EffectCommand {
    /// Append fact to journal
    JournalAppend { fact: RendezvousFact },
    /// Charge flow budget
    ChargeFlowBudget { cost: FlowCost },
    /// Send handshake message
    SendHandshake {
        peer: AuthorityId,
        message: HandshakeInit,
    },
    /// Send handshake response
    SendHandshakeResponse {
        peer: AuthorityId,
        message: HandshakeComplete,
    },
    /// Record receipt for operation
    RecordReceipt {
        operation: String,
        peer: AuthorityId,
    },
}

/// Outcome type shared across Layer 5 feature crates.
pub type GuardOutcome = types::GuardOutcome<EffectCommand>;

// =============================================================================
// Rendezvous Service
// =============================================================================

/// Cache key for descriptor storage
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DescriptorCacheKey {
    context_id: ContextId,
    authority_id: AuthorityId,
}

/// Rendezvous service coordinating peer discovery and channel establishment
pub struct RendezvousService {
    /// Local authority
    authority_id: AuthorityId,
    /// Service configuration
    config: RendezvousConfig,
    /// Transport selector for choosing transports
    transport_selector: TransportSelector,
    /// Descriptor builder
    descriptor_builder: DescriptorBuilder,
    /// Cached descriptors indexed by (context, authority)
    descriptor_cache: HashMap<DescriptorCacheKey, RendezvousDescriptor>,
    /// Active handshake state machines (waiting for response or processing init)
    /// Key: (ContextId, Peer)
    handshakers: RwLock<HashMap<(ContextId, AuthorityId), Handshaker>>,
}

impl RendezvousService {
    /// Create a new rendezvous service
    pub fn new(authority_id: AuthorityId, config: RendezvousConfig) -> Self {
        let transport_selector = TransportSelector::new(config.probe_timeout_ms);
        let descriptor_builder = DescriptorBuilder::new(
            authority_id,
            config.descriptor_validity_ms,
            config.stun_server.clone(),
        );

        Self {
            authority_id,
            config,
            transport_selector,
            descriptor_builder,
            descriptor_cache: HashMap::new(),
            handshakers: RwLock::new(HashMap::new()),
        }
    }

    /// Get the local authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the service configuration
    pub fn config(&self) -> &RendezvousConfig {
        &self.config
    }

    // =========================================================================
    // Descriptor Cache Access
    // =========================================================================

    /// Cache a descriptor received from the journal or network.
    pub fn cache_descriptor(&mut self, descriptor: RendezvousDescriptor) {
        let key = DescriptorCacheKey {
            context_id: descriptor.context_id,
            authority_id: descriptor.authority_id,
        };
        self.descriptor_cache.insert(key, descriptor);
    }

    /// Get a cached descriptor for a specific peer in a context.
    pub fn get_cached_descriptor(
        &self,
        context_id: ContextId,
        authority_id: AuthorityId,
        now_ms: u64,
    ) -> Option<&RendezvousDescriptor> {
        let key = DescriptorCacheKey {
            context_id,
            authority_id,
        };
        self.descriptor_cache
            .get(&key)
            .filter(|d| d.is_valid(now_ms))
    }

    /// Iterate over all cached descriptors in a context.
    pub fn iter_descriptors_in_context(
        &self,
        context_id: ContextId,
    ) -> impl Iterator<Item = &RendezvousDescriptor> {
        self.descriptor_cache
            .iter()
            .filter(move |(k, _)| k.context_id == context_id)
            .map(|(_, v)| v)
    }

    /// Get authorities whose descriptors need refresh in a context.
    pub fn peers_needing_refresh(&self, context_id: ContextId, now_ms: u64) -> Vec<AuthorityId> {
        self.descriptor_cache
            .iter()
            .filter(|(k, d)| {
                k.context_id == context_id
                    && k.authority_id != self.authority_id
                    && d.is_valid(now_ms)
                    && d.needs_refresh(now_ms)
            })
            .map(|(k, _)| k.authority_id)
            .collect()
    }

    /// Check if our own descriptor needs refresh in a context.
    pub fn needs_own_refresh(
        &self,
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        let key = DescriptorCacheKey {
            context_id,
            authority_id: self.authority_id,
        };

        match self.descriptor_cache.get(&key) {
            None => true, // No descriptor cached, need to publish
            Some(descriptor) => {
                if !descriptor.is_valid(now_ms) {
                    return true; // Descriptor expired
                }
                let time_until_expiry = descriptor.valid_until.saturating_sub(now_ms);
                time_until_expiry <= refresh_window_ms
            }
        }
    }

    /// Remove expired descriptors from the cache.
    pub fn evict_expired_descriptors(&mut self, now_ms: u64) {
        self.descriptor_cache.retain(|_, d| d.is_valid(now_ms));
    }

    /// Clear all cached descriptors for a context.
    pub fn clear_context_cache(&mut self, context_id: ContextId) {
        self.descriptor_cache
            .retain(|k, _| k.context_id != context_id);
    }

    // =========================================================================
    // Descriptor Publication
    // =========================================================================

    /// Prepare to publish a descriptor to the context journal.
    pub fn prepare_publish_descriptor(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        now_ms: u64,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = types::check_capability(
            snapshot,
            &types::CapabilityId::from(guards::CAP_RENDEZVOUS_PUBLISH),
        ) {
            return outcome;
        }

        // Check flow budget
        if let Some(outcome) = types::check_flow_budget(snapshot, guards::DESCRIPTOR_PUBLISH_COST) {
            return outcome;
        }

        // Build descriptor
        let descriptor = self
            .descriptor_builder
            .build(context_id, transport_hints, now_ms);

        // Create fact
        let fact = RendezvousFact::Descriptor(descriptor);

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: guards::DESCRIPTOR_PUBLISH_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::RecordReceipt {
                operation: "publish_descriptor".to_string(),
                peer: self.authority_id, // Self-operation
            },
        ];

        if let Err(reason) = types::validate_charge_before_send(
            &effects,
            |c| matches!(c, EffectCommand::ChargeFlowBudget { .. }),
            |c| {
                matches!(
                    c,
                    EffectCommand::SendHandshake { .. }
                        | EffectCommand::SendHandshakeResponse { .. }
                )
            },
        ) {
            return GuardOutcome::denied(reason);
        }

        GuardOutcome::allowed(effects)
    }

    /// Prepare to refresh an existing descriptor.
    pub fn prepare_refresh_descriptor(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        now_ms: u64,
    ) -> GuardOutcome {
        self.prepare_publish_descriptor(snapshot, context_id, transport_hints, now_ms)
    }

    // =========================================================================
    // Channel Establishment
    // =========================================================================

    /// Prepare to establish a channel with a peer.
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_establish_channel<E: NoiseEffects + CryptoEffects>(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        peer: AuthorityId,
        psk: &[u8; 32],
        local_private_key: &[u8], // Ed25519 seed
        remote_public_key: &[u8], // Ed25519 public
        now_ms: u64,
        peer_descriptor: &RendezvousDescriptor,
        effects: &E,
    ) -> AuraResult<GuardOutcome> {
        // Check capability
        if let Some(outcome) = types::check_capability(
            snapshot,
            &types::CapabilityId::from(guards::CAP_RENDEZVOUS_CONNECT),
        ) {
            return Ok(outcome);
        }

        // Check flow budget
        if let Some(outcome) = types::check_flow_budget(snapshot, guards::CONNECT_DIRECT_COST) {
            return Ok(outcome);
        }

        if peer_descriptor.context_id != context_id || peer_descriptor.authority_id != peer {
            return Err(AuraError::invalid(
                "Peer descriptor does not match context or peer",
            ));
        }
        if !peer_descriptor.is_valid(now_ms) {
            return Err(AuraError::invalid(
                "Peer descriptor is expired or not yet valid",
            ));
        }

        // Select transport
        let _transport = self.transport_selector.select(peer_descriptor)?;

        // Compute PSK commitment
        let psk_commitment = compute_psk_commitment(psk);

        // Initialize handshaker
        let handshake_config = HandshakeConfig {
            local: self.authority_id,
            remote: peer,
            context_id,
            psk: *psk,
            timeout_ms: self.config.probe_timeout_ms,
        };
        
        let mut handshaker = Handshaker::new(handshake_config);
        
        // Generate Noise Init message using NoiseEffects + CryptoEffects
        let noise_message = handshaker
            .create_init_message(
                snapshot.epoch,
                local_private_key,
                remote_public_key,
                effects
            )
            .await?;

        // Store handshaker
        let mut handshakers = self.handshakers.write().await;
        handshakers.insert((context_id, peer), handshaker);
        drop(handshakers);

        // Create handshake init message
        let handshake = NoiseHandshake {
            noise_message,
            psk_commitment,
            epoch: snapshot.epoch,
        };

        let init = HandshakeInit { handshake };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: guards::CONNECT_DIRECT_COST,
            },
            EffectCommand::SendHandshake {
                peer,
                message: init,
            },
            EffectCommand::RecordReceipt {
                operation: "establish_channel".to_string(),
                peer,
            },
        ];

        if let Err(reason) = types::validate_charge_before_send(
            &effects,
            |c| matches!(c, EffectCommand::ChargeFlowBudget { .. }),
            |c| {
                matches!(
                    c,
                    EffectCommand::SendHandshake { .. }
                        | EffectCommand::SendHandshakeResponse { .. }
                )
            },
        ) {
            return Ok(GuardOutcome::denied(reason));
        }

        Ok(GuardOutcome::allowed(effects))
    }

    // =========================================================================
    // Handshake Handling
    // =========================================================================

    /// Prepare to handle an incoming handshake as responder.
    pub async fn prepare_handle_handshake<E: NoiseEffects + CryptoEffects>(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        initiator: AuthorityId,
        init_message: NoiseHandshake,
        psk: &[u8; 32],
        local_private_key: &[u8], // Ed25519 seed
        effects: &E,
    ) -> AuraResult<(GuardOutcome, Option<SecureChannel>)> {
        // Check capability
        if let Some(outcome) = types::check_capability(
            snapshot,
            &types::CapabilityId::from(guards::CAP_RENDEZVOUS_CONNECT),
        ) {
            return Ok((outcome, None));
        }

        // Check flow budget
        if let Some(outcome) = types::check_flow_budget(snapshot, guards::CONNECT_DIRECT_COST) {
            return Ok((outcome, None));
        }

        // Verify PSK commitment
        let expected_commitment = compute_psk_commitment(psk);
        if init_message.psk_commitment != expected_commitment {
            return Ok((GuardOutcome::denied(types::GuardViolation::other("PSK commitment mismatch")), None));
        }

        // Initialize handshaker
        let handshake_config = HandshakeConfig {
            local: self.authority_id,
            remote: initiator,
            context_id,
            psk: *psk,
            timeout_ms: self.config.probe_timeout_ms,
        };
        
        let mut handshaker = Handshaker::new(handshake_config);
        
        // Process Init message
        handshaker
            .process_init(
                &init_message.noise_message,
                init_message.epoch,
                local_private_key,
                effects
            )
            .await?;
        
        // Create Response message
        let response_bytes = handshaker.create_response(snapshot.epoch, effects).await?;
        
        // Complete handshake (Responder side)
        let (result, channel) = handshaker.complete(snapshot.epoch, false, effects).await?;
        let channel_id = result.channel_id;

        // Create response handshake
        let response_handshake = NoiseHandshake {
            noise_message: response_bytes,
            psk_commitment: expected_commitment,
            epoch: snapshot.epoch,
        };

        let complete = HandshakeComplete {
            handshake: response_handshake,
            channel_id,
        };

        // Create channel established fact
        let fact = RendezvousFact::ChannelEstablished {
            initiator,
            responder: self.authority_id,
            channel_id,
            epoch: snapshot.epoch,
        };

        // Construct effect commands
        let effects = vec![
            EffectCommand::ChargeFlowBudget {
                cost: guards::CONNECT_DIRECT_COST,
            },
            EffectCommand::JournalAppend { fact },
            EffectCommand::SendHandshakeResponse {
                peer: initiator,
                message: complete,
            },
            EffectCommand::RecordReceipt {
                operation: "handle_handshake".to_string(),
                peer: initiator,
            },
        ];

        if let Err(reason) = types::validate_charge_before_send(
            &effects,
            |c| matches!(c, EffectCommand::ChargeFlowBudget { .. }),
            |c| {
                matches!(
                    c,
                    EffectCommand::SendHandshake { .. }
                        | EffectCommand::SendHandshakeResponse { .. }
                )
            },
        ) {
            return Ok((GuardOutcome::denied(reason), None));
        }

        Ok((GuardOutcome::allowed(effects), Some(channel)))
    }
    
    /// Prepare to handle handshake completion (Initiator side).
    pub async fn prepare_handle_completion<E: NoiseEffects>(
        &self,
        _snapshot: &GuardSnapshot, // Guard check assumed done for initial request
        context_id: ContextId,
        peer: AuthorityId,
        completion_message: HandshakeComplete,
        effects: &E,
    ) -> AuraResult<Option<SecureChannel>> {
        let mut handshakers = self.handshakers.write().await;
        let mut handshaker = handshakers.remove(&(context_id, peer))
            .ok_or_else(|| AuraError::invalid("No pending handshake found for peer"))?;
        drop(handshakers);
            
        // Process Response message
        handshaker.process_response(&completion_message.handshake.noise_message, effects).await?;
        
        // Complete handshake (Initiator side)
        let (_result, channel) = handshaker.complete(completion_message.handshake.epoch, true, effects).await?;
        
        // Verify channel ID matches if provided in message (optional check)
        if completion_message.channel_id != channel.channel_id() {
             return Err(AuraError::crypto("Channel ID mismatch in completion"));
        }
        
        Ok(Some(channel))
    }

    /// Create a channel established fact
    pub fn create_channel_established_fact(
        &self,
        _context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    ) -> RendezvousFact {
        RendezvousFact::ChannelEstablished {
            initiator: self.authority_id,
            responder: peer,
            channel_id,
            epoch,
        }
    }

    /// Prepare a relay request (placeholder for Phase 2+)
    pub fn prepare_relay_request(
        &self,
        _context_id: ContextId,
        _relay: AuthorityId,
        _target: AuthorityId,
        snapshot: &GuardSnapshot,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = types::check_capability(
            snapshot,
            &types::CapabilityId::from(guards::CAP_RENDEZVOUS_RELAY),
        ) {
            return outcome;
        }

        // Relay support will be added in Phase 2+
        GuardOutcome::denied(types::GuardViolation::other(
            "Relay support not yet implemented",
        ))
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Compute PSK commitment (hash of PSK)
fn compute_psk_commitment(psk: &[u8; 32]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(psk);
    let result = hasher.finalize();
    let mut commitment = [0u8; 32];
    commitment.copy_from_slice(&result);
    commitment
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::noise::{HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState};
    use aura_core::effects::{CryptoEffects, CryptoCoreEffects, CryptoExtendedEffects, CryptoError, RandomCoreEffects};
    use async_trait::async_trait;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([2u8; 32])
    }

    fn test_snapshot() -> GuardSnapshot {
        GuardSnapshot {
            authority_id: test_authority(),
            context_id: test_context(),
            flow_budget_remaining: FlowCost::new(100),
            capabilities: vec![
                types::CapabilityId::from(guards::CAP_RENDEZVOUS_PUBLISH),
                types::CapabilityId::from(guards::CAP_RENDEZVOUS_CONNECT),
            ],
            epoch: 1,
        }
    }
    
    // Mock Noise Effects for testing service
    struct MockNoise;
    #[async_trait]
    impl NoiseEffects for MockNoise {
        async fn create_handshake_state(&self, _params: NoiseParams) -> Result<HandshakeState, NoiseError> {
            Ok(HandshakeState(Box::new(())))
        }
        async fn write_message(&self, _state: HandshakeState, _payload: &[u8]) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![1, 2, 3], HandshakeState(Box::new(()))))
        }
        async fn read_message(&self, _state: HandshakeState, _message: &[u8]) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![], HandshakeState(Box::new(()))))
        }
        async fn into_transport_mode(&self, _state: HandshakeState) -> Result<TransportState, NoiseError> {
            Ok(TransportState(Box::new(())))
        }
        async fn encrypt_transport_message(&self, _state: &mut TransportState, payload: &[u8]) -> Result<Vec<u8>, NoiseError> {
            Ok(payload.to_vec())
        }
        async fn decrypt_transport_message(&self, _state: &mut TransportState, message: &[u8]) -> Result<Vec<u8>, NoiseError> {
            Ok(message.to_vec())
        }
    }
    
    // Add Mock Crypto Effects
    #[async_trait]
    impl RandomCoreEffects for MockNoise {
        async fn random_bytes(&self, _len: usize) -> Vec<u8> { vec![] }
        async fn random_bytes_32(&self) -> [u8; 32] { [0u8; 32] }
        async fn random_u64(&self) -> u64 { 0 }
        async fn random_range(&self, _min: u64, _max: u64) -> u64 { 0 }
        async fn random_uuid(&self) -> uuid::Uuid { uuid::Uuid::nil() }
    }
    #[async_trait]
    impl CryptoCoreEffects for MockNoise {
        async fn hkdf_derive(&self, _: &[u8], _: &[u8], _: &[u8], _: u32) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn derive_key(&self, _: &[u8], _: &aura_core::effects::crypto::KeyDerivationContext) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> { Ok((vec![], vec![])) }
        async fn ed25519_sign(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CryptoError> { Ok(vec![]) }
        async fn ed25519_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> { Ok(true) }
        fn is_simulated(&self) -> bool { true }
        fn crypto_capabilities(&self) -> Vec<String> { vec![] }
        fn constant_time_eq(&self, _: &[u8], _: &[u8]) -> bool { true }
        fn secure_zero(&self, _: &mut [u8]) {}
    }
    #[async_trait]
    impl CryptoExtendedEffects for MockNoise {
        async fn convert_ed25519_to_x25519_public(&self, _: &[u8]) -> Result<[u8; 32], CryptoError> { Ok([0u8; 32]) }
        async fn convert_ed25519_to_x25519_private(&self, _: &[u8]) -> Result<[u8; 32], CryptoError> { Ok([0u8; 32]) }
    }
    impl CryptoEffects for MockNoise {}

    #[test]
    fn test_service_creation() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        assert_eq!(service.authority_id(), test_authority());
    }

    #[test]
    fn test_prepare_publish_descriptor_success() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let snapshot = test_snapshot();

        let outcome = service.prepare_publish_descriptor(
            &snapshot,
            test_context(),
            vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            1000,
        );

        assert!(outcome.decision.is_allowed());
        assert_eq!(outcome.effects.len(), 3);
    }

    #[tokio::test]
    async fn test_prepare_establish_channel() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let snapshot = test_snapshot();
        let peer = AuthorityId::new_from_entropy([3u8; 32]);
        let psk = [42u8; 32];
        let mock_effects = MockNoise;
        
        let descriptor = RendezvousDescriptor {
            authority_id: peer,
            context_id: test_context(),
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };

        let outcome = service.prepare_establish_channel(
            &snapshot,
            test_context(),
            peer,
            &psk,
            &[0u8; 32], // local private key
            &[0u8; 32], // remote public key
            100,
            &descriptor,
            &mock_effects
        ).await.unwrap();

        assert!(outcome.decision.is_allowed());
        
        // Check if handshaker was stored
        let handshakers = service.handshakers.read().await;
        assert!(handshakers.contains_key(&(test_context(), peer)));
    }
}

//! Rendezvous Service
//!
//! Main coordinator for peer discovery and channel establishment.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.

use crate::capabilities::RendezvousCapability;
use crate::descriptor::DescriptorBuilder;
use crate::facts::{RendezvousDescriptor, RendezvousFact, TransportHint};
use crate::new_channel::{HandshakeConfig, Handshaker, SecureChannel};
use crate::protocol::{guards, HandshakeComplete, HandshakeInit, NoiseHandshake};
use aura_core::effects::noise::NoiseEffects;
use aura_core::effects::CryptoEffects;
use aura_core::service::EstablishPath;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_core::{AuraError, AuraResult};
use aura_guards::types;
use std::collections::HashMap;
use std::time::{Duration, Instant as MonotonicClock};
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
    /// Time-to-live for pending initiator handshakes in the local registry
    pub pending_handshaker_ttl_ms: u64,
    /// Maximum number of pending initiator handshakes retained locally
    pub max_pending_handshakers: usize,
}

impl Default for RendezvousConfig {
    fn default() -> Self {
        Self {
            descriptor_validity_ms: 3_600_000, // 1 hour
            stun_server: None,
            probe_timeout_ms: 5000, // 5 seconds
            max_relay_hops: 3,
            pending_handshaker_ttl_ms: 60_000, // 60 seconds
            max_pending_handshakers: 256,
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
        path: EstablishPath,
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

/// Rendezvous service coordinating peer discovery and channel establishment
#[aura_macros::service_surface(
    families = "Establish",
    object_categories = "authoritative_shared,transport_protocol,runtime_derived_local,proof_accounting",
    discover = "journal_descriptor_publication",
    permit = "runtime_capability_and_budget_checks",
    transfer = "rendezvous_handler_and_transport_effects",
    select = "rendezvous_manager_runtime_cache",
    authoritative = "RendezvousDescriptor,RendezvousFact::ChannelEstablished",
    runtime_local = "descriptor_snapshot,selected_establish_path,retry_budget,handshake_registry",
    category = "service_surface"
)]
pub struct RendezvousService {
    /// Local authority
    authority_id: AuthorityId,
    /// Service configuration
    config: RendezvousConfig,
    /// Descriptor builder
    descriptor_builder: DescriptorBuilder,
    /// Active handshake state machines (waiting for response or processing init)
    /// Key: (ContextId, Peer)
    handshakers: RwLock<HashMap<(ContextId, AuthorityId), PendingHandshaker>>,
}

#[derive(Debug)]
struct PendingHandshaker {
    handshaker: Handshaker,
    inserted_at: MonotonicClock,
}

impl PendingHandshaker {
    fn new(handshaker: Handshaker) -> Self {
        Self {
            handshaker,
            inserted_at: MonotonicClock::now(),
        }
    }

    fn is_expired(&self, now: MonotonicClock, ttl: Duration) -> bool {
        now.duration_since(self.inserted_at) >= ttl
    }
}

impl RendezvousService {
    fn pending_handshaker_ttl(&self) -> Duration {
        Duration::from_millis(self.config.pending_handshaker_ttl_ms)
    }

    fn cleanup_expired_handshakers(
        &self,
        handshakers: &mut HashMap<(ContextId, AuthorityId), PendingHandshaker>,
        now: MonotonicClock,
    ) -> usize {
        let ttl = self.pending_handshaker_ttl();
        let before = handshakers.len();
        handshakers.retain(|_, pending| !pending.is_expired(now, ttl));
        before.saturating_sub(handshakers.len())
    }

    fn check_capability_and_budget(
        snapshot: &GuardSnapshot,
        capability: RendezvousCapability,
        required_cost: FlowCost,
    ) -> Option<GuardOutcome> {
        if let Some(outcome) = types::check_capability(snapshot, &capability.as_name()) {
            return Some(outcome);
        }

        types::check_flow_budget(snapshot, required_cost)
    }

    fn record_receipt(operation: &str, peer: AuthorityId) -> EffectCommand {
        EffectCommand::RecordReceipt {
            operation: operation.to_string(),
            peer,
        }
    }

    fn is_charge_effect(command: &EffectCommand) -> bool {
        matches!(command, EffectCommand::ChargeFlowBudget { .. })
    }

    fn is_send_effect(command: &EffectCommand) -> bool {
        matches!(
            command,
            EffectCommand::SendHandshake { .. } | EffectCommand::SendHandshakeResponse { .. }
        )
    }

    fn finalize_effects_with_charge(
        required_cost: FlowCost,
        mut effects: Vec<EffectCommand>,
    ) -> GuardOutcome {
        effects.insert(
            0,
            EffectCommand::ChargeFlowBudget {
                cost: required_cost,
            },
        );

        if let Err(reason) = types::validate_charge_before_send(
            &effects,
            Self::is_charge_effect,
            Self::is_send_effect,
        ) {
            return GuardOutcome::denied(reason);
        }

        GuardOutcome::allowed(effects)
    }

    /// Create a new rendezvous service
    pub fn new(authority_id: AuthorityId, config: RendezvousConfig) -> Self {
        let descriptor_builder = DescriptorBuilder::new(
            authority_id,
            config.descriptor_validity_ms,
            config.stun_server.clone(),
        );

        Self {
            authority_id,
            config,
            descriptor_builder,
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
    // Descriptor Publication
    // =========================================================================

    /// Prepare to publish a descriptor to the context journal.
    pub fn prepare_publish_descriptor(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        public_key: [u8; 32],
        now_ms: u64,
    ) -> GuardOutcome {
        if let Some(outcome) = Self::check_capability_and_budget(
            snapshot,
            RendezvousCapability::Publish,
            guards::DESCRIPTOR_PUBLISH_COST,
        ) {
            return outcome;
        }

        let descriptor =
            self.descriptor_builder
                .build(context_id, transport_hints, public_key, now_ms);
        let fact = RendezvousFact::Descriptor(descriptor);

        let effects = vec![
            EffectCommand::JournalAppend { fact },
            Self::record_receipt("publish_descriptor", self.authority_id),
        ];

        Self::finalize_effects_with_charge(guards::DESCRIPTOR_PUBLISH_COST, effects)
    }

    /// Prepare to refresh an existing descriptor.
    pub fn prepare_refresh_descriptor(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        public_key: [u8; 32],
        now_ms: u64,
    ) -> GuardOutcome {
        self.prepare_publish_descriptor(snapshot, context_id, transport_hints, public_key, now_ms)
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
        path: &EstablishPath,
        psk: &[u8; 32],
        local_private_key: &[u8], // Ed25519 seed
        remote_public_key: &[u8], // Ed25519 public
        now_ms: u64,
        peer_descriptor: &RendezvousDescriptor,
        effects: &E,
    ) -> AuraResult<GuardOutcome> {
        if let Some(outcome) = Self::check_capability_and_budget(
            snapshot,
            RendezvousCapability::Connect,
            guards::CONNECT_DIRECT_COST,
        ) {
            return Ok(outcome);
        }

        if snapshot.context_id != context_id {
            return Err(AuraError::invalid(
                "Guard snapshot context does not match channel context",
            ));
        }
        if peer_descriptor.authority_id != peer {
            return Err(AuraError::invalid("Peer descriptor does not match peer"));
        }
        if peer_descriptor.context_id != context_id {
            return Err(AuraError::invalid(
                "Peer descriptor context does not match channel context",
            ));
        }
        if !peer_descriptor.is_valid(now_ms) {
            return Err(AuraError::invalid(
                "Peer descriptor is expired or not yet valid",
            ));
        }

        // Select transport
        if !peer_descriptor
            .advertised_establish_paths()
            .iter()
            .any(|candidate| candidate == path)
        {
            return Err(AuraError::invalid(
                "Establish path is not advertised by the peer descriptor",
            ));
        }

        // Compute PSK commitment
        let psk_commitment = compute_psk_commitment(psk);
        if peer_descriptor.handshake_psk_commitment != psk_commitment {
            return Err(AuraError::invalid(
                "Peer descriptor PSK commitment does not match channel PSK",
            ));
        }

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
                effects,
            )
            .await?;

        // Store handshaker
        let mut handshakers = self.handshakers.write().await;
        let expired = self.cleanup_expired_handshakers(&mut handshakers, MonotonicClock::now());
        if expired > 0 {
            tracing::warn!(
                authority_id = %self.authority_id,
                expired,
                remaining = handshakers.len(),
                "Expired pending rendezvous handshakers were removed before inserting a new one"
            );
        }
        let key = (context_id, peer);
        let replacing_existing = handshakers.contains_key(&key);
        if !replacing_existing && handshakers.len() >= self.config.max_pending_handshakers {
            return Err(AuraError::invalid(format!(
                "Too many pending handshakes: {} >= {}",
                handshakers.len(),
                self.config.max_pending_handshakers
            )));
        }
        handshakers.insert(key, PendingHandshaker::new(handshaker));
        drop(handshakers);

        // Create handshake init message
        let handshake = NoiseHandshake {
            noise_message,
            psk_commitment,
            epoch: snapshot.epoch,
        };

        let init = HandshakeInit { handshake };

        let effects = vec![
            EffectCommand::SendHandshake {
                peer,
                message: init,
            },
            Self::record_receipt("establish_channel", peer),
        ];

        Ok(Self::finalize_effects_with_charge(
            guards::CONNECT_DIRECT_COST,
            effects,
        ))
    }

    // =========================================================================
    // Handshake Handling
    // =========================================================================

    /// Prepare to handle an incoming handshake as responder.
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_handle_handshake<E: NoiseEffects + CryptoEffects>(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        initiator: AuthorityId,
        init_message: NoiseHandshake,
        psk: &[u8; 32],
        local_private_key: &[u8],    // Ed25519 seed
        initiator_public_key: &[u8], // Ed25519 public key from initiator descriptor
        effects: &E,
    ) -> AuraResult<(GuardOutcome, Option<SecureChannel>)> {
        if let Some(outcome) = Self::check_capability_and_budget(
            snapshot,
            RendezvousCapability::Connect,
            guards::CONNECT_DIRECT_COST,
        ) {
            return Ok((outcome, None));
        }

        let expected_commitment = compute_psk_commitment(psk);
        if init_message.psk_commitment != expected_commitment {
            return Ok((
                GuardOutcome::denied(types::GuardViolation::other("PSK commitment mismatch")),
                None,
            ));
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
                initiator_public_key,
                effects,
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

        let effects = vec![
            EffectCommand::JournalAppend { fact },
            EffectCommand::SendHandshakeResponse {
                peer: initiator,
                message: complete,
            },
            Self::record_receipt("handle_handshake", initiator),
        ];

        Ok((
            Self::finalize_effects_with_charge(guards::CONNECT_DIRECT_COST, effects),
            Some(channel),
        ))
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
        let expired = self.cleanup_expired_handshakers(&mut handshakers, MonotonicClock::now());
        if expired > 0 {
            tracing::warn!(
                authority_id = %self.authority_id,
                expired,
                remaining = handshakers.len(),
                "Expired pending rendezvous handshakers were removed before handling completion"
            );
        }
        let pending = handshakers
            .remove(&(context_id, peer))
            .ok_or_else(|| AuraError::invalid("No pending handshake found for peer"))?;
        drop(handshakers);
        let mut handshaker = pending.handshaker;

        // Process Response message
        handshaker
            .process_response(&completion_message.handshake.noise_message, effects)
            .await?;

        // Complete handshake (Initiator side)
        let (_result, channel) = handshaker
            .complete(completion_message.handshake.epoch, true, effects)
            .await?;

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

    /// Prepare a relay request using the same capability and hop-budget checks
    /// as direct rendezvous establishment.
    pub fn prepare_relay_request(
        &self,
        _context_id: ContextId,
        relay: AuthorityId,
        target: AuthorityId,
        snapshot: &GuardSnapshot,
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) =
            types::check_capability(snapshot, &RendezvousCapability::Relay.as_name())
        {
            return outcome;
        }

        if relay == target {
            return GuardOutcome::denied(types::GuardViolation::other(
                "relay authority must differ from relay target",
            ));
        }

        if self.config.max_relay_hops == 0 {
            return GuardOutcome::denied(types::GuardViolation::other(
                "relay requests are disabled by max_relay_hops=0",
            ));
        }

        GuardOutcome::allowed(Vec::new())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Compute PSK commitment (hash of PSK)
fn compute_psk_commitment(psk: &[u8; 32]) -> [u8; 32] {
    aura_core::hash::hash(psk)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::noise::{
        HandshakeState, NoiseEffects, NoiseError, NoiseParams, TransportState,
    };
    use aura_core::effects::{
        CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
    };

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
                RendezvousCapability::Publish.as_name(),
                RendezvousCapability::Connect.as_name(),
            ],
            epoch: 1,
        }
    }

    // Mock Noise Effects for testing service
    struct MockNoise;
    #[async_trait]
    impl NoiseEffects for MockNoise {
        async fn create_handshake_state(
            &self,
            _params: NoiseParams,
        ) -> Result<HandshakeState, NoiseError> {
            Ok(HandshakeState(Box::new(())))
        }
        async fn write_message(
            &self,
            _state: HandshakeState,
            _payload: &[u8],
        ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![1, 2, 3], HandshakeState(Box::new(()))))
        }
        async fn read_message(
            &self,
            _state: HandshakeState,
            _message: &[u8],
        ) -> Result<(Vec<u8>, HandshakeState), NoiseError> {
            Ok((vec![], HandshakeState(Box::new(()))))
        }
        async fn into_transport_mode(
            &self,
            _state: HandshakeState,
        ) -> Result<TransportState, NoiseError> {
            Ok(TransportState(Box::new(())))
        }
        async fn encrypt_transport_message(
            &self,
            _state: &mut TransportState,
            payload: &[u8],
        ) -> Result<Vec<u8>, NoiseError> {
            Ok(payload.to_vec())
        }
        async fn decrypt_transport_message(
            &self,
            _state: &mut TransportState,
            message: &[u8],
        ) -> Result<Vec<u8>, NoiseError> {
            Ok(message.to_vec())
        }
    }

    // Add Mock Crypto Effects
    #[async_trait]
    impl RandomCoreEffects for MockNoise {
        async fn random_bytes(&self, _len: usize) -> Vec<u8> {
            vec![]
        }
        async fn random_bytes_32(&self) -> [u8; 32] {
            [0u8; 32]
        }
        async fn random_u64(&self) -> u64 {
            0
        }
    }
    #[async_trait]
    impl CryptoCoreEffects for MockNoise {
        async fn kdf_derive(
            &self,
            _: &[u8],
            _: &[u8],
            _: &[u8],
            _: u32,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn derive_key(
            &self,
            _: &[u8],
            _: &aura_core::effects::crypto::KeyDerivationContext,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            Ok((vec![], vec![]))
        }
        async fn ed25519_sign(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }
        async fn ed25519_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> {
            Ok(true)
        }
        fn is_simulated(&self) -> bool {
            true
        }
        fn crypto_capabilities(&self) -> Vec<String> {
            vec![]
        }
        fn constant_time_eq(&self, _: &[u8], _: &[u8]) -> bool {
            true
        }
        fn secure_zero(&self, _: &mut [u8]) {}
    }
    #[async_trait]
    impl CryptoExtendedEffects for MockNoise {
        async fn convert_ed25519_to_x25519_public(
            &self,
            _: &[u8],
        ) -> Result<[u8; 32], CryptoError> {
            Ok([0u8; 32])
        }
        async fn convert_ed25519_to_x25519_private(
            &self,
            _: &[u8],
        ) -> Result<[u8; 32], CryptoError> {
            Ok([0u8; 32])
        }
    }
    // Note: CryptoEffects has a blanket impl, so we don't need to impl it explicitly

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
            [0u8; 32],
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
            device_id: None,
            context_id: test_context(),
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: compute_psk_commitment(&psk),
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };
        let establish_path = descriptor
            .advertised_establish_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("establish path"));

        let outcome = service
            .prepare_establish_channel(
                &snapshot,
                test_context(),
                peer,
                &establish_path,
                &psk,
                &[0u8; 32], // local private key
                &[0u8; 32], // remote public key
                100,
                &descriptor,
                &mock_effects,
            )
            .await
            .unwrap();

        assert!(outcome.decision.is_allowed());

        // Check if handshaker was stored
        let handshakers = service.handshakers.read().await;
        assert!(handshakers.contains_key(&(test_context(), peer)));
    }

    #[tokio::test]
    async fn test_expired_pending_handshakers_are_swept_before_insert() {
        let config = RendezvousConfig {
            pending_handshaker_ttl_ms: 10,
            ..RendezvousConfig::default()
        };
        let service = RendezvousService::new(test_authority(), config);
        let peer = AuthorityId::new_from_entropy([4u8; 32]);

        let expired_config = HandshakeConfig {
            local: test_authority(),
            remote: peer,
            context_id: test_context(),
            psk: [7u8; 32],
            timeout_ms: 5,
        };

        {
            let mut handshakers = service.handshakers.write().await;
            handshakers.insert(
                (test_context(), peer),
                PendingHandshaker {
                    handshaker: Handshaker::new(expired_config),
                    inserted_at: MonotonicClock::now() - Duration::from_millis(25),
                },
            );
            let removed =
                service.cleanup_expired_handshakers(&mut handshakers, MonotonicClock::now());
            assert_eq!(removed, 1);
            assert!(handshakers.is_empty());
        }
    }

    #[tokio::test]
    async fn test_pending_handshaker_capacity_rejects_new_entries() {
        let config = RendezvousConfig {
            max_pending_handshakers: 1,
            pending_handshaker_ttl_ms: 60_000,
            ..RendezvousConfig::default()
        };
        let service = RendezvousService::new(test_authority(), config);
        let snapshot = test_snapshot();
        let peer_a = AuthorityId::new_from_entropy([5u8; 32]);
        let peer_b = AuthorityId::new_from_entropy([6u8; 32]);
        let psk = [42u8; 32];
        let mock_effects = MockNoise;

        {
            let mut handshakers = service.handshakers.write().await;
            handshakers.insert(
                (test_context(), peer_a),
                PendingHandshaker::new(Handshaker::new(HandshakeConfig {
                    local: test_authority(),
                    remote: peer_a,
                    context_id: test_context(),
                    psk,
                    timeout_ms: 5,
                })),
            );
        }

        let descriptor = RendezvousDescriptor {
            authority_id: peer_b,
            device_id: None,
            context_id: test_context(),
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: compute_psk_commitment(&psk),
            public_key: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        };
        let establish_path = descriptor
            .advertised_establish_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("establish path"));

        let error = match service
            .prepare_establish_channel(
                &snapshot,
                test_context(),
                peer_b,
                &establish_path,
                &psk,
                &[0u8; 32],
                &[0u8; 32],
                100,
                &descriptor,
                &mock_effects,
            )
            .await
        {
            Ok(_) => panic!("capacity limit should reject new pending handshake"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("Too many pending handshakes"));
    }
}

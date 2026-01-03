//! Rendezvous Service
//!
//! Main coordinator for peer discovery and channel establishment.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.

use crate::descriptor::{DescriptorBuilder, SelectedTransport, TransportSelector};
use crate::facts::{RendezvousDescriptor, RendezvousFact, TransportHint};
use crate::protocol::{guards, HandshakeComplete, HandshakeInit, NoiseHandshake};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_core::{AuraError, AuraResult};
use aura_guards::types;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Convert an AuthorityId to a 32-byte hash for commitment/indexing purposes.
fn authority_hash_bytes(authority: &AuthorityId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(authority.to_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

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
    ///
    /// This stores the descriptor for later lookup by `get_cached_descriptor`.
    /// Expired descriptors are automatically replaced when a newer one is cached.
    pub fn cache_descriptor(&mut self, descriptor: RendezvousDescriptor) {
        let key = DescriptorCacheKey {
            context_id: descriptor.context_id,
            authority_id: descriptor.authority_id,
        };
        self.descriptor_cache.insert(key, descriptor);
    }

    /// Get a cached descriptor for a specific peer in a context.
    ///
    /// Returns `None` if no descriptor is cached or if the cached descriptor
    /// has expired (based on `now_ms`).
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
    ///
    /// Returns descriptors regardless of validity; caller should check `is_valid()`.
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
    ///
    /// Returns authorities with descriptors that are still valid but within
    /// their refresh window (10% of expiry).
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
    ///
    /// # Arguments
    /// * `context_id` - The context to check
    /// * `now_ms` - Current time in milliseconds
    /// * `refresh_window_ms` - Time before expiry to trigger refresh
    ///
    /// Returns `true` if we have no cached descriptor, it's expired, or it's
    /// within the refresh window.
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
                // Check if within custom refresh window
                let time_until_expiry = descriptor.valid_until.saturating_sub(now_ms);
                time_until_expiry <= refresh_window_ms
            }
        }
    }

    /// Remove expired descriptors from the cache.
    ///
    /// Call periodically to prevent unbounded cache growth.
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
    ///
    /// Returns a `GuardOutcome` that the caller must evaluate and execute.
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
    ///
    /// Similar to publish but used for refreshing before expiry.
    pub fn prepare_refresh_descriptor(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        now_ms: u64,
    ) -> GuardOutcome {
        // Refresh uses the same logic as publish
        self.prepare_publish_descriptor(snapshot, context_id, transport_hints, now_ms)
    }

    // =========================================================================
    // Channel Establishment
    // =========================================================================

    /// Prepare to establish a channel with a peer.
    ///
    /// This selects a transport based on the provided descriptor and prepares
    /// the handshake initiation.
    pub fn prepare_establish_channel(
        &self,
        snapshot: &GuardSnapshot,
        context_id: ContextId,
        peer: AuthorityId,
        psk: &[u8; 32],
        now_ms: u64,
        peer_descriptor: &RendezvousDescriptor,
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

        // Create handshake init message
        let handshake = NoiseHandshake {
            noise_message: vec![], // Placeholder - actual Noise message created at execution
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
    ///
    /// Returns a `GuardOutcome` with the handshake response.
    pub fn prepare_handle_handshake(
        &self,
        snapshot: &GuardSnapshot,
        _context_id: ContextId,
        initiator: AuthorityId,
        init_message: NoiseHandshake,
        psk: &[u8; 32],
    ) -> GuardOutcome {
        // Check capability
        if let Some(outcome) = types::check_capability(
            snapshot,
            &types::CapabilityId::from(guards::CAP_RENDEZVOUS_CONNECT),
        ) {
            return outcome;
        }

        // Check flow budget
        if let Some(outcome) = types::check_flow_budget(snapshot, guards::CONNECT_DIRECT_COST) {
            return outcome;
        }

        // Verify PSK commitment
        let expected_commitment = compute_psk_commitment(psk);
        if init_message.psk_commitment != expected_commitment {
            return GuardOutcome::denied(types::GuardViolation::other("PSK commitment mismatch"));
        }

        // Generate channel ID
        let channel_id = generate_channel_id(&self.authority_id, &initiator, snapshot.epoch);

        // Create response handshake
        let response_handshake = NoiseHandshake {
            noise_message: vec![], // Placeholder - actual Noise message created at execution
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
            return GuardOutcome::denied(reason);
        }

        GuardOutcome::allowed(effects)
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
    ///
    /// This will be fully implemented when relay support is added.
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

/// Generate deterministic channel ID from participants and epoch
fn generate_channel_id(
    authority_a: &AuthorityId,
    authority_b: &AuthorityId,
    epoch: u64,
) -> [u8; 32] {
    let mut hasher = Sha256::new();

    // Sort authorities for determinism
    let hash_a = authority_hash_bytes(authority_a);
    let hash_b = authority_hash_bytes(authority_b);
    let (first, second) = if hash_a < hash_b {
        (hash_a, hash_b)
    } else {
        (hash_b, hash_a)
    };

    hasher.update(first);
    hasher.update(second);
    hasher.update(epoch.to_le_bytes());

    let result = hasher.finalize();
    let mut channel_id = [0u8; 32];
    channel_id.copy_from_slice(&result);
    channel_id
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_prepare_publish_descriptor_missing_capability() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.capabilities.clear();

        let outcome = service.prepare_publish_descriptor(
            &snapshot,
            test_context(),
            vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            1000,
        );

        assert!(outcome.decision.is_denied());
    }

    #[test]
    fn test_prepare_publish_descriptor_insufficient_budget() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let mut snapshot = test_snapshot();
        snapshot.flow_budget_remaining = FlowCost::new(0);

        let outcome = service.prepare_publish_descriptor(
            &snapshot,
            test_context(),
            vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            1000,
        );

        assert!(outcome.decision.is_denied());
    }

    #[test]
    fn test_psk_commitment() {
        let psk = [42u8; 32];
        let commitment = compute_psk_commitment(&psk);

        // Same PSK should produce same commitment
        let commitment2 = compute_psk_commitment(&psk);
        assert_eq!(commitment, commitment2);

        // Different PSK should produce different commitment
        let different_psk = [43u8; 32];
        let different_commitment = compute_psk_commitment(&different_psk);
        assert_ne!(commitment, different_commitment);
    }

    #[test]
    fn test_channel_id_generation() {
        let auth_a = AuthorityId::new_from_entropy([1u8; 32]);
        let auth_b = AuthorityId::new_from_entropy([2u8; 32]);

        let id1 = generate_channel_id(&auth_a, &auth_b, 1);
        let id2 = generate_channel_id(&auth_b, &auth_a, 1);

        // Order shouldn't matter
        assert_eq!(id1, id2);

        // Different epoch should produce different ID
        let id3 = generate_channel_id(&auth_a, &auth_b, 2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_prepare_handle_handshake_success() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let snapshot = test_snapshot();
        let initiator = AuthorityId::new_from_entropy([3u8; 32]);
        let psk = [42u8; 32];
        let psk_commitment = compute_psk_commitment(&psk);

        let init_message = NoiseHandshake {
            noise_message: vec![1, 2, 3],
            psk_commitment,
            epoch: 1,
        };

        let outcome = service.prepare_handle_handshake(
            &snapshot,
            test_context(),
            initiator,
            init_message,
            &psk,
        );

        assert!(outcome.decision.is_allowed());
        assert_eq!(outcome.effects.len(), 4);
    }

    #[test]
    fn test_prepare_handle_handshake_psk_mismatch() {
        let service = RendezvousService::new(test_authority(), RendezvousConfig::default());
        let snapshot = test_snapshot();
        let initiator = AuthorityId::new_from_entropy([3u8; 32]);
        let psk = [42u8; 32];
        let wrong_commitment = [0u8; 32]; // Wrong commitment

        let init_message = NoiseHandshake {
            noise_message: vec![1, 2, 3],
            psk_commitment: wrong_commitment,
            epoch: 1,
        };

        let outcome = service.prepare_handle_handshake(
            &snapshot,
            test_context(),
            initiator,
            init_message,
            &psk,
        );

        assert!(outcome.decision.is_denied());
    }
}

//! Secure Channel Wrapper
//!
//! Context-bound secure channels with epoch-based key rotation.
//! This module provides the SecureChannel type that integrates with
//! the guard chain for authorized communication.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{AuraError, AuraResult};
use serde::{Deserialize, Serialize};
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
// Secure Channel
// =============================================================================

/// Context-bound secure channel between two authorities
///
/// Provides encrypted communication with:
/// - Context isolation (each channel is bound to a specific context)
/// - Epoch-based key rotation
/// - Guard chain integration for authorization
#[derive(Debug)]
pub struct SecureChannel {
    /// Unique channel identifier
    channel_id: [u8; 32],
    /// Context this channel belongs to
    context_id: ContextId,
    /// Local authority
    local: AuthorityId,
    /// Remote peer
    remote: AuthorityId,
    /// Current epoch (for key rotation)
    epoch: u64,
    /// Channel state
    state: ChannelState,
    /// Agreement mode (A1/A2/A3) for the channel lifecycle
    agreement_mode: AgreementMode,
    /// Whether reversion is still possible
    reversion_risk: bool,
    /// Whether the channel needs key rotation
    needs_rotation: bool,
    /// Bytes sent on this channel (for flow budget tracking)
    bytes_sent: u64,
    /// Bytes received on this channel
    bytes_received: u64,
}

/// State of a secure channel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelState {
    /// Channel is being established
    Establishing,
    /// Channel is active and ready for communication
    Active,
    /// Channel is rotating keys
    Rotating,
    /// Channel has been closed
    Closed,
    /// Channel encountered an error
    Error(String),
}

impl SecureChannel {
    /// Create a new secure channel
    pub fn new(
        channel_id: [u8; 32],
        context_id: ContextId,
        local: AuthorityId,
        remote: AuthorityId,
        epoch: u64,
    ) -> Self {
        Self {
            channel_id,
            context_id,
            local,
            remote,
            epoch,
            state: ChannelState::Establishing,
            needs_rotation: false,
            bytes_sent: 0,
            bytes_received: 0,
            agreement_mode: policy_for(CeremonyFlow::RendezvousSecureChannel).initial_mode(),
            reversion_risk: true,
        }
    }

    /// Get the channel ID
    pub fn channel_id(&self) -> [u8; 32] {
        self.channel_id
    }

    /// Get the context ID
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// Get the local authority
    pub fn local(&self) -> AuthorityId {
        self.local
    }

    /// Get the remote peer
    pub fn remote(&self) -> AuthorityId {
        self.remote
    }

    /// Get the current epoch
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Get the channel state
    pub fn state(&self) -> &ChannelState {
        &self.state
    }

    /// Check if the channel is active
    pub fn is_active(&self) -> bool {
        self.state == ChannelState::Active
    }

    /// Get bytes sent
    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Get bytes received
    pub fn bytes_received(&self) -> u64 {
        self.bytes_received
    }

    /// Mark the channel as active
    pub fn mark_active(&mut self) {
        self.state = ChannelState::Active;
        self.agreement_mode = AgreementMode::CoordinatorSoftSafe;
        self.reversion_risk = true;
    }

    /// Mark the channel as closed
    pub fn mark_closed(&mut self) {
        self.state = ChannelState::Closed;
    }

    /// Mark the channel as needing rotation
    pub fn mark_needs_rotation(&mut self) {
        self.needs_rotation = true;
    }

    /// Check if the channel needs key rotation
    pub fn needs_rotation(&self) -> bool {
        self.needs_rotation
    }

    /// Check if epoch rotation is needed based on current epoch
    pub fn needs_epoch_rotation(&self, current_epoch: u64) -> bool {
        self.epoch < current_epoch
    }

    /// Rotate channel keys for new epoch
    ///
    /// Full implementation will re-key the underlying Noise transport.
    pub fn rotate(&mut self, new_epoch: u64) -> AuraResult<()> {
        if new_epoch <= self.epoch {
            return Err(AuraError::invalid(
                "New epoch must be greater than current epoch",
            ));
        }

        self.state = ChannelState::Rotating;
        self.epoch = new_epoch;
        self.needs_rotation = false;
        self.state = ChannelState::Active;
        self.agreement_mode = AgreementMode::ConsensusFinalized;
        self.reversion_risk = false;

        Ok(())
    }

    /// Record bytes sent (for tracking)
    pub fn record_sent(&mut self, bytes: usize) {
        self.bytes_sent += bytes as u64;
    }

    /// Record bytes received (for tracking)
    pub fn record_received(&mut self, bytes: usize) {
        self.bytes_received += bytes as u64;
    }
}

// =============================================================================
// Channel Manager
// =============================================================================

/// Manages active secure channels
pub struct ChannelManager {
    /// Active channels by channel ID
    channels: HashMap<[u8; 32], SecureChannel>,
    /// Channels by (context, peer) for lookup
    by_context_peer: HashMap<(ContextId, AuthorityId), [u8; 32]>,
    /// Current epoch for rotation tracking
    current_epoch: u64,
}

impl ChannelManager {
    /// Create a new channel manager
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
            by_context_peer: HashMap::new(),
            current_epoch: 0,
        }
    }

    /// Get the current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Advance the epoch (triggers rotation checks)
    pub fn advance_epoch(&mut self, new_epoch: u64) {
        if new_epoch > self.current_epoch {
            self.current_epoch = new_epoch;
            // Mark all channels as needing rotation
            for channel in self.channels.values_mut() {
                if channel.epoch < new_epoch {
                    channel.mark_needs_rotation();
                }
            }
        }
    }

    /// Register a new channel
    pub fn register(&mut self, channel: SecureChannel) {
        let channel_id = channel.channel_id();
        let context_id = channel.context_id();
        let remote = channel.remote();

        self.by_context_peer
            .insert((context_id, remote), channel_id);
        self.channels.insert(channel_id, channel);
    }

    /// Get a channel by ID
    pub fn get(&self, channel_id: &[u8; 32]) -> Option<&SecureChannel> {
        self.channels.get(channel_id)
    }

    /// Get a mutable channel by ID
    pub fn get_mut(&mut self, channel_id: &[u8; 32]) -> Option<&mut SecureChannel> {
        self.channels.get_mut(channel_id)
    }

    /// Find channel by context and peer
    pub fn find_by_context_peer(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<&SecureChannel> {
        self.by_context_peer
            .get(&(context_id, peer))
            .and_then(|id| self.channels.get(id))
    }

    /// Find mutable channel by context and peer
    pub fn find_by_context_peer_mut(
        &mut self,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> Option<&mut SecureChannel> {
        if let Some(id) = self.by_context_peer.get(&(context_id, peer)).copied() {
            self.channels.get_mut(&id)
        } else {
            None
        }
    }

    /// Remove a channel
    pub fn remove(&mut self, channel_id: &[u8; 32]) -> Option<SecureChannel> {
        if let Some(channel) = self.channels.remove(channel_id) {
            self.by_context_peer
                .remove(&(channel.context_id(), channel.remote()));
            Some(channel)
        } else {
            None
        }
    }

    /// Get all channels that need rotation
    pub fn channels_needing_rotation(&self) -> Vec<[u8; 32]> {
        self.channels
            .iter()
            .filter(|(_, c)| c.needs_rotation())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Get all active channels
    pub fn active_channels(&self) -> Vec<&SecureChannel> {
        self.channels.values().filter(|c| c.is_active()).collect()
    }

    /// Get channel count
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Get active channel count
    pub fn active_channel_count(&self) -> usize {
        self.channels.values().filter(|c| c.is_active()).count()
    }
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Handshake Types
// =============================================================================

/// Configuration for Noise handshake
#[derive(Debug, Clone)]
pub struct HandshakeConfig {
    /// Local authority
    pub local: AuthorityId,
    /// Remote peer
    pub remote: AuthorityId,
    /// Context for the channel
    pub context_id: ContextId,
    /// Pre-shared key for IKpsk2
    pub psk: [u8; 32],
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

/// State machine for Noise IKpsk2 handshake
#[derive(Debug)]
pub struct Handshaker {
    /// Handshake configuration
    config: HandshakeConfig,
    /// Current state
    state: HandshakeState,
    /// Generated channel ID
    channel_id: Option<[u8; 32]>,
}

/// State of the handshake
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    /// Initial state
    Initial,
    /// Initiator has sent first message
    InitSent,
    /// Responder has received init
    InitReceived,
    /// Responder has sent response
    ResponseSent,
    /// Initiator has received response
    ResponseReceived,
    /// Handshake complete
    Complete,
    /// Handshake failed
    Failed(String),
}

/// Handshake result containing the established channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResult {
    /// Generated channel ID
    pub channel_id: [u8; 32],
    /// Final epoch
    pub epoch: u64,
    /// Whether we were initiator
    pub is_initiator: bool,
}

impl Handshaker {
    /// Create a new handshaker
    pub fn new(config: HandshakeConfig) -> Self {
        Self {
            config,
            state: HandshakeState::Initial,
            channel_id: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> &HandshakeState {
        &self.state
    }

    /// Get the channel ID (if generated)
    pub fn channel_id(&self) -> Option<[u8; 32]> {
        self.channel_id
    }

    /// Create init message (initiator)
    pub fn create_init_message(&mut self, epoch: u64) -> AuraResult<Vec<u8>> {
        if self.state != HandshakeState::Initial {
            return Err(AuraError::invalid("Invalid state for create_init_message"));
        }

        // Generate channel ID from participants
        self.channel_id = Some(generate_channel_id(
            &self.config.local,
            &self.config.remote,
            epoch,
        ));

        // Create handshake message (Noise IKpsk2 integration pending)
        let message = create_handshake_message(&self.config.local, &self.config.psk, epoch, true);

        self.state = HandshakeState::InitSent;
        Ok(message)
    }

    /// Process init message (responder)
    pub fn process_init(&mut self, _message: &[u8], epoch: u64) -> AuraResult<()> {
        if self.state != HandshakeState::Initial {
            return Err(AuraError::invalid("Invalid state for process_init"));
        }

        // Generate same channel ID
        self.channel_id = Some(generate_channel_id(
            &self.config.remote, // Remote is the initiator
            &self.config.local,
            epoch,
        ));

        // Process handshake message (Noise IKpsk2 integration pending)
        self.state = HandshakeState::InitReceived;
        Ok(())
    }

    /// Create response message (responder)
    pub fn create_response(&mut self, epoch: u64) -> AuraResult<Vec<u8>> {
        if self.state != HandshakeState::InitReceived {
            return Err(AuraError::invalid("Invalid state for create_response"));
        }

        // Create response message (Noise IKpsk2 integration pending)
        let message = create_handshake_message(&self.config.local, &self.config.psk, epoch, false);

        self.state = HandshakeState::ResponseSent;
        Ok(message)
    }

    /// Process response message (initiator)
    pub fn process_response(&mut self, _message: &[u8]) -> AuraResult<()> {
        if self.state != HandshakeState::InitSent {
            return Err(AuraError::invalid("Invalid state for process_response"));
        }

        // Process response message (Noise IKpsk2 integration pending)
        self.state = HandshakeState::ResponseReceived;
        Ok(())
    }

    /// Complete the handshake and create the channel
    pub fn complete(&mut self, epoch: u64, is_initiator: bool) -> AuraResult<HandshakeResult> {
        let valid_states = [
            HandshakeState::ResponseReceived,
            HandshakeState::ResponseSent,
        ];

        if !valid_states.contains(&self.state) {
            return Err(AuraError::invalid("Invalid state for complete"));
        }

        let channel_id = self
            .channel_id
            .ok_or_else(|| AuraError::internal("Channel ID not generated"))?;

        self.state = HandshakeState::Complete;

        Ok(HandshakeResult {
            channel_id,
            epoch,
            is_initiator,
        })
    }

    /// Build the secure channel from handshake result
    pub fn build_channel(&self, result: &HandshakeResult) -> AuraResult<SecureChannel> {
        if self.state != HandshakeState::Complete {
            return Err(AuraError::invalid("Handshake not complete"));
        }

        let mut channel = SecureChannel::new(
            result.channel_id,
            self.config.context_id,
            self.config.local,
            self.config.remote,
            result.epoch,
        );

        channel.mark_active();
        Ok(channel)
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

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

    hasher.update(b"CHANNEL_ID_V1");
    hasher.update(first);
    hasher.update(second);
    hasher.update(epoch.to_le_bytes());

    let result = hasher.finalize();
    let mut channel_id = [0u8; 32];
    channel_id.copy_from_slice(&result);
    channel_id
}

/// Create a handshake message (Noise IKpsk2 integration pending)
fn create_handshake_message(
    authority: &AuthorityId,
    psk: &[u8; 32],
    epoch: u64,
    is_init: bool,
) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(if is_init { b"INIT" } else { b"RESP" });
    hasher.update(authority_hash_bytes(authority));
    hasher.update(psk);
    hasher.update(epoch.to_le_bytes());
    hasher.finalize().to_vec()
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

    fn peer_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([3u8; 32])
    }

    #[test]
    fn test_secure_channel_creation() {
        let channel = SecureChannel::new(
            [0u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        assert_eq!(channel.epoch(), 1);
        assert_eq!(channel.state(), &ChannelState::Establishing);
        assert!(!channel.is_active());
    }

    #[test]
    fn test_secure_channel_activation() {
        let mut channel = SecureChannel::new(
            [0u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        channel.mark_active();
        assert!(channel.is_active());
        assert_eq!(channel.state(), &ChannelState::Active);
    }

    #[test]
    fn test_secure_channel_rotation() {
        let mut channel = SecureChannel::new(
            [0u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        channel.mark_active();

        // Check rotation needed
        assert!(channel.needs_epoch_rotation(2));
        assert!(!channel.needs_epoch_rotation(1));

        // Perform rotation
        channel.rotate(2).unwrap();
        assert_eq!(channel.epoch(), 2);
        assert!(!channel.needs_rotation());
    }

    #[test]
    fn test_channel_manager() {
        let mut manager = ChannelManager::new();

        let channel = SecureChannel::new(
            [1u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        manager.register(channel);
        assert_eq!(manager.channel_count(), 1);

        // Find by ID
        let found = manager.get(&[1u8; 32]);
        assert!(found.is_some());

        // Find by context/peer
        let found = manager.find_by_context_peer(test_context(), peer_authority());
        assert!(found.is_some());
    }

    #[test]
    fn test_channel_manager_epoch_advance() {
        let mut manager = ChannelManager::new();

        let mut channel = SecureChannel::new(
            [1u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );
        channel.mark_active();

        manager.register(channel);

        // Advance epoch
        manager.advance_epoch(2);

        // Channel should need rotation
        let needing_rotation = manager.channels_needing_rotation();
        assert_eq!(needing_rotation.len(), 1);
    }

    #[test]
    fn test_handshaker_flow() {
        let initiator_config = HandshakeConfig {
            local: test_authority(),
            remote: peer_authority(),
            context_id: test_context(),
            psk: [42u8; 32],
            timeout_ms: 5000,
        };

        let responder_config = HandshakeConfig {
            local: peer_authority(),
            remote: test_authority(),
            context_id: test_context(),
            psk: [42u8; 32],
            timeout_ms: 5000,
        };

        // Initiator creates init
        let mut initiator = Handshaker::new(initiator_config);
        let init_msg = initiator.create_init_message(1).unwrap();
        assert_eq!(initiator.state(), &HandshakeState::InitSent);

        // Responder processes init
        let mut responder = Handshaker::new(responder_config);
        responder.process_init(&init_msg, 1).unwrap();
        assert_eq!(responder.state(), &HandshakeState::InitReceived);

        // Responder creates response
        let response_msg = responder.create_response(1).unwrap();
        assert_eq!(responder.state(), &HandshakeState::ResponseSent);

        // Initiator processes response
        initiator.process_response(&response_msg).unwrap();
        assert_eq!(initiator.state(), &HandshakeState::ResponseReceived);

        // Both complete
        let initiator_result = initiator.complete(1, true).unwrap();
        let responder_result = responder.complete(1, false).unwrap();

        // Channel IDs should match
        assert_eq!(initiator_result.channel_id, responder_result.channel_id);
    }

    #[test]
    fn test_channel_id_determinism() {
        let auth_a = test_authority();
        let auth_b = peer_authority();

        // Order shouldn't matter
        let id1 = generate_channel_id(&auth_a, &auth_b, 1);
        let id2 = generate_channel_id(&auth_b, &auth_a, 1);
        assert_eq!(id1, id2);

        // Different epoch = different ID
        let id3 = generate_channel_id(&auth_a, &auth_b, 2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_mark_needs_rotation() {
        let mut channel = SecureChannel::new(
            [0u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        assert!(!channel.needs_rotation());
        channel.mark_needs_rotation();
        assert!(channel.needs_rotation());
    }

    #[test]
    fn test_bytes_tracking() {
        let mut channel = SecureChannel::new(
            [0u8; 32],
            test_context(),
            test_authority(),
            peer_authority(),
            1,
        );

        channel.record_sent(100);
        channel.record_sent(50);
        channel.record_received(75);

        assert_eq!(channel.bytes_sent(), 150);
        assert_eq!(channel.bytes_received(), 75);
    }
}

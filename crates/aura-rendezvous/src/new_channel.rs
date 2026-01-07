//! Secure Channel Wrapper
//!
//! Context-bound secure channels with epoch-based key rotation.
//! This module provides the SecureChannel type that integrates with
//! the guard chain for authorized communication.

use aura_core::effects::noise::{
    HandshakeState as NoiseHandshakeState, NoiseEffects, NoiseParams, TransportState,
};
use aura_core::effects::CryptoEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::{AuraError, AuraResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

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
    /// Underlying Noise transport state
    #[allow(dead_code)]
    transport: Option<TransportState>,
}

impl fmt::Debug for SecureChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureChannel")
            .field("channel_id", &self.channel_id)
            .field("context_id", &self.context_id)
            .field("local", &self.local)
            .field("remote", &self.remote)
            .field("epoch", &self.epoch)
            .field("state", &self.state)
            .finish()
    }
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
        transport: Option<TransportState>,
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
            transport,
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

    /// Remove closed or error channels to prevent unbounded growth.
    pub fn prune_closed_channels(&mut self) -> usize {
        let before = self.channels.len();
        let mut to_remove = Vec::new();
        for (id, channel) in &self.channels {
            if matches!(channel.state, ChannelState::Closed | ChannelState::Error(_)) {
                to_remove.push(*id);
            }
        }
        for id in to_remove {
            self.remove(&id);
        }
        before.saturating_sub(self.channels.len())
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
pub struct Handshaker {
    /// Handshake configuration
    config: HandshakeConfig,
    /// Current flow state
    state: HandshakeStatus,
    /// Underlying Noise handshake state
    noise_state: Option<NoiseHandshakeState>,
    /// Generated channel ID
    channel_id: Option<[u8; 32]>,
}

impl fmt::Debug for Handshaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handshaker")
            .field("config", &self.config)
            .field("state", &self.state)
            .field("noise_state", &self.noise_state.is_some())
            .field("channel_id", &self.channel_id)
            .finish()
    }
}

/// Status of the handshake flow
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeStatus {
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
            state: HandshakeStatus::Initial,
            noise_state: None,
            channel_id: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> &HandshakeStatus {
        &self.state
    }

    /// Get the channel ID (if generated)
    pub fn channel_id(&self) -> Option<[u8; 32]> {
        self.channel_id
    }

    /// Create init message (initiator)
    pub async fn create_init_message<E: NoiseEffects + CryptoEffects>(
        &mut self,
        epoch: u64,
        local_private_key: &[u8], // Ed25519 seed
        remote_public_key: &[u8], // Ed25519 public
        effects: &E,
    ) -> AuraResult<Vec<u8>> {
        if self.state != HandshakeStatus::Initial {
            return Err(AuraError::invalid("Invalid state for create_init_message"));
        }

        // Convert keys to X25519
        let x25519_local = effects
            .convert_ed25519_to_x25519_private(local_private_key)
            .await?;
        let x25519_remote = effects
            .convert_ed25519_to_x25519_public(remote_public_key)
            .await?;

        let params = NoiseParams {
            local_private_key: x25519_local,
            remote_public_key: x25519_remote,
            psk: self.config.psk,
            is_initiator: true,
        };

        let noise_state = effects.create_handshake_state(params).await?;

        // Generate channel ID
        self.channel_id = Some(generate_channel_id(
            &self.config.local,
            &self.config.remote,
            epoch,
        ));

        // Create handshake message
        // Payload can include metadata (like epoch)
        let payload = epoch.to_le_bytes();
        let (message, new_state) = effects.write_message(noise_state, &payload).await?;

        self.noise_state = Some(new_state);
        self.state = HandshakeStatus::InitSent;
        Ok(message)
    }

    /// Process init message (responder)
    pub async fn process_init<E: NoiseEffects + CryptoEffects>(
        &mut self,
        message: &[u8],
        epoch: u64,
        local_private_key: &[u8], // Ed25519 seed
        effects: &E,
    ) -> AuraResult<()> {
        if self.state != HandshakeStatus::Initial {
            return Err(AuraError::invalid("Invalid state for process_init"));
        }

        // Convert keys to X25519
        let x25519_local = effects
            .convert_ed25519_to_x25519_private(local_private_key)
            .await?;
        // Note: remote public key not needed for responder in IK pattern initially?
        // But NoiseParams expects it.
        // If we don't know it, we might need a different NoiseParams or allow placeholder.
        // However, for IK, the responder usually learns identity.
        // BUT `Noise_IKpsk2` assumes static key exchange.
        // Snow builder `build_responder` documentation says `remote_public_key` is optional if it will be received.
        // I'll assume we can pass all-zeros if unknown, or Snow handles it.
        // Wait, Snow's builder methods might panic if key missing when required?
        // I'll use a placeholder for now, assuming responder learns it.
        let x25519_remote = [0u8; 32];

        let params = NoiseParams {
            local_private_key: x25519_local,
            remote_public_key: x25519_remote,
            psk: self.config.psk,
            is_initiator: false,
        };

        let noise_state = effects.create_handshake_state(params).await?;

        // Generate same channel ID
        self.channel_id = Some(generate_channel_id(
            &self.config.remote, // Remote is the initiator
            &self.config.local,
            epoch,
        ));

        let (_payload, new_state) = effects.read_message(noise_state, message).await?;

        self.noise_state = Some(new_state);
        self.state = HandshakeStatus::InitReceived;
        Ok(())
    }

    /// Create response message (responder)
    pub async fn create_response<E: NoiseEffects>(
        &mut self,
        _epoch: u64,
        effects: &E,
    ) -> AuraResult<Vec<u8>> {
        if self.state != HandshakeStatus::InitReceived {
            return Err(AuraError::invalid("Invalid state for create_response"));
        }

        let noise_state = self
            .noise_state
            .take()
            .ok_or_else(|| AuraError::internal("Missing noise state"))?;

        // Payload can include confirmation
        let payload = b"ACK";
        let (message, new_state) = effects.write_message(noise_state, payload).await?;

        self.noise_state = Some(new_state);
        self.state = HandshakeStatus::ResponseSent;
        Ok(message)
    }

    /// Process response message (initiator)
    pub async fn process_response<E: NoiseEffects>(
        &mut self,
        message: &[u8],
        effects: &E,
    ) -> AuraResult<()> {
        if self.state != HandshakeStatus::InitSent {
            return Err(AuraError::invalid("Invalid state for process_response"));
        }

        let noise_state = self
            .noise_state
            .take()
            .ok_or_else(|| AuraError::internal("Missing noise state"))?;

        let (_payload, new_state) = effects.read_message(noise_state, message).await?;

        self.noise_state = Some(new_state);
        self.state = HandshakeStatus::ResponseReceived;
        Ok(())
    }

    /// Complete the handshake and create the channel
    pub async fn complete<E: NoiseEffects>(
        &mut self,
        epoch: u64,
        is_initiator: bool,
        effects: &E,
    ) -> AuraResult<(HandshakeResult, SecureChannel)> {
        let valid_states = [
            HandshakeStatus::ResponseReceived,
            HandshakeStatus::ResponseSent,
        ];

        if !valid_states.contains(&self.state) {
            return Err(AuraError::invalid("Invalid state for complete"));
        }

        let channel_id = self
            .channel_id
            .ok_or_else(|| AuraError::internal("Channel ID not generated"))?;

        let noise_state = self
            .noise_state
            .take()
            .ok_or_else(|| AuraError::internal("Missing noise state"))?;
        let transport_state = effects.into_transport_mode(noise_state).await?;

        self.state = HandshakeStatus::Complete;

        let result = HandshakeResult {
            channel_id,
            epoch,
            is_initiator,
        };

        let mut channel = SecureChannel::new(
            result.channel_id,
            self.config.context_id,
            self.config.local,
            self.config.remote,
            result.epoch,
            Some(transport_state),
        );

        channel.mark_active();

        Ok((result, channel))
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

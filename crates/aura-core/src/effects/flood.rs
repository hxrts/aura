//! Layer 1: Rendezvous Flooding Effect Trait Definitions
//!
//! This module defines the pure trait interface for rendezvous packet flooding
//! in Aura's progressive disclosure model. Rendezvous is the outermost layer
//! providing maximum privacy with minimum disclosure.
//!
//! **Effect Classification**: Application Effect
//! - Implemented by rendezvous crate (aura-rendezvous provides flooding implementation)
//! - Used by protocol layer (aura-protocol) for bootstrap discovery
//! - Core trait definition belongs in Layer 1 (foundation)
//!
//! # Design Principles
//!
//! **Opaque packets**: Rendezvous packets are encrypted and fixed-size.
//! Relays see only opaque blobs, cannot determine sender, recipient, or content.
//!
//! **Multi-hop flooding**: Packets propagate through the social topology
//! with TTL-based limiting. No acknowledgment or routing feedback.
//!
//! **Budget-controlled**: Origination and forwarding have separate budgets
//! to prevent abuse while allowing legitimate discovery.

use crate::types::{epochs::Epoch, flow::FlowBudget, identifiers::AuthorityId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Fixed size for rendezvous packets.
///
/// All packets are padded to this size to prevent size-based fingerprinting.
/// The size should accommodate typical rendezvous payloads with padding.
pub const RENDEZVOUS_PACKET_SIZE: usize = 512;

/// Default TTL for flood propagation.
///
/// Packets travel at most this many hops from origin. A TTL of 3 reaches
/// 2-hop neighborhood peers (block -> adjacent block -> their adjacent).
pub const DEFAULT_FLOOD_TTL: u8 = 3;

/// Rendezvous packet for flooding through the network.
///
/// Packets are encrypted and fixed-size to prevent information leakage.
/// Only the intended recipient can decrypt the payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendezvousPacket {
    /// Encrypted payload (fixed size, padded).
    ///
    /// Contains the actual rendezvous data encrypted to the recipient's
    /// public key. Padded to RENDEZVOUS_PACKET_SIZE bytes.
    pub ciphertext: Vec<u8>,

    /// Ephemeral public key for decryption.
    ///
    /// Used with the recipient's private key to derive the decryption key.
    /// Unlinkable to the sender's identity.
    pub ephemeral_key: [u8; 32],

    /// Time-to-live for flood propagation.
    ///
    /// Decremented at each hop. Packet is dropped when TTL reaches 0.
    pub ttl: u8,

    /// Packet nonce for deduplication.
    ///
    /// Used to detect duplicate packets during flooding. Not for replay
    /// protection (that's handled at higher layers).
    pub nonce: [u8; 16],
}

impl RendezvousPacket {
    /// Create a new rendezvous packet.
    ///
    /// Note: The ciphertext should already be padded to RENDEZVOUS_PACKET_SIZE.
    pub fn new(ciphertext: Vec<u8>, ephemeral_key: [u8; 32], ttl: u8, nonce: [u8; 16]) -> Self {
        Self {
            ciphertext,
            ephemeral_key,
            ttl,
            nonce,
        }
    }

    /// Check if the packet has expired (TTL = 0).
    pub fn is_expired(&self) -> bool {
        self.ttl == 0
    }

    /// Decrement TTL for forwarding.
    ///
    /// Returns a new packet with decremented TTL, or None if TTL was 0.
    pub fn decrement_ttl(&self) -> Option<Self> {
        if self.ttl == 0 {
            None
        } else {
            Some(Self {
                ciphertext: self.ciphertext.clone(),
                ephemeral_key: self.ephemeral_key,
                ttl: self.ttl - 1,
                nonce: self.nonce,
            })
        }
    }

    /// Get the packet size in bytes.
    pub fn size(&self) -> usize {
        self.ciphertext.len() + 32 + 1 + 16 // ciphertext + ephemeral_key + ttl + nonce
    }
}

/// Decrypted rendezvous payload.
///
/// Contains the actual rendezvous information after successful decryption.
/// The structure depends on the rendezvous protocol version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecryptedRendezvous {
    /// The sender's authority (revealed only to recipient).
    pub sender: AuthorityId,
    /// Protocol version for the payload.
    pub version: u8,
    /// The rendezvous payload data.
    pub payload: Vec<u8>,
}

/// Action to take after receiving a flooded packet.
///
/// Returned by the receive handler to indicate how the packet should
/// be processed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloodAction {
    /// Packet is for us, process it.
    ///
    /// Decryption succeeded and the payload is valid.
    Accept(DecryptedRendezvous),

    /// Forward to peers (TTL > 0, not seen before).
    ///
    /// We couldn't decrypt but the packet is valid for forwarding.
    Forward,

    /// Drop the packet.
    ///
    /// Either TTL = 0, duplicate (seen before), or invalid.
    Drop,
}

impl FloodAction {
    /// Check if this action accepts the packet.
    pub fn is_accept(&self) -> bool {
        matches!(self, Self::Accept(_))
    }

    /// Check if this action forwards the packet.
    pub fn is_forward(&self) -> bool {
        matches!(self, Self::Forward)
    }

    /// Check if this action drops the packet.
    pub fn is_drop(&self) -> bool {
        matches!(self, Self::Drop)
    }

    /// Get the decrypted payload if accepted.
    pub fn payload(&self) -> Option<&DecryptedRendezvous> {
        match self {
            Self::Accept(payload) => Some(payload),
            _ => None,
        }
    }
}

/// Error type for flood operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FloodError {
    /// Budget exhausted for originating packets.
    OriginateBudgetExhausted,

    /// Budget exhausted for forwarding packets.
    ForwardBudgetExhausted,

    /// Packet too large.
    PacketTooLarge {
        /// Actual size in bytes
        size: usize,
        /// Maximum allowed size
        max_size: usize,
    },

    /// Encryption failed.
    EncryptionError(String),

    /// No flood targets available.
    NoTargets,

    /// Network error during flooding.
    NetworkError(String),
}

impl std::fmt::Display for FloodError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OriginateBudgetExhausted => write!(f, "originate budget exhausted"),
            Self::ForwardBudgetExhausted => write!(f, "forward budget exhausted"),
            Self::PacketTooLarge { size, max_size } => {
                write!(f, "packet too large: {} bytes (max {})", size, max_size)
            }
            Self::EncryptionError(msg) => write!(f, "encryption error: {}", msg),
            Self::NoTargets => write!(f, "no flood targets available"),
            Self::NetworkError(msg) => write!(f, "network error: {}", msg),
        }
    }
}

impl std::error::Error for FloodError {}

/// Budget for rendezvous packet flooding.
///
/// Tracks separate limits for originating and forwarding packets,
/// preventing abuse while allowing legitimate discovery operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FloodBudget {
    /// Maximum packets we can originate per epoch.
    pub originate_limit: u32,
    /// Packets we've originated this epoch.
    pub originate_spent: u32,
    /// Maximum packets we can forward per epoch.
    pub forward_limit: u32,
    /// Packets we've forwarded this epoch.
    pub forward_spent: u32,
    /// Current epoch binding the budget.
    pub epoch: Epoch,
}

impl FloodBudget {
    /// Default originate limit per epoch.
    pub const DEFAULT_ORIGINATE_LIMIT: u32 = 100;
    /// Default forward limit per epoch.
    pub const DEFAULT_FORWARD_LIMIT: u32 = 1000;

    /// Create a new flood budget with default limits.
    pub fn new(epoch: Epoch) -> Self {
        Self {
            originate_limit: Self::DEFAULT_ORIGINATE_LIMIT,
            originate_spent: 0,
            forward_limit: Self::DEFAULT_FORWARD_LIMIT,
            forward_spent: 0,
            epoch,
        }
    }

    /// Create a flood budget with custom limits.
    pub fn with_limits(originate_limit: u32, forward_limit: u32, epoch: Epoch) -> Self {
        Self {
            originate_limit,
            originate_spent: 0,
            forward_limit,
            forward_spent: 0,
            epoch,
        }
    }

    /// Remaining originate budget.
    pub fn originate_remaining(&self) -> u32 {
        self.originate_limit.saturating_sub(self.originate_spent)
    }

    /// Remaining forward budget.
    pub fn forward_remaining(&self) -> u32 {
        self.forward_limit.saturating_sub(self.forward_spent)
    }

    /// Check if we can originate a packet.
    pub fn can_originate(&self) -> bool {
        self.originate_spent < self.originate_limit
    }

    /// Check if we can forward a packet.
    pub fn can_forward(&self) -> bool {
        self.forward_spent < self.forward_limit
    }

    /// Record an originate operation.
    ///
    /// Returns true if successful, false if budget exhausted.
    pub fn record_originate(&mut self) -> bool {
        if self.can_originate() {
            self.originate_spent += 1;
            true
        } else {
            false
        }
    }

    /// Record a forward operation.
    ///
    /// Returns true if successful, false if budget exhausted.
    pub fn record_forward(&mut self) -> bool {
        if self.can_forward() {
            self.forward_spent += 1;
            true
        } else {
            false
        }
    }

    /// Advance to a new epoch, resetting spent counters.
    pub fn rotate_epoch(&mut self, next_epoch: Epoch) {
        if next_epoch.value() > self.epoch.value() {
            self.epoch = next_epoch;
            self.originate_spent = 0;
            self.forward_spent = 0;
        }
    }

    /// Merge two replicas of the flood budget.
    ///
    /// Takes max of spent (join-semilattice behavior).
    pub fn merge(&self, other: &Self) -> Self {
        let epoch = if self.epoch.value() >= other.epoch.value() {
            self.epoch
        } else {
            other.epoch
        };

        Self {
            originate_limit: self.originate_limit.min(other.originate_limit),
            originate_spent: self.originate_spent.max(other.originate_spent),
            forward_limit: self.forward_limit.min(other.forward_limit),
            forward_spent: self.forward_spent.max(other.forward_spent),
            epoch,
        }
    }
}

impl Default for FloodBudget {
    fn default() -> Self {
        Self::new(Epoch::initial())
    }
}

/// Layered budget for the progressive disclosure model.
///
/// Each layer of discovery has its own budget to prevent cross-layer
/// abuse. Operations charge against the appropriate layer's budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayeredBudget {
    /// Budget for rendezvous flooding (outermost layer).
    pub flood: FloodBudget,
    /// Budget for neighborhood operations.
    pub neighborhood: FlowBudget,
    /// Budget for block operations.
    pub block: FlowBudget,
    /// Budget for direct channel traffic (innermost layer).
    pub direct: FlowBudget,
}

impl LayeredBudget {
    /// Create a new layered budget with default limits.
    pub fn new(epoch: Epoch) -> Self {
        Self {
            flood: FloodBudget::new(epoch),
            neighborhood: FlowBudget::new(1000, epoch),
            block: FlowBudget::new(1000, epoch),
            direct: FlowBudget::new(10000, epoch),
        }
    }

    /// Rotate all budgets to a new epoch.
    pub fn rotate_epoch(&mut self, next_epoch: Epoch) {
        self.flood.rotate_epoch(next_epoch);
        self.neighborhood.rotate_epoch(next_epoch);
        self.block.rotate_epoch(next_epoch);
        self.direct.rotate_epoch(next_epoch);
    }
}

impl Default for LayeredBudget {
    fn default() -> Self {
        Self::new(Epoch::initial())
    }
}

/// Effect trait for rendezvous packet flooding.
///
/// This trait defines the interface for flooding rendezvous packets
/// through the social topology for bootstrap discovery.
///
/// # Implementation Notes
///
/// Implementations should:
/// - Encrypt packets to recipient's public key
/// - Pad packets to fixed size (RENDEZVOUS_PACKET_SIZE)
/// - Track seen nonces for deduplication
/// - Decrement TTL on forward
/// - Try decrypt on receive
/// - Forward to peers based on social topology
///
/// # Example
///
/// ```ignore
/// impl RendezvousFlooder for FloodPropagation {
///     async fn flood(&self, packet: RendezvousPacket) -> Result<(), FloodError> {
///         if !self.budget.can_originate() {
///             return Err(FloodError::OriginateBudgetExhausted);
///         }
///         let targets = self.flood_targets();
///         for target in targets {
///             self.transport.send(target, &packet).await?;
///         }
///         self.budget.record_originate();
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait RendezvousFlooder: Send + Sync {
    /// Flood a rendezvous packet into the network.
    ///
    /// Originates a flood from this node, sending to immediate peers
    /// who will continue propagation based on TTL.
    ///
    /// # Arguments
    /// * `packet` - The packet to flood
    ///
    /// # Errors
    /// - `OriginateBudgetExhausted` if we've exceeded our originate limit
    /// - `NoTargets` if no peers are available for flooding
    /// - `NetworkError` if sending failed
    async fn flood(&self, packet: RendezvousPacket) -> Result<(), FloodError>;

    /// Handle an incoming flooded packet.
    ///
    /// Processes a packet received from a peer:
    /// 1. Check if we've seen this nonce before (dedup)
    /// 2. Try to decrypt (if we're the recipient)
    /// 3. Decide to accept, forward, or drop
    ///
    /// # Arguments
    /// * `packet` - The received packet
    /// * `from` - The authority that sent us this packet
    ///
    /// # Returns
    /// The action to take (Accept, Forward, or Drop)
    async fn receive(&self, packet: RendezvousPacket, from: AuthorityId) -> FloodAction;

    /// Get the current flood budget.
    fn budget(&self) -> &FloodBudget;
}

/// Blanket implementation for Arc<T> where T: RendezvousFlooder
#[async_trait]
impl<T: RendezvousFlooder + ?Sized> RendezvousFlooder for std::sync::Arc<T> {
    async fn flood(&self, packet: RendezvousPacket) -> Result<(), FloodError> {
        (**self).flood(packet).await
    }

    async fn receive(&self, packet: RendezvousPacket, from: AuthorityId) -> FloodAction {
        (**self).receive(packet, from).await
    }

    fn budget(&self) -> &FloodBudget {
        (**self).budget()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rendezvous_packet_ttl() {
        let packet = RendezvousPacket::new(vec![0u8; 512], [1u8; 32], 3, [2u8; 16]);

        assert!(!packet.is_expired());
        assert_eq!(packet.ttl, 3);

        let forwarded = packet.decrement_ttl().unwrap();
        assert_eq!(forwarded.ttl, 2);

        let expired = RendezvousPacket::new(vec![0u8; 512], [1u8; 32], 0, [2u8; 16]);
        assert!(expired.is_expired());
        assert!(expired.decrement_ttl().is_none());
    }

    #[test]
    fn test_flood_action_checks() {
        let accept = FloodAction::Accept(DecryptedRendezvous {
            sender: AuthorityId::default(),
            version: 1,
            payload: vec![],
        });
        assert!(accept.is_accept());
        assert!(!accept.is_forward());
        assert!(!accept.is_drop());
        assert!(accept.payload().is_some());

        let forward = FloodAction::Forward;
        assert!(!forward.is_accept());
        assert!(forward.is_forward());
        assert!(!forward.is_drop());
        assert!(forward.payload().is_none());

        let drop = FloodAction::Drop;
        assert!(!drop.is_accept());
        assert!(!drop.is_forward());
        assert!(drop.is_drop());
    }

    #[test]
    fn test_flood_budget_operations() {
        let epoch = Epoch::initial();
        let mut budget = FloodBudget::with_limits(2, 3, epoch);

        assert!(budget.can_originate());
        assert!(budget.can_forward());
        assert_eq!(budget.originate_remaining(), 2);
        assert_eq!(budget.forward_remaining(), 3);

        assert!(budget.record_originate());
        assert_eq!(budget.originate_remaining(), 1);

        assert!(budget.record_originate());
        assert_eq!(budget.originate_remaining(), 0);

        assert!(!budget.record_originate()); // Exhausted
        assert!(!budget.can_originate());
    }

    #[test]
    fn test_flood_budget_epoch_rotation() {
        let epoch1 = Epoch::initial();
        let epoch2 = Epoch::new(epoch1.value() + 1);

        let mut budget = FloodBudget::with_limits(2, 3, epoch1);
        budget.record_originate();
        budget.record_forward();

        assert_eq!(budget.originate_spent, 1);
        assert_eq!(budget.forward_spent, 1);

        budget.rotate_epoch(epoch2);

        assert_eq!(budget.originate_spent, 0);
        assert_eq!(budget.forward_spent, 0);
        assert_eq!(budget.epoch, epoch2);
    }

    #[test]
    fn test_flood_budget_merge() {
        let epoch = Epoch::initial();
        let mut b1 = FloodBudget::with_limits(10, 100, epoch);
        let mut b2 = FloodBudget::with_limits(8, 120, epoch);

        b1.record_originate();
        b1.record_originate();
        b2.record_originate();
        b2.record_forward();
        b2.record_forward();
        b2.record_forward();

        let merged = b1.merge(&b2);

        // Limits take min
        assert_eq!(merged.originate_limit, 8);
        assert_eq!(merged.forward_limit, 100);

        // Spent takes max
        assert_eq!(merged.originate_spent, 2); // max(2, 1)
        assert_eq!(merged.forward_spent, 3); // max(0, 3)
    }

    #[test]
    fn test_layered_budget_creation() {
        let epoch = Epoch::initial();
        let budget = LayeredBudget::new(epoch);

        assert!(budget.flood.can_originate());
        assert!(budget.flood.can_forward());
        assert!(budget.neighborhood.can_charge(100));
        assert!(budget.block.can_charge(100));
        assert!(budget.direct.can_charge(100));
    }

    #[test]
    fn test_flood_error_display() {
        let originate = FloodError::OriginateBudgetExhausted;
        assert!(originate.to_string().contains("originate"));

        let forward = FloodError::ForwardBudgetExhausted;
        assert!(forward.to_string().contains("forward"));

        let too_large = FloodError::PacketTooLarge {
            size: 1000,
            max_size: 512,
        };
        assert!(too_large.to_string().contains("too large"));
    }
}

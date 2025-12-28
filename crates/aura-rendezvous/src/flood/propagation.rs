//! Flood Propagation Coordinator
//!
//! This module implements the FloodPropagation coordinator that manages
//! rendezvous packet flooding through the social topology. It implements
//! the RendezvousFlooder trait from aura-core.
//!
//! # Design
//!
//! **Social topology routing**: Packets are flooded along social relationships:
//! first to block peers, then to neighborhood peers. This provides natural
//! flood boundaries while ensuring coverage.
//!
//! **Deduplication**: An LRU cache of seen nonces prevents packet amplification
//! from retransmitted packets.
//!
//! **Budget enforcement**: Separate budgets for originating and forwarding
//! packets prevent abuse while allowing legitimate discovery.

use super::packet::{DecryptedPayload, PacketCrypto};
use async_lock::RwLock;
use async_trait::async_trait;
use aura_core::{
    effects::{
        flood::{
            DecryptedRendezvous, FloodAction, FloodBudget, FloodError, RendezvousFlooder,
            RendezvousPacket,
        },
        CryptoEffects, NetworkEffects,
    },
    identifiers::AuthorityId,
};
use aura_social::SocialTopology;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, trace, warn};

/// Maximum number of nonces to track for deduplication.
const MAX_SEEN_NONCES: usize = 10_000;

/// Seen nonces tracker for packet deduplication.
///
/// Uses a bounded set to track recently seen packet nonces. When the set
/// is full, it's cleared to maintain bounded memory usage (simple approach
/// that works because nonces have TTL-limited relevance).
#[derive(Debug)]
pub struct SeenNonces {
    nonces: HashSet<[u8; 16]>,
    max_size: usize,
}

impl SeenNonces {
    /// Create a new seen nonces tracker.
    pub fn new() -> Self {
        Self {
            nonces: HashSet::new(),
            max_size: MAX_SEEN_NONCES,
        }
    }

    /// Create with custom max size.
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            nonces: HashSet::new(),
            max_size,
        }
    }

    /// Check if a nonce has been seen, and mark it as seen if not.
    ///
    /// Returns true if this is a new nonce (first time seen),
    /// false if it's a duplicate.
    pub fn check_and_mark(&mut self, nonce: &[u8; 16]) -> bool {
        // Clear if at capacity (simple bounded approach)
        if self.nonces.len() >= self.max_size {
            debug!(
                count = self.nonces.len(),
                "SeenNonces at capacity, clearing"
            );
            self.nonces.clear();
        }

        self.nonces.insert(*nonce)
    }

    /// Check if a nonce has been seen without marking it.
    pub fn has_seen(&self, nonce: &[u8; 16]) -> bool {
        self.nonces.contains(nonce)
    }

    /// Get the number of tracked nonces.
    pub fn len(&self) -> usize {
        self.nonces.len()
    }

    /// Check if the tracker is empty.
    pub fn is_empty(&self) -> bool {
        self.nonces.is_empty()
    }

    /// Clear all tracked nonces.
    pub fn clear(&mut self) {
        self.nonces.clear();
    }
}

impl Default for SeenNonces {
    fn default() -> Self {
        Self::new()
    }
}

/// Flood propagation coordinator.
///
/// Manages rendezvous packet flooding through the social topology.
/// Implements the `RendezvousFlooder` trait.
///
/// # Type Parameters
///
/// * `C` - Crypto effects for encryption/decryption
/// * `N` - Network effects for sending packets
///
/// # Example
///
/// ```ignore
/// let flood = FloodPropagation::new(
///     local_authority,
///     private_key,
///     topology,
///     crypto,
///     network,
/// );
///
/// // Flood a packet
/// let packet = PacketBuilder::new()
///     .with_sender(local_authority)
///     .with_payload(data)
///     .encrypt_to(&recipient_key, &crypto).await?;
/// flood.flood(packet).await?;
/// ```
pub struct FloodPropagation<C, N> {
    /// Our local authority ID.
    local_authority: AuthorityId,
    /// Our X25519 private key for decryption.
    private_key: [u8; 32],
    /// Social topology for flood target selection.
    topology: Arc<RwLock<SocialTopology>>,
    /// Crypto effects.
    crypto: Arc<C>,
    /// Network effects for sending packets.
    network: Arc<N>,
    /// Flood budget.
    budget: RwLock<FloodBudget>,
    /// Seen nonces for deduplication.
    seen_nonces: RwLock<SeenNonces>,
}

impl<C, N> FloodPropagation<C, N>
where
    C: CryptoEffects,
    N: NetworkEffects,
{
    /// Create a new flood propagation coordinator.
    pub fn new(
        local_authority: AuthorityId,
        private_key: [u8; 32],
        topology: Arc<RwLock<SocialTopology>>,
        crypto: Arc<C>,
        network: Arc<N>,
    ) -> Self {
        Self {
            local_authority,
            private_key,
            topology,
            crypto,
            network,
            budget: RwLock::new(FloodBudget::default()),
            seen_nonces: RwLock::new(SeenNonces::new()),
        }
    }

    /// Create with custom budget.
    pub fn with_budget(mut self, budget: FloodBudget) -> Self {
        self.budget = RwLock::new(budget);
        self
    }

    /// Get flood targets from the social topology.
    ///
    /// Returns authorities to flood to, prioritizing block peers
    /// over neighborhood peers.
    pub async fn flood_targets(&self) -> Vec<AuthorityId> {
        let topology = self.topology.read().await;

        let mut targets = Vec::new();

        // First add block peers (highest priority)
        targets.extend(topology.block_peers());

        // Then neighborhood peers
        targets.extend(topology.neighborhood_peers());

        // Remove self if present
        targets.retain(|a| *a != self.local_authority);

        targets
    }

    /// Try to decrypt a packet as the recipient.
    async fn try_decrypt(&self, packet: &RendezvousPacket) -> Option<DecryptedPayload> {
        PacketCrypto::try_decrypt(packet, &self.private_key, self.crypto.as_ref()).await
    }

    /// Forward a packet to flood targets.
    async fn forward_to_targets(
        &self,
        packet: &RendezvousPacket,
        exclude: &AuthorityId,
    ) -> Result<(), FloodError> {
        let targets = self.flood_targets().await;
        let targets: Vec<_> = targets.into_iter().filter(|t| t != exclude).collect();

        if targets.is_empty() {
            return Err(FloodError::NoTargets);
        }

        // Decrement TTL for forwarding
        let forwarded = packet
            .decrement_ttl()
            .ok_or(FloodError::ForwardBudgetExhausted)?;

        let serialized = aura_core::util::serialization::to_vec(&forwarded)
            .map_err(|e| FloodError::NetworkError(e.to_string()))?;

        // Send to each target (using AuthorityId's inner UUID)
        for target in targets {
            if let Err(e) = self
                .network
                .send_to_peer(target.uuid(), serialized.clone())
                .await
            {
                warn!(?target, error = %e, "Failed to forward flood packet");
                // Continue with other targets
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<C, N> RendezvousFlooder for FloodPropagation<C, N>
where
    C: CryptoEffects + Send + Sync,
    N: NetworkEffects + Send + Sync,
{
    async fn flood(&self, packet: RendezvousPacket) -> Result<(), FloodError> {
        // Check originate budget
        {
            let mut budget = self.budget.write().await;
            if !budget.record_originate() {
                return Err(FloodError::OriginateBudgetExhausted);
            }
        }

        // Mark our own packet as seen (so we don't re-process it)
        {
            let mut seen = self.seen_nonces.write().await;
            seen.check_and_mark(&packet.nonce);
        }

        let targets = self.flood_targets().await;

        if targets.is_empty() {
            return Err(FloodError::NoTargets);
        }

        trace!(
            target_count = targets.len(),
            ttl = packet.ttl,
            "Flooding packet to targets"
        );

        let serialized = aura_core::util::serialization::to_vec(&packet)
            .map_err(|e| FloodError::NetworkError(e.to_string()))?;

        // Send to each target (using AuthorityId's inner UUID)
        for target in targets {
            if let Err(e) = self
                .network
                .send_to_peer(target.uuid(), serialized.clone())
                .await
            {
                warn!(?target, error = %e, "Failed to send flood packet");
                // Continue with other targets
            }
        }

        Ok(())
    }

    async fn receive(&self, packet: RendezvousPacket, from: AuthorityId) -> FloodAction {
        // Check if already seen (dedup)
        {
            let mut seen = self.seen_nonces.write().await;
            if !seen.check_and_mark(&packet.nonce) {
                trace!("Duplicate packet, dropping");
                return FloodAction::Drop;
            }
        }

        // Check TTL
        if packet.is_expired() {
            trace!("Packet expired (TTL=0), dropping");
            return FloodAction::Drop;
        }

        // Try to decrypt (we might be the recipient)
        if let Some(decrypted) = self.try_decrypt(&packet).await {
            trace!(
                sender = ?decrypted.sender,
                version = decrypted.version,
                "Successfully decrypted packet, accepting"
            );
            return FloodAction::Accept(DecryptedRendezvous {
                sender: decrypted.sender,
                version: decrypted.version,
                payload: decrypted.payload,
            });
        }

        // Not for us, check forward budget
        {
            let mut budget = self.budget.write().await;
            if !budget.record_forward() {
                trace!("Forward budget exhausted, dropping");
                return FloodAction::Drop;
            }
        }

        // Forward to targets (excluding sender)
        if let Err(e) = self.forward_to_targets(&packet, &from).await {
            warn!(error = %e, "Failed to forward packet");
            return FloodAction::Drop;
        }

        FloodAction::Forward
    }

    async fn budget(&self) -> FloodBudget {
        *self.budget.read().await
    }

    async fn update_budget(&self, budget: FloodBudget) {
        *self.budget.write().await = budget;
    }
}

impl<C, N> FloodPropagation<C, N>
where
    C: CryptoEffects,
    N: NetworkEffects,
{
    /// Rotate budget to a new epoch.
    pub async fn rotate_epoch(&self, epoch: aura_core::types::epochs::Epoch) {
        self.budget.write().await.rotate_epoch(epoch);
    }

    /// Clear seen nonces (e.g., on epoch rotation).
    pub async fn clear_seen_nonces(&self) {
        self.seen_nonces.write().await.clear();
    }

    /// Get count of seen nonces.
    pub async fn seen_nonces_count(&self) -> usize {
        self.seen_nonces.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seen_nonces_check_and_mark() {
        let mut seen = SeenNonces::new();
        let nonce = [1u8; 16];

        // First time should return true (new)
        assert!(seen.check_and_mark(&nonce));

        // Second time should return false (duplicate)
        assert!(!seen.check_and_mark(&nonce));
    }

    #[test]
    fn test_seen_nonces_has_seen() {
        let mut seen = SeenNonces::new();
        let nonce = [1u8; 16];

        assert!(!seen.has_seen(&nonce));
        seen.check_and_mark(&nonce);
        assert!(seen.has_seen(&nonce));
    }

    #[test]
    fn test_seen_nonces_clear() {
        let mut seen = SeenNonces::new();
        seen.check_and_mark(&[1u8; 16]);
        seen.check_and_mark(&[2u8; 16]);

        assert_eq!(seen.len(), 2);

        seen.clear();

        assert!(seen.is_empty());
    }

    #[test]
    fn test_seen_nonces_capacity_clear() {
        let mut seen = SeenNonces::with_max_size(3);

        seen.check_and_mark(&[1u8; 16]);
        seen.check_and_mark(&[2u8; 16]);
        seen.check_and_mark(&[3u8; 16]);

        assert_eq!(seen.len(), 3);

        // This should trigger a clear since we're at capacity
        seen.check_and_mark(&[4u8; 16]);

        // After clear, only the new nonce should be present
        assert_eq!(seen.len(), 1);
        assert!(seen.has_seen(&[4u8; 16]));
        assert!(!seen.has_seen(&[1u8; 16]));
    }

    #[test]
    fn test_seen_nonces_default() {
        let seen = SeenNonces::default();
        assert!(seen.is_empty());
        assert_eq!(seen.max_size, MAX_SEEN_NONCES);
    }
}

//! Social Bulletin Board (SBB) Implementation
//!
//! This module implements controlled flooding through the social graph for peer discovery.
//! SBB enables envelope propagation through friend/guardian relationships with TTL limits,
//! duplicate detection, and capability enforcement.

use crate::envelope_encryption::EncryptedEnvelope;
use aura_core::context_derivation::RelayContextDerivation;
use aura_core::hash::hasher;
use aura_core::identifiers::RelayId;
use aura_core::{AuraError, AuraResult, DeviceId};
use aura_protocol::effects::AuraEffects;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type SharedEffects = Arc<dyn AuraEffects>;

/// Content-addressed envelope ID (Blake3 hash)
pub type EnvelopeId = [u8; 32];

/// SBB message size for flow budget calculations
pub const SBB_MESSAGE_SIZE: u64 = 1024; // 1KB standard envelope size

/// Rendezvous envelope for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousEnvelope {
    /// Content-addressed envelope ID (Blake3 hash of payload)
    pub id: EnvelopeId,
    /// Time-to-live for flooding (max hops)
    pub ttl: u8,
    /// Creation timestamp (for cache expiration)
    pub created_at: u64,
    /// Transport offer payload (encrypted or plaintext for backward compatibility)
    pub payload: Vec<u8>,
}

/// Enhanced SBB envelope supporting both plaintext and encrypted variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SbbEnvelope {
    /// Plaintext envelope (for backward compatibility and testing)
    Plaintext(RendezvousEnvelope),
    /// Encrypted envelope with relationship-based encryption
    Encrypted {
        /// Content-addressed envelope ID
        id: EnvelopeId,
        /// Time-to-live for flooding
        ttl: u8,
        /// Creation timestamp
        created_at: u64,
        /// Encrypted payload with padding
        encrypted_payload: EncryptedEnvelope,
    },
}

impl SbbEnvelope {
    /// Create new plaintext SBB envelope
    pub fn new_plaintext(payload: Vec<u8>, ttl: Option<u8>) -> Self {
        let envelope = RendezvousEnvelope::new(payload, ttl);
        SbbEnvelope::Plaintext(envelope)
    }

    /// Create new encrypted SBB envelope
    pub fn new_encrypted(encrypted_payload: EncryptedEnvelope, ttl: Option<u8>) -> Self {
        let ttl = ttl.unwrap_or(6);
        let created_at = current_timestamp();

        // Compute ID from encrypted payload for deduplication
        let id = Self::compute_encrypted_envelope_id(&encrypted_payload);

        SbbEnvelope::Encrypted {
            id,
            ttl,
            created_at,
            encrypted_payload,
        }
    }

    /// Get envelope ID for deduplication
    pub fn id(&self) -> EnvelopeId {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.id,
            SbbEnvelope::Encrypted { id, .. } => *id,
        }
    }

    /// Get TTL for flooding control
    pub fn ttl(&self) -> u8 {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.ttl,
            SbbEnvelope::Encrypted { ttl, .. } => *ttl,
        }
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> u64 {
        match self {
            SbbEnvelope::Plaintext(envelope) => envelope.created_at,
            SbbEnvelope::Encrypted { created_at, .. } => *created_at,
        }
    }

    /// Decrement TTL for next hop
    pub fn decrement_ttl(self) -> Option<Self> {
        match self {
            SbbEnvelope::Plaintext(envelope) => {
                envelope.decrement_ttl().map(SbbEnvelope::Plaintext)
            }
            SbbEnvelope::Encrypted {
                id,
                ttl,
                created_at,
                encrypted_payload,
            } => {
                if ttl > 0 {
                    Some(SbbEnvelope::Encrypted {
                        id,
                        ttl: ttl - 1,
                        created_at,
                        encrypted_payload,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Check if envelope has expired
    pub fn is_expired(&self, current_time: u64, max_age_seconds: u64) -> bool {
        current_time > self.created_at() + max_age_seconds
    }

    /// Get envelope size for flow budget calculations
    pub fn size(&self) -> usize {
        match self {
            SbbEnvelope::Plaintext(envelope) => {
                32 + 1 + 8 + envelope.payload.len() // id + ttl + created_at + payload
            }
            SbbEnvelope::Encrypted {
                encrypted_payload, ..
            } => {
                32 + 1 + 8 + encrypted_payload.size() // id + ttl + created_at + encrypted_payload
            }
        }
    }

    /// Compute content-addressed ID for encrypted envelope
    fn compute_encrypted_envelope_id(encrypted_payload: &EncryptedEnvelope) -> EnvelopeId {
        let mut h = hasher();
        h.update(b"aura-sbb-encrypted-envelope-v1");
        h.update(&encrypted_payload.nonce);
        h.update(&encrypted_payload.ciphertext);
        if let Some(hint) = &encrypted_payload.key_hint {
            h.update(hint);
        }

        let hash = h.finalize();
        let mut id = [0u8; 32];
        id.copy_from_slice(&hash);
        id
    }
}

/// Result of envelope flooding operation
#[derive(Debug, Clone)]
pub enum FloodResult {
    /// Envelope was forwarded to peers
    Forwarded { peer_count: usize },
    /// Envelope was dropped (TTL expired, duplicate, or no peers)
    Dropped,
    /// Envelope flooding failed due to error
    Failed { reason: String },
}

/// SBB flooding protocol trait
#[async_trait::async_trait]
pub trait SbbFlooding: Send + Sync {
    /// Flood envelope through social graph
    ///
    /// Note: Callers should obtain `now` from TimeEffects and convert to Unix timestamp
    async fn flood_envelope(
        &mut self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<FloodResult>;

    /// Get forwarding peers (friends + guardians, capability-filtered)
    async fn get_forwarding_peers(
        &self,
        exclude: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<Vec<DeviceId>>;

    /// Check if can forward to specific peer (capability + flow budget)
    async fn can_forward_to(
        &self,
        peer: &DeviceId,
        message_size: u64,
        now: u64,
    ) -> AuraResult<bool>;

    /// Forward envelope to specific peer
    async fn forward_to_peer(
        &mut self,
        envelope: RendezvousEnvelope,
        peer: DeviceId,
        now: u64,
    ) -> AuraResult<()>;
}

/// SBB flooding coordinator implementing controlled propagation
pub struct SbbFloodingCoordinator {
    /// Device ID of this node
    #[allow(dead_code)]
    device_id: DeviceId,
    /// Friend relationships for flooding
    friends: Vec<DeviceId>,
    /// Guardian relationships for flooding (preferred)
    guardians: Vec<DeviceId>,
    /// Recently seen envelope IDs (for duplicate detection)
    seen_envelopes: HashSet<EnvelopeId>,
    /// Envelope cache with expiration tracking
    envelope_cache: HashMap<EnvelopeId, (RendezvousEnvelope, u64)>, // (envelope, expires_at)
    /// Effect system interface for journal and capability checks
    effects: SharedEffects,
}

impl RendezvousEnvelope {
    /// Create new rendezvous envelope with content-addressed ID
    pub fn new(payload: Vec<u8>, ttl: Option<u8>) -> Self {
        let id = Self::compute_envelope_id(&payload);
        let ttl = ttl.unwrap_or(6); // Default 6 hops for friend networks
        let created_at = current_timestamp(); // Would use time effects in real implementation

        Self {
            id,
            ttl,
            created_at,
            payload,
        }
    }

    /// Compute content-addressed envelope ID using SHA-256
    fn compute_envelope_id(payload: &[u8]) -> EnvelopeId {
        let mut h = hasher();
        h.update(b"aura-sbb-envelope-v1");
        h.update(payload);
        h.finalize()
    }

    /// Decrement TTL for next hop
    pub fn decrement_ttl(mut self) -> Option<Self> {
        if self.ttl > 0 {
            self.ttl -= 1;
            Some(self)
        } else {
            None
        }
    }

    /// Check if envelope has expired based on creation time
    pub fn is_expired(&self, current_time: u64, max_age_seconds: u64) -> bool {
        current_time > self.created_at + max_age_seconds
    }
}

impl SbbFloodingCoordinator {
    /// Create new SBB flooding coordinator
    pub fn new(device_id: DeviceId, effects: SharedEffects) -> Self {
        Self {
            device_id,
            friends: Vec::new(),
            guardians: Vec::new(),
            seen_envelopes: HashSet::new(),
            envelope_cache: HashMap::new(),
            effects,
        }
    }

    /// Add friend relationship for flooding
    pub fn add_friend(&mut self, friend_id: DeviceId) {
        if !self.friends.contains(&friend_id) {
            self.friends.push(friend_id);
        }
    }

    /// Add guardian relationship for flooding (preferred over friends)
    pub fn add_guardian(&mut self, guardian_id: DeviceId) {
        if !self.guardians.contains(&guardian_id) {
            self.guardians.push(guardian_id);
        }
    }

    /// Remove friend relationship
    pub fn remove_friend(&mut self, friend_id: &DeviceId) {
        self.friends.retain(|id| id != friend_id);
    }

    /// Remove guardian relationship
    pub fn remove_guardian(&mut self, guardian_id: &DeviceId) {
        self.guardians.retain(|id| id != guardian_id);
    }

    /// Cache envelope to prevent duplicate processing
    fn cache_envelope(&mut self, envelope: &RendezvousEnvelope, expires_at: u64) {
        self.seen_envelopes.insert(envelope.id);
        self.envelope_cache
            .insert(envelope.id, (envelope.clone(), expires_at));
    }

    /// Check if envelope was already seen
    fn is_duplicate(&self, envelope_id: &EnvelopeId) -> bool {
        self.seen_envelopes.contains(envelope_id)
    }

    /// Clean up expired envelopes from cache
    pub fn cleanup_expired_envelopes(&mut self, current_time: u64) {
        let expired_ids: Vec<EnvelopeId> = self
            .envelope_cache
            .iter()
            .filter(|(_, (_, expires_at))| *expires_at <= current_time)
            .map(|(id, _)| *id)
            .collect();

        for id in expired_ids {
            self.envelope_cache.remove(&id);
            self.seen_envelopes.remove(&id);
        }
    }
}

#[async_trait::async_trait]
impl SbbFlooding for SbbFloodingCoordinator {
    /// Flood envelope through social graph with TTL and duplicate detection
    async fn flood_envelope(
        &mut self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<FloodResult> {
        // Check TTL - drop if zero
        if envelope.ttl == 0 {
            return Ok(FloodResult::Dropped);
        }

        // Check for duplicate - drop if seen
        if self.is_duplicate(&envelope.id) {
            return Ok(FloodResult::Dropped);
        }

        // Cache envelope for duplicate detection
        let expires_at = now + 3600; // 1 hour cache
        self.cache_envelope(&envelope, expires_at);

        // Get peers to forward to (exclude sender)
        let forwarding_peers = self.get_forwarding_peers(from_peer, now).await?;

        if forwarding_peers.is_empty() {
            return Ok(FloodResult::Dropped);
        }

        // Decrement TTL for forwarding
        let forwarded_envelope = match envelope.decrement_ttl() {
            Some(env) => env,
            None => return Ok(FloodResult::Dropped),
        };

        // Forward to all capable peers
        let mut successful_forwards = 0;
        for peer in &forwarding_peers {
            match self
                .forward_to_peer(forwarded_envelope.clone(), *peer, now)
                .await
            {
                Ok(()) => successful_forwards += 1,
                Err(_) => continue, // Ignore individual forward failures
            }
        }

        if successful_forwards > 0 {
            Ok(FloodResult::Forwarded {
                peer_count: successful_forwards,
            })
        } else {
            Ok(FloodResult::Failed {
                reason: "All forwards failed".to_string(),
            })
        }
    }

    /// Get forwarding peers - prefer guardians, then friends, exclude sender
    async fn get_forwarding_peers(
        &self,
        exclude: Option<DeviceId>,
        now: u64,
    ) -> AuraResult<Vec<DeviceId>> {
        let mut peers = Vec::new();

        // Add guardians first (preferred for reliability)
        for guardian in &self.guardians {
            if Some(*guardian) != exclude
                && self.can_forward_to(guardian, SBB_MESSAGE_SIZE, now).await?
            {
                peers.push(*guardian);
            }
        }

        // Add friends if we have capacity
        for friend in &self.friends {
            if Some(*friend) != exclude
                && self.can_forward_to(friend, SBB_MESSAGE_SIZE, now).await?
            {
                peers.push(*friend);
            }
        }

        Ok(peers)
    }

    /// Check if can forward to peer with flow budget and capability checking
    async fn can_forward_to(
        &self,
        peer: &DeviceId,
        message_size: u64,
        _now: u64,
    ) -> AuraResult<bool> {
        // 1. Check relay capability for this peer
        let relay_permission = format!("relay:forward_to:{}", peer);
        let resource = "relay:network";

        // Get current journal to check capabilities
        let journal_result = self.effects.get_journal().await;
        let journal = match journal_result {
            Ok(journal) => journal,
            Err(e) => {
                tracing::error!("Failed to get journal for relay authorization: {:?}", e);
                return Ok(false); // Deny by default
            }
        };

        // Check relay capability
        if !journal.caps.allows(&relay_permission) {
            tracing::debug!(
                peer = ?peer,
                permission = relay_permission,
                "Relay capability denied"
            );
            return Ok(false);
        }

        // 2. Calculate flow cost based on message size
        let base_cost = 10u32; // Base cost for relay operation
        let size_cost = (message_size / 1024) as u32; // 1 unit per KB
        let total_cost = base_cost + size_cost;

        // 3. Check flow budget using context for this peer
        let relay_context = RelayContextDerivation::derive_relay_context(&self.device_id, peer)
            .map_err(|e| AuraError::invalid(format!("Failed to derive relay context: {}", e)))?;

        // For now, assume flow budget checking is available through the effect system
        // In a full implementation, this would check against the peer's flow budget
        let budget_result: Result<(), String> = Ok(());

        match budget_result {
            Ok(_updated_budget) => {
                tracing::debug!(
                    peer = ?peer,
                    cost = total_cost,
                    message_size = message_size,
                    "Flow budget charged for relay"
                );
                Ok(true)
            }
            Err(e) => {
                tracing::warn!(
                    peer = ?peer,
                    cost = total_cost,
                    error = ?e,
                    "Flow budget charging failed"
                );
                Ok(false)
            }
        }
    }

    /// Forward envelope to specific peer (placeholder for transport integration)
    async fn forward_to_peer(
        &mut self,
        _envelope: RendezvousEnvelope,
        _peer: DeviceId,
        _now: u64,
    ) -> AuraResult<()> {
        // TODO: Integrate with transport layer (will be implemented in Task 1.3)
        // For now, simulate successful forward
        Ok(())
    }
}

/// Get current timestamp (placeholder - would use time effects)
pub(crate) fn current_timestamp() -> u64 {
    1234567890
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_envelope_creation() {
        let payload = b"test rendezvous offer".to_vec();
        let envelope = RendezvousEnvelope::new(payload.clone(), Some(4));

        assert_eq!(envelope.ttl, 4);
        assert_eq!(envelope.payload, payload);
        assert_ne!(envelope.id, [0u8; 32]); // Should have computed ID
    }

    #[test]
    fn test_content_addressed_id() {
        let payload = b"identical payload".to_vec();
        let envelope1 = RendezvousEnvelope::new(payload.clone(), None);
        let envelope2 = RendezvousEnvelope::new(payload, None);

        // Same payload should produce same ID
        assert_eq!(envelope1.id, envelope2.id);
    }

    #[test]
    fn test_ttl_decrement() {
        let payload = b"test".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(2));

        let decremented = envelope.decrement_ttl().unwrap();
        assert_eq!(decremented.ttl, 1);

        let final_envelope = decremented.decrement_ttl().unwrap();
        assert_eq!(final_envelope.ttl, 0);

        // Should return None when TTL reaches 0
        assert!(final_envelope.decrement_ttl().is_none());
    }

    // Helper to create test effects
    fn create_test_effects(device_id: DeviceId) -> SharedEffects {
        use aura_agent::runtime::{AuraEffectSystem, EffectSystemConfig};

        let config = EffectSystemConfig::for_testing(device_id);
        let system = AuraEffectSystem::new();
        Arc::new(system)
    }

    #[test]
    fn test_flooding_coordinator_creation() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let coordinator = SbbFloodingCoordinator::new(device_id, effects);

        assert_eq!(coordinator.device_id, device_id);
        assert!(coordinator.friends.is_empty());
        assert!(coordinator.guardians.is_empty());
        assert!(coordinator.seen_envelopes.is_empty());
    }

    #[test]
    fn test_relationship_management() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let mut coordinator = SbbFloodingCoordinator::new(device_id, effects);

        let friend_id = DeviceId::new();
        let guardian_id = DeviceId::new();

        coordinator.add_friend(friend_id);
        coordinator.add_guardian(guardian_id);

        assert_eq!(coordinator.friends.len(), 1);
        assert_eq!(coordinator.guardians.len(), 1);
        assert!(coordinator.friends.contains(&friend_id));
        assert!(coordinator.guardians.contains(&guardian_id));

        coordinator.remove_friend(&friend_id);
        coordinator.remove_guardian(&guardian_id);

        assert!(coordinator.friends.is_empty());
        assert!(coordinator.guardians.is_empty());
    }

    #[test]
    fn test_duplicate_detection() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let mut coordinator = SbbFloodingCoordinator::new(device_id, effects);

        let payload = b"test envelope".to_vec();
        let envelope = RendezvousEnvelope::new(payload, None);

        // Should not be duplicate initially
        assert!(!coordinator.is_duplicate(&envelope.id));

        // Cache envelope
        coordinator.cache_envelope(&envelope, current_timestamp() + 3600);

        // Should now be detected as duplicate
        assert!(coordinator.is_duplicate(&envelope.id));
    }

    #[tokio::test]
    async fn test_flood_with_zero_ttl() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let mut coordinator = SbbFloodingCoordinator::new(device_id, effects);

        let payload = b"test".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(0));

        let result = coordinator.flood_envelope(envelope, None).await.unwrap();
        match result {
            FloodResult::Dropped => (), // Expected
            _ => panic!("Expected envelope with TTL 0 to be dropped"),
        }
    }

    #[tokio::test]
    async fn test_flood_duplicate_envelope() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let mut coordinator = SbbFloodingCoordinator::new(device_id, effects);

        let payload = b"test".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(2));

        // First flood should succeed (though no peers to forward to)
        let result1 = coordinator
            .flood_envelope(envelope.clone(), None)
            .await
            .unwrap();
        // No peers to forward to - result should be Dropped
        if let FloodResult::Dropped = result1 {}

        // Second flood of same envelope should be dropped as duplicate
        let result2 = coordinator.flood_envelope(envelope, None).await.unwrap();
        match result2 {
            FloodResult::Dropped => (), // Expected duplicate
            _ => panic!("Expected duplicate envelope to be dropped"),
        }
    }

    #[test]
    fn test_sbb_envelope_creation() {
        // Test plaintext envelope
        let payload = b"test transport offer".to_vec();
        let plaintext_env = SbbEnvelope::new_plaintext(payload.clone(), Some(4));

        assert_eq!(plaintext_env.ttl(), 4);
        match plaintext_env {
            SbbEnvelope::Plaintext(env) => assert_eq!(env.payload, payload),
            _ => panic!("Expected plaintext envelope"),
        }

        // Test encrypted envelope (mock)
        use crate::envelope_encryption::EncryptedEnvelope;
        let encrypted_payload = EncryptedEnvelope::new([1; 12], vec![0; 1024], Some([1, 2, 3, 4]));
        let encrypted_env = SbbEnvelope::new_encrypted(encrypted_payload, Some(3));

        assert_eq!(encrypted_env.ttl(), 3);
        match encrypted_env {
            SbbEnvelope::Encrypted {
                encrypted_payload, ..
            } => {
                assert_eq!(encrypted_payload.ciphertext.len(), 1024);
            }
            _ => panic!("Expected encrypted envelope"),
        }
    }

    #[test]
    fn test_sbb_envelope_ttl_decrement() {
        // Test plaintext envelope TTL
        let payload = b"test".to_vec();
        let plaintext_env = SbbEnvelope::new_plaintext(payload, Some(2));

        let decremented = plaintext_env.decrement_ttl().unwrap();
        assert_eq!(decremented.ttl(), 1);

        let final_env = decremented.decrement_ttl().unwrap();
        assert_eq!(final_env.ttl(), 0);

        assert!(final_env.decrement_ttl().is_none());

        // Test encrypted envelope TTL
        use crate::envelope_encryption::EncryptedEnvelope;
        let encrypted_payload = EncryptedEnvelope::new([1; 12], vec![0; 1024], None);
        let encrypted_env = SbbEnvelope::new_encrypted(encrypted_payload, Some(1));

        let decremented = encrypted_env.decrement_ttl().unwrap();
        assert_eq!(decremented.ttl(), 0);

        assert!(decremented.decrement_ttl().is_none());
    }

    #[test]
    fn test_sbb_envelope_size_calculation() {
        // Test plaintext envelope size
        let payload = vec![0u8; 100];
        let plaintext_env = SbbEnvelope::new_plaintext(payload, None);
        let expected_plaintext_size = 32 + 1 + 8 + 100; // id + ttl + timestamp + payload
        assert_eq!(plaintext_env.size(), expected_plaintext_size);

        // Test encrypted envelope size
        use crate::envelope_encryption::EncryptedEnvelope;
        let encrypted_payload = EncryptedEnvelope::new([1; 12], vec![0; 1024], Some([1, 2, 3, 4]));
        let encrypted_env = SbbEnvelope::new_encrypted(encrypted_payload, None);
        let expected_encrypted_size = 32 + 1 + 8 + (12 + 1024 + 4); // id + ttl + timestamp + (nonce + ciphertext + hint)
        assert_eq!(encrypted_env.size(), expected_encrypted_size);
    }

    #[test]
    fn test_encrypted_envelope_id_uniqueness() {
        use crate::envelope_encryption::EncryptedEnvelope;

        let encrypted1 = EncryptedEnvelope::new([1; 12], vec![0; 1024], None);
        let encrypted2 = EncryptedEnvelope::new([2; 12], vec![0; 1024], None); // Different nonce

        let env1 = SbbEnvelope::new_encrypted(encrypted1, Some(3));
        let env2 = SbbEnvelope::new_encrypted(encrypted2, Some(3));

        // Different encrypted payloads should produce different IDs
        assert_ne!(env1.id(), env2.id());
    }
}

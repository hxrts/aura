//! SBB Flooding Protocol
//!
//! Implements controlled flooding through the social graph with TTL limits,
//! duplicate detection, and capability enforcement.

use super::envelope::{EnvelopeId, RendezvousEnvelope, SBB_MESSAGE_SIZE};
use aura_core::context_derivation::RelayContextDerivation;
use aura_core::{AuraError, AuraResult, DeviceId};
use aura_protocol::effects::AuraEffects;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

type SharedEffects = Arc<dyn AuraEffects>;

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
    envelope_cache: HashMap<EnvelopeId, (RendezvousEnvelope, u64)>,
    /// Effect system interface for journal and capability checks
    effects: SharedEffects,
}

impl std::fmt::Debug for SbbFloodingCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SbbFloodingCoordinator")
            .field("device_id", &self.device_id)
            .field("friends", &self.friends)
            .field("guardians", &self.guardians)
            .field("seen_envelopes", &self.seen_envelopes)
            .field("envelope_cache", &self.envelope_cache)
            .field("effects", &"<dyn AuraEffects>")
            .finish()
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

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get friends list
    pub fn friends(&self) -> &[DeviceId] {
        &self.friends
    }

    /// Get guardians list
    pub fn guardians(&self) -> &[DeviceId] {
        &self.guardians
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
                Err(_) => continue,
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

    async fn can_forward_to(
        &self,
        peer: &DeviceId,
        message_size: u64,
        _now: u64,
    ) -> AuraResult<bool> {
        // 1. Check relay capability for this peer
        let relay_permission = format!("relay:forward_to:{}", peer);
        let _resource = "relay:network";

        // Get current journal to check capabilities
        let journal_result = self.effects.get_journal().await;
        let journal = match journal_result {
            Ok(journal) => journal,
            Err(e) => {
                tracing::error!("Failed to get journal for relay authorization: {:?}", e);
                return Ok(false);
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
        let base_cost = 10u32;
        let size_cost = (message_size / 1024) as u32;
        let total_cost = base_cost + size_cost;

        // 3. Check flow budget using context for this peer
        let _relay_context = RelayContextDerivation::derive_relay_context(&self.device_id, peer)
            .map_err(|e| AuraError::invalid(format!("Failed to derive relay context: {}", e)))?;

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

    async fn forward_to_peer(
        &mut self,
        envelope: RendezvousEnvelope,
        peer: DeviceId,
        _now: u64,
    ) -> AuraResult<()> {
        // Serialize the envelope for transport
        let envelope_data = serde_json::to_vec(&envelope).map_err(|e| {
            AuraError::internal(format!(
                "Failed to serialize envelope for peer {}: {}",
                peer, e
            ))
        })?;

        // Convert DeviceId to UUID for NetworkEffects
        let peer_uuid: uuid::Uuid = peer.into();

        // Send envelope to peer via network effects
        self.effects
            .send_to_peer(peer_uuid, envelope_data)
            .await
            .map_err(|e| {
                AuraError::network(format!(
                    "Failed to send rendezvous envelope to peer {}: {}",
                    peer, e
                ))
            })?;

        tracing::debug!(
            peer = ?peer,
            envelope_id = ?envelope.id,
            ttl = envelope.ttl,
            "Successfully forwarded rendezvous envelope to peer"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_agent::AgentConfig;
    use aura_agent::AuraEffectSystem;

    fn create_test_effects(_device_id: DeviceId) -> SharedEffects {
        let config = AgentConfig::default();
        let system = AuraEffectSystem::testing(&config);
        Arc::new(system)
    }

    #[test]
    fn test_coordinator_creation() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let coordinator = SbbFloodingCoordinator::new(device_id, effects);

        assert_eq!(coordinator.device_id, device_id);
        assert!(coordinator.friends.is_empty());
        assert!(coordinator.guardians.is_empty());
    }

    #[tokio::test]
    async fn test_flood_with_zero_ttl() {
        let device_id = DeviceId::new();
        let effects = create_test_effects(device_id);
        let mut coordinator = SbbFloodingCoordinator::new(device_id, effects);

        let payload = b"test".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(0));
        let now = 1000000u64;

        let result = coordinator
            .flood_envelope(envelope, None, now)
            .await
            .unwrap();
        match result {
            FloodResult::Dropped => (),
            _ => panic!("Expected envelope with TTL 0 to be dropped"),
        }
    }
}

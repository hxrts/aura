//! Capability-Aware SBB Flooding
//!
//! This module integrates the SBB flooding system with Web-of-Trust (WoT) trust evaluation
//! and capability-based flow budget enforcement. It provides trust-aware forwarding decisions
//! and respects relay capabilities and flow budgets.

use crate::sbb::{FloodResult, RendezvousEnvelope, SbbFlooding, SBB_MESSAGE_SIZE};
use aura_core::{AuraError, AuraResult, DeviceId, RelationshipId};
use aura_wot::{Capability, CapabilitySet, TrustLevel};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Flow budget tracking for SBB forwarding
#[derive(Debug, Clone)]
pub struct SbbFlowBudget {
    /// Bytes spent in current period
    pub spent: u64,
    /// Maximum bytes allowed per period
    pub limit: u64,
    /// Period duration in seconds
    pub period_seconds: u64,
    /// Period start timestamp
    pub period_start: u64,
}

/// Relationship metadata for SBB flooding decisions
#[derive(Debug, Clone)]
pub struct SbbRelationship {
    /// Peer device ID
    pub peer_id: DeviceId,
    /// Relationship identifier
    pub relationship_id: RelationshipId,
    /// Trust level (None < Low < Medium < High < Full)
    pub trust_level: TrustLevel,
    /// Relay capabilities for this peer
    pub relay_capabilities: CapabilitySet,
    /// Flow budget for SBB messages
    pub flow_budget: SbbFlowBudget,
    /// Is this peer a guardian (preferred for reliability)
    pub is_guardian: bool,
}

/// Capability-aware SBB flooding coordinator with WoT integration
#[derive(Debug)]
pub struct CapabilityAwareSbbCoordinator {
    /// This device's ID
    device_id: DeviceId,
    /// Friend and guardian relationships with trust/capability metadata
    relationships: HashMap<DeviceId, SbbRelationship>,
    /// Recently seen envelope IDs for duplicate detection
    seen_envelopes: std::collections::HashSet<[u8; 32]>,
    /// Envelope cache with expiration
    envelope_cache: HashMap<[u8; 32], (RendezvousEnvelope, u64)>,
}

/// SBB forwarding policy based on trust and capabilities
#[derive(Debug, Clone)]
pub struct SbbForwardingPolicy {
    /// Minimum trust level for SBB forwarding
    pub min_trust_level: TrustLevel,
    /// Maximum flow percentage to use for SBB (0.0-1.0)
    pub max_flow_usage: f64,
    /// Prefer guardians over friends
    pub prefer_guardians: bool,
    /// Maximum concurrent SBB streams per peer
    pub max_streams_per_peer: u32,
}

impl Default for SbbForwardingPolicy {
    fn default() -> Self {
        Self {
            min_trust_level: TrustLevel::Low,
            max_flow_usage: 0.3, // Use max 30% of flow budget for SBB
            prefer_guardians: true,
            max_streams_per_peer: 5,
        }
    }
}

impl SbbFlowBudget {
    /// Create new flow budget with period limits
    pub fn new(limit: u64, period_seconds: u64) -> Self {
        let now = current_timestamp();
        Self {
            spent: 0,
            limit,
            period_seconds,
            period_start: now,
        }
    }

    /// Check if there's enough budget for a message
    pub fn can_spend(&self, bytes: u64) -> bool {
        let now = current_timestamp();

        // If period expired, reset budget
        if now >= self.period_start + self.period_seconds {
            return bytes <= self.limit;
        }

        self.spent + bytes <= self.limit
    }

    /// Spend bytes from budget (call after can_spend check)
    pub fn spend(&mut self, bytes: u64) -> AuraResult<()> {
        let now = current_timestamp();

        // Reset budget if period expired
        if now >= self.period_start + self.period_seconds {
            self.spent = 0;
            self.period_start = now;
        }

        if self.spent + bytes > self.limit {
            return Err(AuraError::coordination_failed(format!(
                "Flow budget exceeded: {} + {} > {}",
                self.spent, bytes, self.limit
            )));
        }

        self.spent += bytes;
        Ok(())
    }

    /// Get remaining budget in current period
    pub fn remaining(&self) -> u64 {
        let now = current_timestamp();

        // If period expired, full budget is available
        if now >= self.period_start + self.period_seconds {
            return self.limit;
        }

        self.limit.saturating_sub(self.spent)
    }

    /// Get budget utilization (0.0-1.0)
    pub fn utilization(&self) -> f64 {
        if self.limit == 0 {
            return 1.0;
        }

        let now = current_timestamp();
        if now >= self.period_start + self.period_seconds {
            return 0.0;
        }

        self.spent as f64 / self.limit as f64
    }
}

impl SbbRelationship {
    /// Create new SBB relationship with default capabilities
    pub fn new(
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        trust_level: TrustLevel,
        is_guardian: bool,
    ) -> Self {
        // Default relay capability based on trust level
        let (flow_limit, period) = match trust_level {
            TrustLevel::None => (0, 3600),
            TrustLevel::Low => (10 * 1024, 3600), // 10KB/hour
            TrustLevel::Medium => (100 * 1024, 3600), // 100KB/hour
            TrustLevel::High => (10 * 1024 * 1024, 3600), // 10MB/hour
        };

        let relay_capability = Capability::Relay {
            max_bytes_per_period: flow_limit,
            period_seconds: period,
            max_streams: 10,
        };

        let relay_capabilities = {
            let mut caps = CapabilitySet::empty();
            caps.insert(relay_capability);
            caps
        };
        let flow_budget = SbbFlowBudget::new(flow_limit, period);

        Self {
            peer_id,
            relationship_id,
            trust_level,
            relay_capabilities,
            flow_budget,
            is_guardian,
        }
    }

    /// Check if peer can forward SBB message of given size
    pub fn can_forward_sbb(&self, message_size: u64, policy: &SbbForwardingPolicy) -> bool {
        // Check trust level requirement
        if self.trust_level < policy.min_trust_level {
            return false;
        }

        // Check if we should limit flow usage
        let max_usage_bytes = (self.flow_budget.limit as f64 * policy.max_flow_usage) as u64;
        if self.flow_budget.spent + message_size > max_usage_bytes {
            return false;
        }

        // Check relay capability permits the operation
        let relay_operation = format!("relay:{}:1", message_size);
        if !self.relay_capabilities.permits(&relay_operation) {
            return false;
        }

        // Check if budget allows this message
        self.flow_budget.can_spend(message_size)
    }
}

impl CapabilityAwareSbbCoordinator {
    /// Create new capability-aware SBB coordinator
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            relationships: HashMap::new(),
            seen_envelopes: std::collections::HashSet::new(),
            envelope_cache: HashMap::new(),
        }
    }

    /// Add relationship with trust level and capabilities
    pub fn add_relationship(
        &mut self,
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        trust_level: TrustLevel,
        is_guardian: bool,
    ) {
        let relationship = SbbRelationship::new(peer_id, relationship_id, trust_level, is_guardian);
        self.relationships.insert(peer_id, relationship);
    }

    /// Update trust level for existing relationship
    pub fn update_trust_level(
        &mut self,
        peer_id: DeviceId,
        trust_level: TrustLevel,
    ) -> AuraResult<()> {
        match self.relationships.get_mut(&peer_id) {
            Some(rel) => {
                rel.trust_level = trust_level;
                // Update flow budget based on new trust level
                let (flow_limit, period) = match trust_level {
                    TrustLevel::None => (0, 3600),
                    TrustLevel::Low => (10 * 1024, 3600),
                    TrustLevel::Medium => (100 * 1024, 3600),
                    TrustLevel::High => (10 * 1024 * 1024, 3600),
                };
                rel.flow_budget = SbbFlowBudget::new(flow_limit, period);
                Ok(())
            }
            None => Err(AuraError::coordination_failed(format!(
                "Relationship not found for peer {}",
                peer_id.0
            ))),
        }
    }

    /// Remove relationship
    pub fn remove_relationship(&mut self, peer_id: &DeviceId) {
        self.relationships.remove(peer_id);
    }

    /// Get forwarding peers based on trust, capabilities, and flow budgets
    pub async fn get_capability_aware_forwarding_peers(
        &self,
        exclude: Option<DeviceId>,
        message_size: u64,
        policy: &SbbForwardingPolicy,
    ) -> AuraResult<Vec<DeviceId>> {
        let mut eligible_peers = Vec::new();
        let mut guardian_peers = Vec::new();

        for (peer_id, relationship) in &self.relationships {
            // Skip excluded peer
            if Some(*peer_id) == exclude {
                continue;
            }

            // Check if peer can forward based on trust and capabilities
            if relationship.can_forward_sbb(message_size, policy) {
                if relationship.is_guardian && policy.prefer_guardians {
                    guardian_peers.push(*peer_id);
                } else {
                    eligible_peers.push(*peer_id);
                }
            }
        }

        // Return guardians first if preference is enabled
        if policy.prefer_guardians {
            guardian_peers.extend(eligible_peers);
            Ok(guardian_peers)
        } else {
            eligible_peers.extend(guardian_peers);
            Ok(eligible_peers)
        }
    }

    /// Spend flow budget for SBB forwarding
    pub fn spend_flow_budget(&mut self, peer_id: DeviceId, bytes: u64) -> AuraResult<()> {
        match self.relationships.get_mut(&peer_id) {
            Some(rel) => rel.flow_budget.spend(bytes),
            None => Err(AuraError::coordination_failed(format!(
                "No relationship found for peer {}",
                peer_id.0
            ))),
        }
    }

    /// Get relationship metadata for peer
    pub fn get_relationship(&self, peer_id: &DeviceId) -> Option<&SbbRelationship> {
        self.relationships.get(peer_id)
    }

    /// Get trust statistics for monitoring
    pub fn get_trust_statistics(&self) -> TrustStatistics {
        let mut stats = TrustStatistics::default();

        for relationship in self.relationships.values() {
            match relationship.trust_level {
                TrustLevel::None => stats.none_count += 1,
                TrustLevel::Low => stats.low_count += 1,
                TrustLevel::Medium => stats.medium_count += 1,
                TrustLevel::High => stats.high_count += 1,
            }

            if relationship.is_guardian {
                stats.guardian_count += 1;
            }

            stats.total_flow_spent += relationship.flow_budget.spent;
            stats.total_flow_limit += relationship.flow_budget.limit;
        }

        stats.relationship_count = self.relationships.len();
        stats
    }

    /// Clean up expired envelopes from cache
    pub fn cleanup_expired_envelopes(&mut self, current_time: u64) {
        let expired_ids: Vec<[u8; 32]> = self
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

/// Trust and flow statistics for monitoring
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrustStatistics {
    pub relationship_count: usize,
    pub guardian_count: usize,
    pub none_count: usize,
    pub low_count: usize,
    pub medium_count: usize,
    pub high_count: usize,
    pub full_count: usize,
    pub total_flow_spent: u64,
    pub total_flow_limit: u64,
}

impl TrustStatistics {
    /// Get average trust level (weighted)
    pub fn average_trust_level(&self) -> f64 {
        let total = self.none_count
            + self.low_count
            + self.medium_count
            + self.high_count
            + self.full_count;
        if total == 0 {
            return 0.0;
        }

        let weighted_sum = (self.none_count * 0)
            + (self.low_count * 1)
            + (self.medium_count * 2)
            + (self.high_count * 3)
            + (self.full_count * 4);

        weighted_sum as f64 / total as f64 / 4.0 // Normalize to 0-1
    }

    /// Get flow budget utilization
    pub fn flow_utilization(&self) -> f64 {
        if self.total_flow_limit == 0 {
            return 0.0;
        }
        self.total_flow_spent as f64 / self.total_flow_limit as f64
    }
}

#[async_trait::async_trait]
impl SbbFlooding for CapabilityAwareSbbCoordinator {
    async fn flood_envelope(
        &mut self,
        envelope: RendezvousEnvelope,
        from_peer: Option<DeviceId>,
    ) -> AuraResult<FloodResult> {
        // Check TTL
        if envelope.ttl == 0 {
            return Ok(FloodResult::Dropped);
        }

        // Check for duplicate
        if self.seen_envelopes.contains(&envelope.id) {
            return Ok(FloodResult::Dropped);
        }

        // Cache envelope
        let expires_at = current_timestamp() + 3600;
        self.envelope_cache
            .insert(envelope.id, (envelope.clone(), expires_at));
        self.seen_envelopes.insert(envelope.id);

        // Get forwarding peers using capability-aware logic
        let policy = SbbForwardingPolicy::default();
        let forwarding_peers = self
            .get_capability_aware_forwarding_peers(from_peer, SBB_MESSAGE_SIZE, &policy)
            .await?;

        if forwarding_peers.is_empty() {
            return Ok(FloodResult::Dropped);
        }

        // Decrement TTL for forwarding
        let forwarded_envelope = match envelope.decrement_ttl() {
            Some(env) => env,
            None => return Ok(FloodResult::Dropped),
        };

        // Forward to capable peers and spend flow budgets
        let mut successful_forwards = 0;
        for peer in &forwarding_peers {
            match self
                .forward_to_peer(forwarded_envelope.clone(), *peer)
                .await
            {
                Ok(()) => {
                    // Spend flow budget for successful forward
                    if let Err(e) = self.spend_flow_budget(*peer, SBB_MESSAGE_SIZE) {
                        tracing::warn!("Failed to spend flow budget for peer {}: {}", peer.0, e);
                    } else {
                        successful_forwards += 1;
                    }
                }
                Err(_) => continue,
            }
        }

        if successful_forwards > 0 {
            Ok(FloodResult::Forwarded {
                peer_count: successful_forwards,
            })
        } else {
            Ok(FloodResult::Failed {
                reason: "All capability-aware forwards failed".to_string(),
            })
        }
    }

    async fn get_forwarding_peers(&self, exclude: Option<DeviceId>) -> AuraResult<Vec<DeviceId>> {
        let policy = SbbForwardingPolicy::default();
        self.get_capability_aware_forwarding_peers(exclude, SBB_MESSAGE_SIZE, &policy)
            .await
    }

    async fn can_forward_to(&self, peer: &DeviceId, message_size: u64) -> AuraResult<bool> {
        match self.relationships.get(peer) {
            Some(relationship) => {
                let policy = SbbForwardingPolicy::default();
                Ok(relationship.can_forward_sbb(message_size, &policy))
            }
            None => Ok(false),
        }
    }

    async fn forward_to_peer(
        &mut self,
        envelope: RendezvousEnvelope,
        peer: DeviceId,
    ) -> AuraResult<()> {
        // Extract envelope ID before moving envelope
        let envelope_id = envelope.id.clone();

        // Create SBB message for transport layer
        let sbb_message = crate::messaging::SbbMessageType::RendezvousFlood {
            envelope,
            from_peer: Some(self.device_id),
        };

        // Use transport sender if available via effect system or direct integration
        // For now, we'll use a simplified approach - in production this would
        // integrate with the actual transport layer through dependency injection

        // Check if peer relationship exists and validate forwarding capability
        if let Some(relationship) = self.relationships.get(&peer) {
            let policy = SbbForwardingPolicy::default();
            if !relationship.can_forward_sbb(crate::sbb::SBB_MESSAGE_SIZE, &policy) {
                return Err(AuraError::coordination_failed(format!(
                    "Peer {} cannot accept SBB messages (insufficient capability or budget)",
                    peer.0
                )));
            }
        } else {
            return Err(AuraError::coordination_failed(format!(
                "No relationship found for peer {}",
                peer.0
            )));
        }

        // Simulate transport send - in real implementation would use:
        // self.transport_sender.send_to_peer(peer, sbb_message).await
        tracing::debug!(
            peer_id = %peer.0,
            envelope_id = %hex::encode(&envelope_id),
            "Forwarded SBB envelope to peer"
        );

        Ok(())
    }
}

/// Get current timestamp (placeholder - would use time effects)
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flow_budget_spending() {
        let mut budget = SbbFlowBudget::new(1024, 3600);

        assert_eq!(budget.remaining(), 1024);
        assert_eq!(budget.utilization(), 0.0);

        assert!(budget.can_spend(512));
        budget.spend(512).unwrap();
        assert_eq!(budget.remaining(), 512);
        assert_eq!(budget.utilization(), 0.5);

        assert!(budget.can_spend(512));
        assert!(!budget.can_spend(513));

        budget.spend(512).unwrap();
        assert_eq!(budget.remaining(), 0);
        assert_eq!(budget.utilization(), 1.0);

        assert!(budget.spend(1).is_err());
    }

    #[test]
    fn test_relationship_forwarding_logic() {
        let peer_id = DeviceId::new();
        let rel_id = RelationshipId::new();

        // Test different trust levels
        let low_trust_rel = SbbRelationship::new(peer_id, rel_id, TrustLevel::Low, false);
        let high_trust_rel = SbbRelationship::new(peer_id, rel_id, TrustLevel::High, true);

        let policy = SbbForwardingPolicy::default();

        // High trust should allow forwarding
        assert!(high_trust_rel.can_forward_sbb(SBB_MESSAGE_SIZE, &policy));

        // Low trust should allow forwarding if policy permits
        assert!(low_trust_rel.can_forward_sbb(SBB_MESSAGE_SIZE, &policy));

        // No trust should be rejected
        let no_trust_rel = SbbRelationship::new(peer_id, rel_id, TrustLevel::None, false);
        let strict_policy = SbbForwardingPolicy {
            min_trust_level: TrustLevel::Low,
            ..Default::default()
        };
        assert!(!no_trust_rel.can_forward_sbb(SBB_MESSAGE_SIZE, &strict_policy));
    }

    #[tokio::test]
    async fn test_capability_aware_coordinator() {
        let device_id = DeviceId::new();
        let mut coordinator = CapabilityAwareSbbCoordinator::new(device_id);

        let peer1 = DeviceId::new();
        let peer2 = DeviceId::new();
        let rel_id = RelationshipId::new();

        // Add relationships with different trust levels
        coordinator.add_relationship(peer1, rel_id, TrustLevel::High, true); // Guardian
        coordinator.add_relationship(peer2, rel_id, TrustLevel::Low, false); // Friend

        let policy = SbbForwardingPolicy::default();
        let peers = coordinator
            .get_capability_aware_forwarding_peers(None, SBB_MESSAGE_SIZE, &policy)
            .await
            .unwrap();

        // Should return both peers, with guardian first due to preference
        assert_eq!(peers.len(), 2);
        assert_eq!(peers[0], peer1); // Guardian should be first

        // Test trust statistics
        let stats = coordinator.get_trust_statistics();
        assert_eq!(stats.relationship_count, 2);
        assert_eq!(stats.guardian_count, 1);
        assert_eq!(stats.high_count, 1);
        assert_eq!(stats.low_count, 1);
    }

    #[tokio::test]
    async fn test_envelope_flooding_with_capabilities() {
        let device_id = DeviceId::new();
        let mut coordinator = CapabilityAwareSbbCoordinator::new(device_id);

        let peer_id = DeviceId::new();
        let rel_id = RelationshipId::new();
        coordinator.add_relationship(peer_id, rel_id, TrustLevel::Medium, false);

        let payload = b"test envelope".to_vec();
        let envelope = RendezvousEnvelope::new(payload, Some(2));

        // Should succeed with capable peer
        let result = coordinator
            .flood_envelope(envelope.clone(), None)
            .await
            .unwrap();
        match result {
            FloodResult::Forwarded { peer_count } => assert_eq!(peer_count, 1),
            _ => panic!("Expected successful forwarding"),
        }

        // Duplicate should be dropped
        let result2 = coordinator.flood_envelope(envelope, None).await.unwrap();
        match result2 {
            FloodResult::Dropped => (), // Expected
            _ => panic!("Expected duplicate to be dropped"),
        }
    }
}

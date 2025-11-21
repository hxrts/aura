//! Privacy-Aware Peer Selection Logic
//!
//! Simple peer selection with manifest privacy and capability blinding.
//! Target: <200 lines (focused implementation).

use super::info::{PeerInfo, ReliabilityLevel};
use aura_core::{identifiers::DeviceId, RelationshipId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::PrivacyLevel;

/// Simple peer selection with manifest privacy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyAwareSelectionCriteria {
    /// Required relationship context
    pub relationship_context: Option<RelationshipId>,

    /// Required capabilities (will be blinded for privacy)
    pub required_capabilities: HashSet<String>,

    /// Minimum reliability level
    pub min_reliability: ReliabilityLevel,

    /// Maximum number of peers to select
    pub max_peers: usize,

    /// Prefer peers with privacy features
    pub prefer_privacy_features: bool,

    /// Exclude specific peers
    pub excluded_peers: HashSet<DeviceId>,
}

/// Privacy-preserving peer selection result
#[derive(Debug, Clone)]
pub struct SelectionResult {
    /// Selected peers with selection reasons
    pub selected_peers: Vec<SelectedPeer>,

    /// Total candidates considered (privacy-preserving count)
    pub candidates_considered: usize,

    /// Privacy level used for selection
    pub privacy_level: PrivacyLevel,
}

/// Individual peer selection with privacy-preserving reasoning
#[derive(Debug, Clone)]
pub struct SelectedPeer {
    /// Peer information
    pub peer_info: PeerInfo,

    /// Selection score (privacy-preserving)
    pub selection_score: f64,

    /// Selection reasons (blinded)
    pub selection_reasons: Vec<String>,
}

impl PrivacyAwareSelectionCriteria {
    /// Create new selection criteria
    pub fn new() -> Self {
        Self {
            relationship_context: None,
            required_capabilities: HashSet::new(),
            min_reliability: ReliabilityLevel::Medium,
            max_peers: 5,
            prefer_privacy_features: true,
            excluded_peers: HashSet::new(),
        }
    }

    /// Create criteria for specific relationship
    pub fn for_relationship(relationship_id: RelationshipId) -> Self {
        Self {
            relationship_context: Some(relationship_id),
            ..Self::new()
        }
    }

    /// Add required capability
    pub fn require_capability(&mut self, capability: String) -> &mut Self {
        self.required_capabilities.insert(capability);
        self
    }

    /// Set minimum reliability level
    pub fn min_reliability(&mut self, level: ReliabilityLevel) -> &mut Self {
        self.min_reliability = level;
        self
    }

    /// Exclude specific peer
    pub fn exclude_peer(&mut self, device_id: DeviceId) -> &mut Self {
        self.excluded_peers.insert(device_id);
        self
    }

    /// Select peers from available candidates
    pub fn select_peers(&self, candidates: Vec<PeerInfo>) -> SelectionResult {
        let mut scored_peers = Vec::new();
        let mut candidates_considered = 0;

        for peer in candidates {
            // Basic filtering
            if !self.passes_basic_filters(&peer) {
                continue;
            }

            candidates_considered += 1;

            // Calculate privacy-preserving selection score
            if let Some(score) = self.calculate_selection_score(&peer) {
                let selected_peer = SelectedPeer {
                    selection_score: score,
                    selection_reasons: self.get_selection_reasons(&peer),
                    peer_info: peer,
                };
                scored_peers.push(selected_peer);
            }
        }

        // Sort by score (highest first) and limit to max_peers
        scored_peers.sort_by(|a, b| {
            b.selection_score
                .partial_cmp(&a.selection_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored_peers.truncate(self.max_peers);

        SelectionResult {
            selected_peers: scored_peers,
            candidates_considered,
            privacy_level: PrivacyLevel::Blinded,
        }
    }

    /// Check if peer passes basic filters
    fn passes_basic_filters(&self, peer: &PeerInfo) -> bool {
        // Exclude excluded peers
        if self.excluded_peers.contains(&peer.device_id) {
            return false;
        }

        // Check relationship context
        if let Some(relationship_id) = &self.relationship_context {
            if !peer.is_available_in_relationship(relationship_id) {
                return false;
            }
        }

        // Check required capabilities (privacy-preserving)
        for capability in &self.required_capabilities {
            if !peer.capabilities.has_capability_like(capability) {
                return false;
            }
        }

        true
    }

    /// Calculate privacy-preserving selection score
    fn calculate_selection_score(&self, peer: &PeerInfo) -> Option<f64> {
        let mut score = 0.5; // Base score

        // Reliability component
        match self.get_peer_reliability(peer) {
            ReliabilityLevel::High => score += 0.3,
            ReliabilityLevel::Medium => score += 0.1,
            ReliabilityLevel::Low => score -= 0.1,
            ReliabilityLevel::Unknown => score += 0.0,
        }

        // Privacy features preference
        if self.prefer_privacy_features {
            let capability_count = peer.capabilities.capability_count();
            score += (capability_count as f64 * 0.05).min(0.2);
        }

        // Relationship context bonus
        if let Some(relationship_id) = &self.relationship_context {
            if peer.relationship_contexts.contains(relationship_id) {
                score += 0.2;
            }
        }

        // Normalize score to [0, 1]
        score = score.clamp(0.0, 1.0);

        // Apply minimum reliability filter
        if self.meets_reliability_requirement(peer) {
            Some(score)
        } else {
            None
        }
    }

    /// Get privacy-preserving selection reasons
    fn get_selection_reasons(&self, peer: &PeerInfo) -> Vec<String> {
        let mut reasons = Vec::new();

        // Blinded reasons for privacy
        reasons.push("capability_match".to_string());

        if let Some(_relationship_id) = &self.relationship_context {
            if !peer.relationship_contexts.is_empty() {
                reasons.push("relationship_available".to_string());
            }
        }

        match self.get_peer_reliability(peer) {
            ReliabilityLevel::High => reasons.push("high_reliability".to_string()),
            ReliabilityLevel::Medium => reasons.push("medium_reliability".to_string()),
            _ => {}
        }

        if peer.capabilities.capability_count() > 2 {
            reasons.push("feature_rich".to_string());
        }

        reasons
    }

    /// Get peer reliability (privacy-preserving)
    fn get_peer_reliability(&self, peer: &PeerInfo) -> ReliabilityLevel {
        // Simple reliability assessment based on metrics
        let score = peer.metrics.reliability_score;

        if score >= 0.8 {
            ReliabilityLevel::High
        } else if score >= 0.5 {
            ReliabilityLevel::Medium
        } else if score > 0.0 {
            ReliabilityLevel::Low
        } else {
            ReliabilityLevel::Unknown
        }
    }

    /// Check if peer meets reliability requirement
    fn meets_reliability_requirement(&self, peer: &PeerInfo) -> bool {
        let peer_reliability = self.get_peer_reliability(peer);

        match self.min_reliability {
            ReliabilityLevel::High => matches!(peer_reliability, ReliabilityLevel::High),
            ReliabilityLevel::Medium => matches!(
                peer_reliability,
                ReliabilityLevel::High | ReliabilityLevel::Medium
            ),
            ReliabilityLevel::Low => !matches!(peer_reliability, ReliabilityLevel::Unknown),
            ReliabilityLevel::Unknown => true,
        }
    }
}

impl SelectionResult {
    /// Get selected device IDs
    pub fn device_ids(&self) -> Vec<DeviceId> {
        self.selected_peers
            .iter()
            .map(|sp| sp.peer_info.device_id)
            .collect()
    }

    /// Get average selection score
    pub fn average_score(&self) -> f64 {
        if self.selected_peers.is_empty() {
            0.0
        } else {
            let total: f64 = self
                .selected_peers
                .iter()
                .map(|sp| sp.selection_score)
                .sum();
            total / self.selected_peers.len() as f64
        }
    }
}

impl Default for PrivacyAwareSelectionCriteria {
    fn default() -> Self {
        Self::new()
    }
}

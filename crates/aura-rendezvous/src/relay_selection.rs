//! Relay Selection Heuristic
//!
//! Implements capability-aware relay selection with preference for guardians over friends.
//! Clean, minimal implementation following "zero legacy code" principle.

use aura_core::{AuraError, DeviceId, TrustLevel};
use aura_wot::CapabilitySet;
use serde::{Deserialize, Serialize};

/// Relay selection result
#[derive(Debug, Clone)]
pub struct RelaySelectionResult {
    /// Selected relay node
    pub relay_node: DeviceId,
    /// Relay type (guardian vs friend)
    pub relay_type: RelayType,
    /// Trust level of selected relay
    pub trust_level: TrustLevel,
    /// Available capability set
    pub capabilities: CapabilitySet,
}

/// Type of relay relationship
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayType {
    /// Guardian relationship (highest preference)
    Guardian,
    /// Friend relationship (lower preference)
    Friend,
}

/// Relay candidate for selection
#[derive(Debug, Clone)]
pub struct RelayCandidate {
    /// Device identifier
    pub device_id: DeviceId,
    /// Relationship type
    pub relay_type: RelayType,
    /// Trust level
    pub trust_level: TrustLevel,
    /// Available capabilities
    pub capabilities: CapabilitySet,
    /// Current load factor (0.0 to 1.0)
    pub load_factor: f32,
    /// Average latency in milliseconds
    pub avg_latency_ms: u32,
    /// Success rate (0.0 to 1.0)
    pub success_rate: f32,
}

/// Relay selection configuration
#[derive(Debug, Clone)]
pub struct RelaySelectionConfig {
    /// Prefer guardians over friends
    pub prefer_guardians: bool,
    /// Minimum trust level required
    pub min_trust_level: TrustLevel,
    /// Maximum acceptable latency
    pub max_latency_ms: u32,
    /// Minimum success rate required
    pub min_success_rate: f32,
    /// Maximum load factor to consider
    pub max_load_factor: f32,
}

impl Default for RelaySelectionConfig {
    fn default() -> Self {
        Self {
            prefer_guardians: true,
            min_trust_level: TrustLevel::Medium,
            max_latency_ms: 1000,
            min_success_rate: 0.95,
            max_load_factor: 0.8,
        }
    }
}

/// Relay selection engine
pub struct RelaySelector {
    config: RelaySelectionConfig,
}

impl RelaySelector {
    /// Create new relay selector with configuration
    pub fn new(config: RelaySelectionConfig) -> Self {
        Self { config }
    }

    /// Select best relay from candidates
    pub fn select_relay(
        &self,
        candidates: &[RelayCandidate],
        required_capability: &str,
    ) -> Result<RelaySelectionResult, AuraError> {
        // Filter candidates by basic requirements
        let qualified_candidates: Vec<&RelayCandidate> = candidates
            .iter()
            .filter(|candidate| self.meets_requirements(candidate, required_capability))
            .collect();

        if qualified_candidates.is_empty() {
            return Err(AuraError::not_found("No qualified relay candidates"));
        }

        // Group by relay type for priority selection
        let mut guardians: Vec<&RelayCandidate> = Vec::new();
        let mut friends: Vec<&RelayCandidate> = Vec::new();

        for candidate in &qualified_candidates {
            match candidate.relay_type {
                RelayType::Guardian => guardians.push(candidate),
                RelayType::Friend => friends.push(candidate),
            }
        }

        // Select from guardians first if preference is enabled
        let selected_candidate = if self.config.prefer_guardians && !guardians.is_empty() {
            self.select_best_from_group(&guardians)?
        } else if !friends.is_empty() {
            self.select_best_from_group(&friends)?
        } else if !guardians.is_empty() {
            self.select_best_from_group(&guardians)?
        } else {
            return Err(AuraError::not_found("No suitable relay candidates"));
        };

        Ok(RelaySelectionResult {
            relay_node: selected_candidate.device_id,
            relay_type: selected_candidate.relay_type.clone(),
            trust_level: selected_candidate.trust_level,
            capabilities: selected_candidate.capabilities.clone(),
        })
    }

    /// Select multiple relays for redundancy
    pub fn select_multiple_relays(
        &self,
        candidates: &[RelayCandidate],
        required_capability: &str,
        count: usize,
    ) -> Result<Vec<RelaySelectionResult>, AuraError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut selected_relays = Vec::new();
        let mut remaining_candidates = candidates.to_vec();

        for _ in 0..count {
            match self.select_relay(&remaining_candidates, required_capability) {
                Ok(selected) => {
                    // Remove selected relay from remaining candidates
                    remaining_candidates.retain(|c| c.device_id != selected.relay_node);
                    selected_relays.push(selected);
                }
                Err(_) => break, // No more qualified candidates
            }
        }

        if selected_relays.is_empty() {
            Err(AuraError::not_found("No qualified relay candidates"))
        } else {
            Ok(selected_relays)
        }
    }

    /// Check if candidate meets basic requirements
    fn meets_requirements(&self, candidate: &RelayCandidate, required_capability: &str) -> bool {
        // Check capability
        if !candidate.capabilities.permits(required_capability) {
            return false;
        }

        // Check trust level
        if candidate.trust_level < self.config.min_trust_level {
            return false;
        }

        // Check latency
        if candidate.avg_latency_ms > self.config.max_latency_ms {
            return false;
        }

        // Check success rate
        if candidate.success_rate < self.config.min_success_rate {
            return false;
        }

        // Check load factor
        if candidate.load_factor > self.config.max_load_factor {
            return false;
        }

        true
    }

    /// Select best relay from a group of same type
    fn select_best_from_group<'a>(
        &self,
        candidates: &[&'a RelayCandidate],
    ) -> Result<&'a RelayCandidate, AuraError> {
        if candidates.is_empty() {
            return Err(AuraError::not_found("Empty candidate group"));
        }

        // Score each candidate and select the best
        let mut best_candidate = candidates[0];
        let mut best_score = self.calculate_score(candidates[0]);

        for &candidate in candidates.iter().skip(1) {
            let score = self.calculate_score(candidate);
            if score > best_score {
                best_candidate = candidate;
                best_score = score;
            }
        }

        Ok(best_candidate)
    }

    /// Calculate score for relay candidate (higher is better)
    fn calculate_score(&self, candidate: &RelayCandidate) -> f32 {
        // Combine multiple factors into a score
        let trust_score = self.trust_level_score(candidate.trust_level);
        let latency_score = self.latency_score(candidate.avg_latency_ms);
        let load_score = 1.0 - candidate.load_factor; // Lower load is better
        let success_score = candidate.success_rate;

        // Weighted combination
        (trust_score * 0.3) + (latency_score * 0.25) + (load_score * 0.25) + (success_score * 0.2)
    }

    /// Convert trust level to numeric score
    fn trust_level_score(&self, trust_level: TrustLevel) -> f32 {
        match trust_level {
            TrustLevel::None => 0.0,
            TrustLevel::Low => 0.3,
            TrustLevel::Medium => 0.6,
            TrustLevel::High => 1.0,
            TrustLevel::Full => 1.2,
        }
    }

    /// Convert latency to score (lower latency = higher score)
    fn latency_score(&self, latency_ms: u32) -> f32 {
        let max_latency = self.config.max_latency_ms as f32;
        let normalized = 1.0 - (latency_ms as f32 / max_latency);
        normalized.max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relay_selection_prefers_guardians() {
        let selector = RelaySelector::new(RelaySelectionConfig::default());

        let guardian = RelayCandidate {
            device_id: DeviceId::from("guardian_1"),
            relay_type: RelayType::Guardian,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.5,
            avg_latency_ms: 100,
            success_rate: 0.99,
        };

        let friend = RelayCandidate {
            device_id: DeviceId::from("friend_1"),
            relay_type: RelayType::Friend,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.3, // Better performance than guardian
            avg_latency_ms: 50,
            success_rate: 0.995,
        };

        let candidates = vec![friend, guardian.clone()];
        let result = selector
            .select_relay(&candidates, "relay:500000:1")
            .unwrap();

        // Should select guardian despite friend having better performance
        assert_eq!(result.relay_node, guardian.device_id);
        assert_eq!(result.relay_type, RelayType::Guardian);
    }

    #[test]
    fn test_relay_selection_capability_filtering() {
        let selector = RelaySelector::new(RelaySelectionConfig::default());

        let sufficient_relay = RelayCandidate {
            device_id: DeviceId::from("sufficient"),
            relay_type: RelayType::Guardian,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.5,
            avg_latency_ms: 100,
            success_rate: 0.99,
        };

        let insufficient_relay = RelayCandidate {
            device_id: DeviceId::from("insufficient"),
            relay_type: RelayType::Guardian,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:512:3600:5"]), // Too small budget
            load_factor: 0.3,
            avg_latency_ms: 50,
            success_rate: 0.995,
        };

        let candidates = vec![insufficient_relay, sufficient_relay.clone()];
        let result = selector
            .select_relay(&candidates, "relay:1000000:1")
            .unwrap();

        // Should select relay with sufficient capability
        assert_eq!(result.relay_node, sufficient_relay.device_id);
    }

    #[test]
    fn test_relay_selection_multiple_relays() {
        let selector = RelaySelector::new(RelaySelectionConfig::default());

        let relay1 = RelayCandidate {
            device_id: DeviceId::from("relay_1"),
            relay_type: RelayType::Guardian,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.3,
            avg_latency_ms: 50,
            success_rate: 0.99,
        };

        let relay2 = RelayCandidate {
            device_id: DeviceId::from("relay_2"),
            relay_type: RelayType::Friend,
            trust_level: TrustLevel::High,
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.4,
            avg_latency_ms: 75,
            success_rate: 0.98,
        };

        let candidates = vec![relay1.clone(), relay2.clone()];
        let results = selector
            .select_multiple_relays(&candidates, "relay:500000:1", 2)
            .unwrap();

        assert_eq!(results.len(), 2);
        assert_ne!(results[0].relay_node, results[1].relay_node);
    }

    #[test]
    fn test_relay_selection_no_qualified_candidates() {
        let selector = RelaySelector::new(RelaySelectionConfig {
            min_trust_level: TrustLevel::High,
            max_latency_ms: 50,
            min_success_rate: 0.99,
            max_load_factor: 0.3,
            prefer_guardians: true,
        });

        let poor_relay = RelayCandidate {
            device_id: DeviceId::from("poor_relay"),
            relay_type: RelayType::Friend,
            trust_level: TrustLevel::Low, // Too low
            capabilities: CapabilitySet::from_permissions(&["relay:1048576:3600:10"]),
            load_factor: 0.9,    // Too high
            avg_latency_ms: 200, // Too high
            success_rate: 0.90,  // Too low
        };

        let candidates = vec![poor_relay];
        let result = selector.select_relay(&candidates, "relay:500000:1");

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No qualified"));
    }
}

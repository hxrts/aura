//! Traversal service
//!
//! Provides traversal rules and capability checks for moving through
//! the social topology.

use crate::{error::SocialError, Home, Neighborhood};
use crate::facts::{
    HomeId, TraversalAllowedFact, TraversalDepth, TraversalPosition,
};
use std::collections::HashMap;

/// Service for checking traversal permissions.
pub struct TraversalService {
    /// Traversal rules indexed by (from_home, to_home)
    rules: HashMap<(HomeId, HomeId), TraversalAllowedFact>,
}

impl TraversalService {
    /// Create a new traversal service from traversal facts.
    pub fn from_facts(facts: &[TraversalAllowedFact]) -> Self {
        let rules = facts
            .iter()
            .map(|f| ((f.from_home, f.to_home), f.clone()))
            .collect();
        Self { rules }
    }

    /// Create an empty traversal service.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }

    /// Check if traversal is allowed from one home to another.
    ///
    /// # Arguments
    /// * `from` - The current traversal position
    /// * `to_home` - The target home
    /// * `capabilities` - The capabilities the traversing authority has
    pub fn can_traverse(
        &self,
        from: &TraversalPosition,
        to_home: HomeId,
        capabilities: &[String],
    ) -> bool {
        let from_home = match from.current_home {
            Some(b) => b,
            None => return false, // Can't traverse from nowhere
        };

        // Check if there's a rule for this traversal
        if let Some(rule) = self.rules.get(&(from_home, to_home)) {
            // Check if the required capability is present
            capabilities.contains(&rule.capability_requirement)
        } else {
            // No explicit rule means traversal is not allowed
            false
        }
    }

    /// Get the required capability for traversal between two blocks.
    pub fn required_capability(&self, from: HomeId, to: HomeId) -> Option<&str> {
        self.rules
            .get(&(from, to))
            .map(|r| r.capability_requirement.as_str())
    }

    /// Add a traversal rule.
    pub fn add_rule(&mut self, fact: TraversalAllowedFact) {
        self.rules.insert((fact.from_home, fact.to_home), fact);
    }

    /// Validate a traversal request.
    pub fn validate_traversal(
        &self,
        from: &TraversalPosition,
        to_home: HomeId,
        capabilities: &[String],
    ) -> Result<(), SocialError> {
        if !self.can_traverse(from, to_home, capabilities) {
            let reason = match from.current_home {
                Some(from_home) => {
                    if let Some(required) = self.required_capability(from_home, to_home) {
                        format!("missing capability: {required}")
                    } else {
                        "no traversal path exists".to_string()
                    }
                }
                None => "no current position".to_string(),
            };
            return Err(SocialError::traversal_denied(reason));
        }
        Ok(())
    }
}

impl Default for TraversalService {
    fn default() -> Self {
        Self::new()
    }
}

/// Determines the traversal depth for an authority visiting a home.
pub fn determine_depth(
    home: &Home,
    authority_home: Option<HomeId>,
    neighborhoods: &[Neighborhood],
) -> TraversalDepth {
    // If authority is a resident, they have interior access
    if Some(home.home_id) == authority_home {
        return TraversalDepth::Interior;
    }

    // If authority.s home shares a neighborhood, they have frontage access
        if let Some(auth_block) = authority_home {
            for neighborhood in neighborhoods {
                if neighborhood.is_member(home.home_id) && neighborhood.is_member(auth_block) {
                    return TraversalDepth::Frontage;
                }
            }
        }

    // Otherwise, only street-level access
    TraversalDepth::Street
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        identifiers::ContextId,
        time::{PhysicalTime, TimeStamp},
    };

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    fn test_position(home_id: HomeId) -> TraversalPosition {
        TraversalPosition {
            neighborhood: None,
            current_home: Some(home_id),
            depth: TraversalDepth::Interior,
            context_id: ContextId::new_from_entropy([0u8; 32]),
            entered_at: test_timestamp(),
        }
    }

    #[test]
    fn test_traversal_with_capability() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let rule = TraversalAllowedFact::new(home_a, home_b, "neighbor_access");
        let service = TraversalService::from_facts(&[rule]);

        let position = test_position(home_a);
        let caps = vec!["neighbor_access".to_string()];

        assert!(service.can_traverse(&position, home_b, &caps));
        assert!(service
            .validate_traversal(&position, home_b, &caps)
            .is_ok());
    }

    #[test]
    fn test_traversal_without_capability() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let rule = TraversalAllowedFact::new(home_a, home_b, "neighbor_access");
        let service = TraversalService::from_facts(&[rule]);

        let position = test_position(home_a);
        let caps: Vec<String> = vec![];

        assert!(!service.can_traverse(&position, home_b, &caps));
        assert!(service
            .validate_traversal(&position, home_b, &caps)
            .is_err());
    }

    #[test]
    fn test_no_traversal_rule() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let service = TraversalService::new();
        let position = test_position(home_a);

        assert!(!service.can_traverse(&position, home_b, &["any_cap".to_string()]));
    }

    #[test]
    fn test_determine_depth_resident() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let home_instance = Home::new_empty(home_id);

        let depth = determine_depth(&home_instance, Some(home_id), &[]);
        assert_eq!(depth, TraversalDepth::Interior);
    }

    #[test]
    fn test_determine_depth_neighbor() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let neighborhood_id = crate::facts::NeighborhoodId::from_bytes([1u8; 32]);

        let home_instance = Home::new_empty(home_a);
        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b];

        let depth = determine_depth(&home_instance, Some(home_b), &[neighborhood]);
        assert_eq!(depth, TraversalDepth::Frontage);
    }

    #[test]
    fn test_determine_depth_stranger() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let home_instance = Home::new_empty(home_a);

        let depth = determine_depth(&home_instance, Some(home_b), &[]);
        assert_eq!(depth, TraversalDepth::Street);
    }
}

//! Traversal service
//!
//! Provides traversal rules and capability checks for moving through
//! the social topology.

use crate::facts::{
    AccessLevel, AccessLevelCapabilityConfig, AccessLevelCapabilityConfigFact, AccessOverrideFact,
    HomeId, TraversalAllowedFact, TraversalPosition,
};
use crate::{error::SocialError, Home, Neighborhood};
use aura_core::types::identifiers::AuthorityId;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

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

/// Determines the default access-level mapping for an authority visiting a home.
///
/// This computes the default level from graph topology. Higher-level policy
/// may override this mapping for specific users or contexts.
pub fn determine_default_access_level(
    home: &Home,
    authority_home: Option<HomeId>,
    neighborhoods: &[Neighborhood],
) -> AccessLevel {
    let source_home = match authority_home {
        Some(home_id) => home_id,
        None => return AccessLevel::Limited,
    };
    let hops = minimum_hop_distance(source_home, home.home_id, neighborhoods);
    default_access_level_for_hops(hops)
}

/// Determine effective access level after applying per-authority overrides.
///
/// Override precedence is deterministic:
/// 1. Compute default level from the committed-fact graph and minimum hop distance.
/// 2. Apply the newest matching override for (authority_id, target_home).
pub fn determine_access_level(
    home: &Home,
    authority_id: AuthorityId,
    authority_home: Option<HomeId>,
    neighborhoods: &[Neighborhood],
    overrides: &[AccessOverrideFact],
) -> AccessLevel {
    let default_level = determine_default_access_level(home, authority_home, neighborhoods);
    if let Some(level) =
        latest_valid_override_level(home.home_id, authority_id, default_level, overrides)
    {
        return level;
    }
    default_level
}

/// Resolve the effective capability configuration for a home.
///
/// If no config fact exists, returns the default capability mapping.
pub fn resolve_access_level_capability_config(
    home_id: HomeId,
    configs: &[AccessLevelCapabilityConfigFact],
) -> AccessLevelCapabilityConfig {
    latest_capability_config(home_id, configs)
        .map(|config| config.config.clone())
        .unwrap_or_default()
}

/// Resolve effective capabilities for an authority in the target home.
pub fn resolve_access_capabilities(
    home: &Home,
    authority_id: AuthorityId,
    authority_home: Option<HomeId>,
    neighborhoods: &[Neighborhood],
    overrides: &[AccessOverrideFact],
    configs: &[AccessLevelCapabilityConfigFact],
) -> BTreeSet<String> {
    let level =
        determine_access_level(home, authority_id, authority_home, neighborhoods, overrides);
    let config = resolve_access_level_capability_config(home.home_id, configs);
    config.capabilities_for(level).clone()
}

/// Check whether an authority has a specific configured access capability.
pub fn has_access_capability(
    home: &Home,
    authority_id: AuthorityId,
    authority_home: Option<HomeId>,
    neighborhoods: &[Neighborhood],
    overrides: &[AccessOverrideFact],
    configs: &[AccessLevelCapabilityConfigFact],
    capability: &str,
) -> bool {
    let level =
        determine_access_level(home, authority_id, authority_home, neighborhoods, overrides);
    let config = resolve_access_level_capability_config(home.home_id, configs);
    config.allows(level, capability)
}

/// Compute the minimum hop distance between two homes in the committed graph.
///
/// Returns `None` when homes are disconnected.
pub fn minimum_hop_distance(
    source_home: HomeId,
    target_home: HomeId,
    neighborhoods: &[Neighborhood],
) -> Option<u32> {
    if source_home == target_home {
        return Some(0);
    }

    let graph = build_home_graph(neighborhoods);
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back((source_home, 0u32));
    visited.insert(source_home);

    while let Some((current, distance)) = queue.pop_front() {
        let neighbors = match graph.get(&current) {
            Some(neighbors) => neighbors,
            None => continue,
        };

        for neighbor in neighbors {
            if *neighbor == target_home {
                return Some(distance + 1);
            }
            if visited.insert(*neighbor) {
                queue.push_back((*neighbor, distance + 1));
            }
        }
    }

    None
}

fn build_home_graph(neighborhoods: &[Neighborhood]) -> HashMap<HomeId, HashSet<HomeId>> {
    let mut graph: HashMap<HomeId, HashSet<HomeId>> = HashMap::new();

    for neighborhood in neighborhoods {
        for (index, home_a) in neighborhood.member_homes.iter().enumerate() {
            graph.entry(*home_a).or_default();
            for home_b in neighborhood.member_homes.iter().skip(index + 1) {
                graph.entry(*home_a).or_default().insert(*home_b);
                graph.entry(*home_b).or_default().insert(*home_a);
            }
        }

        for (home_a, home_b) in &neighborhood.adjacencies {
            graph.entry(*home_a).or_default().insert(*home_b);
            graph.entry(*home_b).or_default().insert(*home_a);
        }
    }

    graph
}

fn latest_valid_override_level(
    target_home: HomeId,
    authority_id: AuthorityId,
    default_level: AccessLevel,
    overrides: &[AccessOverrideFact],
) -> Option<AccessLevel> {
    overrides
        .iter()
        .filter(|override_fact| {
            override_fact.home_id == target_home && override_fact.authority_id == authority_id
        })
        .filter(|override_fact| override_fact.is_valid_for_default(default_level))
        .max_by_key(|override_fact| override_fact.set_at.to_index_ms().value())
        .map(|override_fact| override_fact.access_level)
}

fn latest_capability_config(
    home_id: HomeId,
    configs: &[AccessLevelCapabilityConfigFact],
) -> Option<&AccessLevelCapabilityConfigFact> {
    configs
        .iter()
        .filter(|config| config.home_id == home_id)
        .max_by_key(|config| config.configured_at.to_index_ms().value())
}

fn default_access_level_for_hops(hops: Option<u32>) -> AccessLevel {
    match hops {
        Some(0) => AccessLevel::Full,
        Some(1) => AccessLevel::Partial,
        Some(_) | None => AccessLevel::Limited,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        time::{PhysicalTime, TimeStamp},
        types::identifiers::{AuthorityId, ContextId},
    };
    use std::collections::BTreeSet;

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
            depth: AccessLevel::Full,
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
        assert!(service.validate_traversal(&position, home_b, &caps).is_ok());
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
    fn test_determine_default_access_level_member() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let home_instance = Home::new_empty(home_id);

        let depth = determine_default_access_level(&home_instance, Some(home_id), &[]);
        assert_eq!(depth, AccessLevel::Full);
    }

    #[test]
    fn test_determine_default_access_level_neighbor() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let neighborhood_id = crate::facts::NeighborhoodId::from_bytes([1u8; 32]);

        let home_instance = Home::new_empty(home_a);
        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b];

        let depth = determine_default_access_level(&home_instance, Some(home_b), &[neighborhood]);
        assert_eq!(depth, AccessLevel::Partial);
    }

    #[test]
    fn test_determine_default_access_level_stranger() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let home_instance = Home::new_empty(home_a);

        let depth = determine_default_access_level(&home_instance, Some(home_b), &[]);
        assert_eq!(depth, AccessLevel::Limited);
    }

    #[test]
    fn test_minimum_hop_distance_multi_hop() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let home_c = HomeId::from_bytes([3u8; 32]);

        let mut n1 = Neighborhood::new_empty(crate::facts::NeighborhoodId::from_bytes([1u8; 32]));
        n1.member_homes = vec![home_a, home_b];

        let mut n2 = Neighborhood::new_empty(crate::facts::NeighborhoodId::from_bytes([2u8; 32]));
        n2.member_homes = vec![home_b, home_c];

        let hops = minimum_hop_distance(home_a, home_c, &[n1, n2]);
        assert_eq!(hops, Some(2));
    }

    #[test]
    fn test_determine_access_level_with_override() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let authority = AuthorityId::new_from_entropy([9u8; 32]);

        let home_instance = Home::new_empty(home_a);
        let override_fact = AccessOverrideFact {
            authority_id: authority,
            home_id: home_a,
            access_level: AccessLevel::Limited,
            set_at: test_timestamp(),
        };

        let level = determine_access_level(
            &home_instance,
            authority,
            Some(home_b),
            &[],
            &[override_fact],
        );
        assert_eq!(level, AccessLevel::Limited);
    }

    #[test]
    fn test_determine_access_level_applies_valid_upgrade_override() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let authority = AuthorityId::new_from_entropy([8u8; 32]);
        let home_instance = Home::new_empty(home_a);

        // Default is Limited for disconnected/2+-hop.
        let override_fact = AccessOverrideFact {
            authority_id: authority,
            home_id: home_a,
            access_level: AccessLevel::Partial,
            set_at: test_timestamp(),
        };

        let level = determine_access_level(
            &home_instance,
            authority,
            Some(home_b),
            &[],
            &[override_fact],
        );
        assert_eq!(level, AccessLevel::Partial);
    }

    #[test]
    fn test_determine_access_level_ignores_invalid_override_transition() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let authority = AuthorityId::new_from_entropy([7u8; 32]);
        let home_instance = Home::new_empty(home_a);

        // Default is Full (same home). Full -> Limited override is invalid.
        let invalid_override = AccessOverrideFact {
            authority_id: authority,
            home_id: home_a,
            access_level: AccessLevel::Limited,
            set_at: test_timestamp(),
        };

        let level = determine_access_level(
            &home_instance,
            authority,
            Some(home_a),
            &[],
            &[invalid_override],
        );
        assert_eq!(level, AccessLevel::Full);
    }

    #[test]
    fn test_determine_access_level_applies_valid_downgrade_override() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let neighborhood_id = crate::facts::NeighborhoodId::from_bytes([3u8; 32]);
        let authority = AuthorityId::new_from_entropy([6u8; 32]);
        let home_instance = Home::new_empty(home_a);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b];

        // Default is Partial (1-hop). Partial -> Limited override is valid.
        let override_fact = AccessOverrideFact {
            authority_id: authority,
            home_id: home_a,
            access_level: AccessLevel::Limited,
            set_at: test_timestamp(),
        };

        let level = determine_access_level(
            &home_instance,
            authority,
            Some(home_b),
            &[neighborhood],
            &[override_fact],
        );
        assert_eq!(level, AccessLevel::Limited);
    }

    #[test]
    fn test_resolve_access_level_capability_config_defaults_when_missing() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let config = resolve_access_level_capability_config(home_a, &[]);
        assert!(config.allows(AccessLevel::Full, "send_message"));
        assert!(config.allows(AccessLevel::Partial, "send_message"));
        assert!(!config.allows(AccessLevel::Limited, "send_message"));
    }

    #[test]
    fn test_resolve_access_level_capability_config_uses_latest_fact() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let old_at = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 100,
            uncertainty: None,
        });
        let new_at = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 200,
            uncertainty: None,
        });

        let mut old_partial = BTreeSet::new();
        old_partial.insert("view_members".to_string());
        let mut new_partial = BTreeSet::new();
        new_partial.insert("send_dm".to_string());

        let old = AccessLevelCapabilityConfigFact {
            home_id: home_a,
            config: AccessLevelCapabilityConfig {
                full: BTreeSet::new(),
                partial: old_partial,
                limited: BTreeSet::new(),
            },
            configured_at: old_at,
        };
        let new = AccessLevelCapabilityConfigFact {
            home_id: home_a,
            config: AccessLevelCapabilityConfig {
                full: BTreeSet::new(),
                partial: new_partial,
                limited: BTreeSet::new(),
            },
            configured_at: new_at,
        };

        let resolved = resolve_access_level_capability_config(home_a, &[old, new]);
        assert!(resolved.allows(AccessLevel::Partial, "send_dm"));
        assert!(!resolved.allows(AccessLevel::Partial, "view_members"));
    }

    #[test]
    fn test_has_access_capability_uses_effective_level_and_config() {
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let authority = AuthorityId::new_from_entropy([5u8; 32]);
        let home_instance = Home::new_empty(home_a);

        let override_fact = AccessOverrideFact {
            authority_id: authority,
            home_id: home_a,
            access_level: AccessLevel::Partial,
            set_at: test_timestamp(),
        };

        let config = AccessLevelCapabilityConfigFact {
            home_id: home_a,
            config: AccessLevelCapabilityConfig {
                full: BTreeSet::new(),
                partial: ["can_partial".to_string()].into_iter().collect(),
                limited: BTreeSet::new(),
            },
            configured_at: test_timestamp(),
        };

        assert!(has_access_capability(
            &home_instance,
            authority,
            Some(home_b),
            &[],
            &[override_fact],
            &[config],
            "can_partial"
        ));
    }
}

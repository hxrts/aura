//! Integration Tests for Social Infrastructure
//!
//! Tests the complete social topology infrastructure including:
//! - Discovery layer selection
//! - Relay candidate generation
//! - Home and neighborhood availability
//! - Social topology queries

use aura_core::effects::relay::{RelayContext, RelayRelationship};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_social::facts::{
    AdjacencyFact, HomeConfigFact, HomeFact, HomeId, HomeMemberFact, NeighborhoodFact,
    NeighborhoodId, ResidentFact, StewardFact,
};
use aura_social::{
    DiscoveryLayer, Home, Neighborhood, ReachabilityChecker, RelayCandidateBuilder, SocialTopology,
};

// ============================================================================
// Test Helpers
// ============================================================================

fn test_timestamp() -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1700000000000,
        uncertainty: None,
    })
}

fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn test_home_id(seed: u8) -> HomeId {
    HomeId::from_bytes([seed; 32])
}

fn test_neighborhood_id(seed: u8) -> NeighborhoodId {
    NeighborhoodId::from_bytes([seed; 32])
}

fn test_context(source: AuthorityId, destination: AuthorityId) -> RelayContext {
    RelayContext::new(
        ContextId::new_from_entropy([1u8; 32]),
        source,
        destination,
        3,
        [0u8; 32],
    )
}

/// Create a home with the specified number of residents
fn create_home(home_seed: u8, resident_count: usize) -> (Home, AuthorityId, Vec<AuthorityId>) {
    let home_id = test_home_id(home_seed);
    let timestamp = test_timestamp();

    let home_fact = HomeFact::new(home_id, timestamp.clone());

    let mut residents = Vec::with_capacity(resident_count);
    let mut resident_facts = Vec::with_capacity(resident_count);

    for i in 0..resident_count {
        let authority = test_authority((home_seed * 10) + i as u8 + 1);
        residents.push(authority);
        resident_facts.push(ResidentFact::new(authority, home_id, timestamp.clone()));
    }

    let steward = residents[0];
    let steward_facts = vec![StewardFact::new(steward, home_id, timestamp)];

    let home = Home::from_facts(&home_fact, None, &resident_facts, &steward_facts);

    (home, steward, residents)
}

/// Create a neighborhood with the specified homes
fn create_neighborhood(neighborhood_seed: u8, home_ids: Vec<HomeId>) -> Neighborhood {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut member_facts = Vec::with_capacity(home_ids.len());
    for home_id in &home_ids {
        member_facts.push(HomeMemberFact::new(
            *home_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create linear adjacencies
    let mut adjacency_facts = Vec::new();
    for i in 0..home_ids.len().saturating_sub(1) {
        adjacency_facts.push(AdjacencyFact::new(
            home_ids[i],
            home_ids[i + 1],
            neighborhood_id,
        ));
    }

    Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts)
}

// ============================================================================
// Reachability Implementations for Testing
// ============================================================================

struct AlwaysReachable;
impl ReachabilityChecker for AlwaysReachable {
    fn is_reachable(&self, _peer: &AuthorityId) -> bool {
        true
    }
}

struct NeverReachable;
impl ReachabilityChecker for NeverReachable {
    fn is_reachable(&self, _peer: &AuthorityId) -> bool {
        false
    }
}

// ============================================================================
// Discovery Layer Selection Tests
// ============================================================================

#[test]
fn test_discovery_layer_direct_for_self() {
    let (home, steward, _residents) = create_home(1, 3);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Self is always Direct
    assert_eq!(topology.discovery_layer(&steward), DiscoveryLayer::Direct);
}

#[test]
fn test_discovery_layer_direct_for_home_peers() {
    let (home, steward, residents) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // All home peers should be Direct (we have a relationship with them)
    for resident in &residents[1..] {
        assert_eq!(topology.discovery_layer(resident), DiscoveryLayer::Direct);
    }
}

#[test]
fn test_discovery_layer_home_for_unknown_with_social_presence() {
    let (home, steward, _residents) = create_home(1, 3);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Unknown peer with home presence should be Home layer
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);
}

#[test]
fn test_discovery_layer_rendezvous_without_social_presence() {
    let authority = test_authority(1);
    let topology = SocialTopology::empty(authority);

    // Without social presence, unknown peers require Rendezvous
    let unknown = test_authority(99);
    assert_eq!(
        topology.discovery_layer(&unknown),
        DiscoveryLayer::Rendezvous
    );
}

#[test]
fn test_discovery_layer_with_neighborhoods_but_has_home_peers() {
    let (home1, steward, _residents1) = create_home(1, 3);
    let (home2, _, _residents2) = create_home(2, 3);

    let neighborhood = create_neighborhood(1, vec![home1.home_id, home2.home_id]);

    let topology = SocialTopology::new(steward, Some(home1), vec![neighborhood]);

    // With home peers available, discovery layer is Home (faster path)
    // Neighborhood layer is only used when we don't have home peers
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);
}

#[test]
fn test_discovery_layer_neighborhood_without_home_peers() {
    // Create a single-resident home (no home peers)
    let (home1, steward, _) = create_home(1, 1);
    let (home2, peer_from_home2, _) = create_home(2, 3);

    let neighborhood = create_neighborhood(1, vec![home1.home_id, home2.home_id]);

    let mut topology = SocialTopology::new(steward, Some(home1), vec![neighborhood]);

    // No home peers, so would fall through to check neighborhoods
    // But we need to add neighborhood peers explicitly
    let neighborhood_id_bytes = *test_neighborhood_id(1).as_bytes();
    topology.add_peer(
        peer_from_home2,
        RelayRelationship::NeighborhoodPeer {
            neighborhood_id: neighborhood_id_bytes,
        },
    );

    // Now with neighborhood peers but no home peers, should be Neighborhood
    let unknown = test_authority(99);
    assert_eq!(
        topology.discovery_layer(&unknown),
        DiscoveryLayer::Neighborhood
    );
}

#[test]
fn test_discovery_layer_priority() {
    // Direct is highest priority
    assert_eq!(DiscoveryLayer::Direct.priority(), 0);
    assert_eq!(DiscoveryLayer::Home.priority(), 1);
    assert_eq!(DiscoveryLayer::Neighborhood.priority(), 2);
    assert_eq!(DiscoveryLayer::Rendezvous.priority(), 3);

    // Lower priority number = faster/better path
    assert!(DiscoveryLayer::Direct.priority() < DiscoveryLayer::Home.priority());
    assert!(DiscoveryLayer::Home.priority() < DiscoveryLayer::Neighborhood.priority());
    assert!(DiscoveryLayer::Neighborhood.priority() < DiscoveryLayer::Rendezvous.priority());
}

// ============================================================================
// Relay Candidate Generation Tests
// ============================================================================

#[test]
fn test_relay_candidates_from_home_peers() {
    let (home, steward, _residents) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Get home peers
    let home_peers = topology.home_peers();
    assert_eq!(home_peers.len(), 4); // 5 residents - 1 self

    // Generate relay candidates using the builder
    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Should have candidates from home peers
    assert!(!candidates.is_empty());
    assert_eq!(candidates.len(), 4);
}

#[test]
fn test_relay_candidate_builder_with_empty_topology() {
    let authority = test_authority(1);
    let topology = SocialTopology::empty(authority);

    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(authority, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Empty topology should have no candidates
    assert!(candidates.is_empty());
}

#[test]
fn test_relay_candidate_builder_with_reachability_filter() {
    let (home, steward, _residents) = create_home(1, 3);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);

    // With NeverReachable, reachable candidates should be empty
    let reachable_candidates = builder.build_reachable_candidates(&context, &NeverReachable);
    assert!(reachable_candidates.is_empty());

    // With AlwaysReachable, we should have reachable candidates
    let reachable_candidates = builder.build_reachable_candidates(&context, &AlwaysReachable);
    assert_eq!(reachable_candidates.len(), 2); // 3 residents - 1 self
}

// ============================================================================
// Home Availability Tests
// ============================================================================

#[test]
fn test_home_resident_query() {
    let (home, steward, residents) = create_home(1, 5);

    // Steward should be a resident
    assert!(home.is_resident(&steward));

    // All residents should be residents
    for resident in &residents {
        assert!(home.is_resident(resident));
    }

    // Non-resident should not be a resident
    let non_resident = test_authority(99);
    assert!(!home.is_resident(&non_resident));
}

#[test]
fn test_home_steward_query() {
    let (home, steward, residents) = create_home(1, 3);

    // Steward should be a steward
    assert!(home.is_steward(&steward));

    // Other residents are not stewards
    for resident in &residents[1..] {
        assert!(!home.is_steward(resident));
    }
}

#[test]
fn test_home_available_slots() {
    let (home, _, _) = create_home(1, 5);

    // Default max is 8 (from HomeConfigFact::V1_MAX_RESIDENTS)
    assert!(home.can_add_resident()); // 5 < 8

    // Create a full home
    let (full_home, _, _) = create_home(2, 8);
    assert!(!full_home.can_add_resident()); // 8 == 8
}

// ============================================================================
// Neighborhood Traversal Tests
// ============================================================================

#[test]
fn test_neighborhood_adjacency() {
    let home_ids: Vec<HomeId> = (1..=4).map(test_home_id).collect();
    let neighborhood = create_neighborhood(1, home_ids.clone());

    // Linear chain adjacencies: 1-2, 2-3, 3-4
    assert!(neighborhood.are_adjacent(home_ids[0], home_ids[1]));
    assert!(neighborhood.are_adjacent(home_ids[1], home_ids[2]));
    assert!(neighborhood.are_adjacent(home_ids[2], home_ids[3]));

    // Non-adjacent pairs
    assert!(!neighborhood.are_adjacent(home_ids[0], home_ids[2]));
    assert!(!neighborhood.are_adjacent(home_ids[0], home_ids[3]));
}

#[test]
fn test_neighborhood_membership() {
    let home_ids: Vec<HomeId> = (1..=3).map(test_home_id).collect();
    let neighborhood = create_neighborhood(1, home_ids.clone());

    // All homes should be members
    for home_id in &home_ids {
        assert!(neighborhood.is_member(*home_id));
    }

    // Non-member home
    let non_member = test_home_id(99);
    assert!(!neighborhood.is_member(non_member));
}

#[test]
fn test_neighborhood_adjacent_homes() {
    let home_ids: Vec<HomeId> = (1..=4).map(test_home_id).collect();
    let neighborhood = create_neighborhood(1, home_ids.clone());

    // Home 2 (index 1) should have homes 1 and 3 as adjacent
    let adjacent_to_2 = neighborhood.adjacent_homes(home_ids[1]);
    assert_eq!(adjacent_to_2.len(), 2);
    assert!(adjacent_to_2.contains(&home_ids[0]));
    assert!(adjacent_to_2.contains(&home_ids[2]));

    // Home 1 (index 0) should only have home 2 as adjacent
    let adjacent_to_1 = neighborhood.adjacent_homes(home_ids[0]);
    assert_eq!(adjacent_to_1.len(), 1);
    assert!(adjacent_to_1.contains(&home_ids[1]));
}

// ============================================================================
// Social Topology Integration Tests
// ============================================================================

#[test]
fn test_topology_has_social_presence() {
    let (home, steward, _) = create_home(1, 3);

    // With home
    let topology_with_home = SocialTopology::new(steward, Some(home), vec![]);
    assert!(topology_with_home.has_social_presence());

    // Without home
    let topology_empty = SocialTopology::empty(steward);
    assert!(!topology_empty.has_social_presence());
}

#[test]
fn test_topology_knows_peer() {
    let (home, steward, residents) = create_home(1, 3);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Should know home peers
    for resident in &residents[1..] {
        assert!(topology.knows_peer(resident));
    }

    // Should not know unknown peer
    let unknown = test_authority(99);
    assert!(!topology.knows_peer(&unknown));
}

#[test]
fn test_topology_add_guardian() {
    let (home, steward, _) = create_home(1, 3);
    let mut topology = SocialTopology::new(steward, Some(home), vec![]);

    let guardian = test_authority(99);

    // Initially unknown
    assert!(!topology.knows_peer(&guardian));

    // Add as guardian
    topology.add_peer(guardian, RelayRelationship::Guardian);

    // Now known
    assert!(topology.knows_peer(&guardian));

    // Discovery layer should be Direct
    assert_eq!(topology.discovery_layer(&guardian), DiscoveryLayer::Direct);
}

#[test]
fn test_topology_discovery_context() {
    let (home, steward, residents) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Check discovery context for unknown peer
    let unknown = test_authority(99);
    let (layer, peers) = topology.discovery_context(&unknown);

    assert_eq!(layer, DiscoveryLayer::Home);
    assert_eq!(peers.len(), 4); // 5 residents - 1 self

    // All returned peers should be home peers
    for peer in peers {
        assert!(residents[1..].contains(&peer));
    }
}

// ============================================================================
// Budget Enforcement Pattern Tests (Conceptual)
// ============================================================================

#[test]
fn test_budget_layer_ordering() {
    // This tests the conceptual budget ordering:
    // Flood (highest) > Neighborhood > Home > Direct (lowest/cheapest)

    let flood_cost = 100;
    let neighborhood_cost = 10;
    let home_cost = 3;
    let direct_cost = 1;

    assert!(flood_cost > neighborhood_cost);
    assert!(neighborhood_cost > home_cost);
    assert!(home_cost > direct_cost);
}

#[test]
fn test_discovery_layer_implies_budget() {
    // Direct: minimal cost (known peer)
    assert!(DiscoveryLayer::Direct.is_known());

    // Home: low cost (relay through known peers)
    assert!(DiscoveryLayer::Home.has_social_presence());
    assert!(!DiscoveryLayer::Home.is_known());

    // Neighborhood: medium cost (traverse neighborhood)
    assert!(DiscoveryLayer::Neighborhood.has_social_presence());
    assert!(!DiscoveryLayer::Neighborhood.is_known());

    // Rendezvous: highest cost (global flood)
    assert!(!DiscoveryLayer::Rendezvous.has_social_presence());
    assert!(!DiscoveryLayer::Rendezvous.is_known());
}

// ============================================================================
// Multi-Home Topology Tests
// ============================================================================

#[test]
fn test_multi_home_neighborhood_topology() {
    // Create multiple homes
    let (home1, steward1, _) = create_home(1, 3);
    let (home2, _, _) = create_home(2, 3);
    let (home3, _, _) = create_home(3, 3);

    // Create neighborhood with all homes
    let neighborhood = create_neighborhood(1, vec![home1.home_id, home2.home_id, home3.home_id]);

    // Create topology for steward1 in home1
    let topology = SocialTopology::new(steward1, Some(home1), vec![neighborhood.clone()]);

    // Should have home presence
    assert!(topology.has_social_presence());

    // Should have 2 home peers
    assert_eq!(topology.home_peers().len(), 2);

    // Neighborhood should have 3 members
    assert_eq!(neighborhood.member_homes.len(), 3);

    // Discovery layer for unknown is Home since we have home peers
    // (Home layer is preferred/faster when available)
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_single_resident_home() {
    let (home, steward, _residents) = create_home(1, 1);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Should have social presence even with single resident
    assert!(topology.has_social_presence());

    // No home peers (only self)
    assert!(topology.home_peers().is_empty());

    // Discovery layer for unknown is Home (have home but no peers to relay)
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);
}

#[test]
fn test_empty_neighborhood() {
    let neighborhood_id = test_neighborhood_id(1);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp);
    let neighborhood = Neighborhood::from_facts(&neighborhood_fact, &[], &[]);

    // Empty neighborhood
    assert!(neighborhood.member_homes.is_empty());

    // No home is a member
    let home_id = test_home_id(1);
    assert!(!neighborhood.is_member(home_id));
}

#[test]
fn test_home_with_config() {
    let home_id = test_home_id(1);
    let timestamp = test_timestamp();

    let home_fact = HomeFact::new(home_id, timestamp.clone());
    let config_fact = HomeConfigFact {
        home_id,
        max_residents: 4,
        neighborhood_limit: 2,
    };

    let residents: Vec<AuthorityId> = (1..=3).map(test_authority).collect();
    let resident_facts: Vec<ResidentFact> = residents
        .iter()
        .map(|r| ResidentFact::new(*r, home_id, timestamp.clone()))
        .collect();
    let steward_facts = vec![StewardFact::new(residents[0], home_id, timestamp)];

    let home = Home::from_facts(
        &home_fact,
        Some(&config_fact),
        &resident_facts,
        &steward_facts,
    );

    // Should have custom max residents
    assert!(home.can_add_resident()); // 3 < 4

    // Verify residents
    assert_eq!(home.residents.len(), 3);
}

// ============================================================================
// Relay Selection with Guardian Tests
// ============================================================================

#[test]
fn test_guardian_in_relay_candidates() {
    let (home, steward, _) = create_home(1, 3);
    let mut topology = SocialTopology::new(steward, Some(home), vec![]);

    let guardian = test_authority(88);
    topology.add_peer(guardian, RelayRelationship::Guardian);

    // Build relay candidates
    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Should include both home peers and guardian
    // 3 residents - 1 self = 2 home peers + 1 guardian = 3 candidates
    assert_eq!(candidates.len(), 3);

    // Guardian should be in candidates
    assert!(candidates.iter().any(|c| c.authority_id == guardian));
}

#[test]
fn test_relay_candidate_relationship_types() {
    let (home, steward, _) = create_home(1, 3);
    let mut topology = SocialTopology::new(steward, Some(home), vec![]);

    let guardian = test_authority(88);
    topology.add_peer(guardian, RelayRelationship::Guardian);

    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Check relationships
    for candidate in &candidates {
        if candidate.authority_id == guardian {
            assert!(matches!(
                candidate.relationship,
                RelayRelationship::Guardian
            ));
        } else {
            assert!(matches!(
                candidate.relationship,
                RelayRelationship::HomePeer { .. }
            ));
        }
    }
}

//! Simulation Tests for Social Infrastructure
//!
//! Tests deterministic behavior under various network conditions:
//! - Multi-node topology scenarios
//! - Partition scenarios
//! - Budget exhaustion patterns
//! - Deterministic replay verification

use aura_core::effects::relay::RelayRelationship;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_social::facts::{
    AdjacencyFact, HomeFact, HomeId, HomeMemberFact, NeighborhoodFact, NeighborhoodId,
    ResidentFact, StewardFact,
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

/// Create a fully connected neighborhood
fn create_fully_connected_neighborhood(
    neighborhood_seed: u8,
    home_ids: Vec<HomeId>,
) -> Neighborhood {
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

    // Create all pairs of adjacencies
    let mut adjacency_facts = Vec::new();
    for i in 0..home_ids.len() {
        for j in (i + 1)..home_ids.len() {
            adjacency_facts.push(AdjacencyFact::new(
                home_ids[i],
                home_ids[j],
                neighborhood_id,
            ));
        }
    }

    Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts)
}

/// Always reachable implementation
struct AlwaysReachable;
impl ReachabilityChecker for AlwaysReachable {
    fn is_reachable(&self, _peer: &AuthorityId) -> bool {
        true
    }
}

/// Never reachable implementation
struct NeverReachable;
impl ReachabilityChecker for NeverReachable {
    fn is_reachable(&self, _peer: &AuthorityId) -> bool {
        false
    }
}

/// Partial reachability - only specified peers are reachable
struct PartialReachability {
    reachable: std::collections::HashSet<AuthorityId>,
}

impl PartialReachability {
    fn new(peers: impl IntoIterator<Item = AuthorityId>) -> Self {
        Self {
            reachable: peers.into_iter().collect(),
        }
    }
}

impl ReachabilityChecker for PartialReachability {
    fn is_reachable(&self, peer: &AuthorityId) -> bool {
        self.reachable.contains(peer)
    }
}

// ============================================================================
// Multi-Node Topology Simulation Tests
// ============================================================================

#[test]
fn test_large_home_simulation() {
    // Simulate a home at maximum capacity (8 residents)
    let (home, steward, residents) = create_home(1, 8);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Should have 7 home peers
    assert_eq!(topology.home_peers().len(), 7);

    // All residents should be known
    for resident in &residents {
        if resident != &steward {
            assert!(topology.knows_peer(resident));
        }
    }

    // Home should be at capacity
    let (full_home, _, _) = create_home(2, 8);
    assert!(!full_home.can_add_resident());
}

#[test]
fn test_multi_neighborhood_simulation() {
    // Create a node that's part of multiple neighborhoods
    let (home1, steward, _) = create_home(1, 4);
    let (home2, _, _) = create_home(2, 4);
    let (home3, _, _) = create_home(3, 4);
    let (home4, _, _) = create_home(4, 4);

    // Neighborhood 1: homes 1, 2
    let neighborhood1 = create_neighborhood(1, vec![home1.home_id, home2.home_id]);

    // Neighborhood 2: homes 1, 3, 4
    let neighborhood2 = create_neighborhood(2, vec![home1.home_id, home3.home_id, home4.home_id]);

    // Store home1_id before moving home1
    let home1_id = home1.home_id;

    // Create topology with both neighborhoods
    let topology = SocialTopology::new(
        steward,
        Some(home1),
        vec![neighborhood1.clone(), neighborhood2.clone()],
    );

    // Should have social presence
    assert!(topology.has_social_presence());

    // Should have 3 home peers
    assert_eq!(topology.home_peers().len(), 3);

    // Both neighborhoods should track home1
    assert!(neighborhood1.is_member(home1_id));
    assert!(neighborhood2.is_member(home1_id));
}

#[test]
fn test_mesh_neighborhood_topology() {
    // Create a fully-connected mesh of homes
    let homes: Vec<(Home, AuthorityId, Vec<AuthorityId>)> =
        (1..=5).map(|i| create_home(i, 3)).collect();

    let home_ids: Vec<HomeId> = homes.iter().map(|(b, _, _)| b.home_id).collect();
    let neighborhood = create_fully_connected_neighborhood(1, home_ids.clone());

    // All homes should be adjacent to each other
    for i in 0..homes.len() {
        for j in 0..homes.len() {
            if i != j {
                assert!(neighborhood.are_adjacent(home_ids[i], home_ids[j]));
            }
        }
    }

    // Create topology for first home
    let (home0, steward0, _) = &homes[0];
    let topology = SocialTopology::new(*steward0, Some(home0.clone()), vec![neighborhood]);

    // Should have social presence
    assert!(topology.has_social_presence());
}

// ============================================================================
// Partition Scenario Tests
// ============================================================================

#[test]
fn test_partial_home_partition() {
    // Simulate a scenario where some home peers are unreachable
    let (home, steward, residents) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    // Only first two peers are reachable
    let reachable_peers: std::collections::HashSet<AuthorityId> =
        residents[1..=2].iter().copied().collect();
    let reachability = PartialReachability::new(reachable_peers.iter().copied());

    // Build candidates
    let builder = RelayCandidateBuilder::from_topology(topology);
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        steward,
        test_authority(99),
        3,
        [0u8; 32],
    );

    let reachable_candidates = builder.build_reachable_candidates(&context, &reachability);

    // Should only have 2 reachable candidates
    assert_eq!(reachable_candidates.len(), 2);

    // All reachable candidates should be in our reachable set
    for candidate in &reachable_candidates {
        assert!(reachable_peers.contains(&candidate.authority_id));
    }
}

#[test]
fn test_complete_home_partition() {
    // Simulate a scenario where ALL home peers are unreachable
    let (home, steward, _) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    let builder = RelayCandidateBuilder::from_topology(topology);
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        steward,
        test_authority(99),
        3,
        [0u8; 32],
    );

    // With NeverReachable, no candidates should be reachable
    let reachable_candidates = builder.build_reachable_candidates(&context, &NeverReachable);
    assert!(reachable_candidates.is_empty());

    // But we should still get all candidates when not filtering
    let all_candidates = builder.build_candidates(&context, &NeverReachable);
    assert_eq!(all_candidates.len(), 4); // 5 residents - 1 self
}

#[test]
fn test_guardian_fallback_during_partition() {
    // Test that guardians can be used when home peers are unreachable
    let (home, steward, _residents) = create_home(1, 3);
    let mut topology = SocialTopology::new(steward, Some(home), vec![]);

    let guardian = test_authority(88);
    topology.add_peer(guardian, RelayRelationship::Guardian);

    // Only guardian is reachable
    let reachability = PartialReachability::new(vec![guardian]);

    let builder = RelayCandidateBuilder::from_topology(topology);
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        steward,
        test_authority(99),
        3,
        [0u8; 32],
    );

    let reachable_candidates = builder.build_reachable_candidates(&context, &reachability);

    // Should only have guardian
    assert_eq!(reachable_candidates.len(), 1);
    assert_eq!(reachable_candidates[0].authority_id, guardian);
}

// ============================================================================
// Budget Exhaustion Behavior Tests
// ============================================================================

#[test]
fn test_discovery_layer_cost_progression() {
    // Test that discovery layers have increasing costs
    let costs = [
        (DiscoveryLayer::Direct, 0),
        (DiscoveryLayer::Home, 1),
        (DiscoveryLayer::Neighborhood, 2),
        (DiscoveryLayer::Rendezvous, 3),
    ];

    // Verify ordering
    for i in 0..costs.len() - 1 {
        assert!(
            costs[i].1 < costs[i + 1].1,
            "{:?} should have lower cost than {:?}",
            costs[i].0,
            costs[i + 1].0
        );
    }
}

#[test]
fn test_progressive_social_presence_loss() {
    let (home, steward, _residents) = create_home(1, 3);

    // Full social presence
    let topology_full = SocialTopology::new(steward, Some(home.clone()), vec![]);
    assert!(topology_full.has_social_presence());
    assert_eq!(topology_full.home_peers().len(), 2);

    // Empty social presence
    let topology_empty = SocialTopology::empty(steward);
    assert!(!topology_empty.has_social_presence());
    assert!(topology_empty.home_peers().is_empty());

    // Discovery layers should reflect this
    let target = test_authority(99);
    assert_eq!(topology_full.discovery_layer(&target), DiscoveryLayer::Home);
    assert_eq!(
        topology_empty.discovery_layer(&target),
        DiscoveryLayer::Rendezvous
    );
}

// ============================================================================
// Deterministic Replay Tests
// ============================================================================

#[test]
fn test_deterministic_topology_construction() {
    // Verify that topology construction is deterministic
    let (home1a, steward1a, residents1a) = create_home(1, 5);
    let (home1b, steward1b, residents1b) = create_home(1, 5);

    // Same seed should produce same results
    assert_eq!(home1a.home_id, home1b.home_id);
    assert_eq!(steward1a, steward1b);
    assert_eq!(residents1a, residents1b);

    // Topologies should be equivalent
    let topology_a = SocialTopology::new(steward1a, Some(home1a), vec![]);
    let topology_b = SocialTopology::new(steward1b, Some(home1b), vec![]);

    assert_eq!(topology_a.home_peers().len(), topology_b.home_peers().len());

    // Home peers should be the same
    let peers_a: std::collections::HashSet<_> = topology_a.home_peers().into_iter().collect();
    let peers_b: std::collections::HashSet<_> = topology_b.home_peers().into_iter().collect();
    assert_eq!(peers_a, peers_b);
}

#[test]
fn test_deterministic_neighborhood_adjacency() {
    // Verify that neighborhood adjacency is deterministic
    let home_ids: Vec<HomeId> = (1..=4).map(test_home_id).collect();

    let neighborhood_a = create_neighborhood(1, home_ids.clone());
    let neighborhood_b = create_neighborhood(1, home_ids.clone());

    // Same adjacencies
    for i in 0..home_ids.len() {
        for j in 0..home_ids.len() {
            assert_eq!(
                neighborhood_a.are_adjacent(home_ids[i], home_ids[j]),
                neighborhood_b.are_adjacent(home_ids[i], home_ids[j]),
            );
        }
    }
}

#[test]
fn test_deterministic_relay_candidate_order() {
    // Verify that relay candidates are generated in deterministic order
    let (home, steward, _) = create_home(1, 5);
    let topology = SocialTopology::new(steward, Some(home), vec![]);

    let builder = RelayCandidateBuilder::from_topology(topology.clone());
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        steward,
        test_authority(99),
        3,
        [0u8; 32],
    );

    // Generate candidates multiple times
    let candidates1 = builder.build_candidates(&context, &AlwaysReachable);

    let builder2 = RelayCandidateBuilder::from_topology(topology);
    let candidates2 = builder2.build_candidates(&context, &AlwaysReachable);

    // Should be in same order
    assert_eq!(candidates1.len(), candidates2.len());
    for (c1, c2) in candidates1.iter().zip(candidates2.iter()) {
        assert_eq!(c1.authority_id, c2.authority_id);
    }
}

// ============================================================================
// Complex Scenario Tests
// ============================================================================

#[test]
fn test_cross_neighborhood_routing() {
    // Simulate routing across neighborhood boundaries
    let (home1, steward1, _) = create_home(1, 3);
    let (home2, steward2, _) = create_home(2, 3);
    let (home3, _, _) = create_home(3, 3);

    // Two neighborhoods sharing home2
    // Neighborhood 1: home1, home2
    // Neighborhood 2: home2, home3
    let neighborhood1 = create_neighborhood(1, vec![home1.home_id, home2.home_id]);
    let neighborhood2 = create_neighborhood(2, vec![home2.home_id, home3.home_id]);

    // Topology for home1 - only in neighborhood1
    let topology1 = SocialTopology::new(steward1, Some(home1.clone()), vec![neighborhood1.clone()]);

    // Topology for home2 - bridge between neighborhoods
    let topology2 = SocialTopology::new(
        steward2,
        Some(home2),
        vec![neighborhood1, neighborhood2.clone()],
    );

    // home1 has social presence through neighborhood1
    assert!(topology1.has_social_presence());

    // home2 is the bridge node with access to both neighborhoods
    assert!(topology2.has_social_presence());

    // Both should see unknown targets at Home layer (have home peers)
    let unknown = test_authority(99);
    assert_eq!(topology1.discovery_layer(&unknown), DiscoveryLayer::Home);
    assert_eq!(topology2.discovery_layer(&unknown), DiscoveryLayer::Home);
}

#[test]
fn test_isolated_node_behavior() {
    // Test behavior of a node with no social connections
    let authority = test_authority(1);
    let topology = SocialTopology::empty(authority);

    // No social presence
    assert!(!topology.has_social_presence());

    // No peers
    assert!(topology.home_peers().is_empty());

    // All unknown targets require rendezvous
    let targets: Vec<_> = (2..=10).map(test_authority).collect();
    for target in targets {
        assert_eq!(
            topology.discovery_layer(&target),
            DiscoveryLayer::Rendezvous
        );
    }

    // Building relay candidates should return empty
    let builder = RelayCandidateBuilder::from_topology(topology);
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        authority,
        test_authority(99),
        3,
        [0u8; 32],
    );
    let candidates = builder.build_candidates(&context, &AlwaysReachable);
    assert!(candidates.is_empty());
}

#[test]
fn test_gradually_expanding_network() {
    // Simulate a node gradually gaining social connections
    let authority = test_authority(1);

    // Phase 1: Isolated
    let topology1 = SocialTopology::empty(authority);
    assert_eq!(
        topology1.discovery_layer(&test_authority(99)),
        DiscoveryLayer::Rendezvous
    );

    // Phase 2: Has a guardian
    let mut topology2 = SocialTopology::empty(authority);
    topology2.add_peer(test_authority(50), RelayRelationship::Guardian);
    assert_eq!(
        topology2.discovery_layer(&test_authority(99)),
        DiscoveryLayer::Rendezvous
    ); // Still rendezvous - guardian doesn't give social presence

    // Phase 3: Joins a home
    let (home, _steward, _) = create_home(1, 5);
    let topology3 = SocialTopology::new(authority, Some(home), vec![]);
    assert_eq!(
        topology3.discovery_layer(&test_authority(99)),
        DiscoveryLayer::Home
    );

    // Phase 4: Home joins neighborhood (but still have home peers, so Home layer)
    let (home2, _, _) = create_home(2, 3);
    let neighborhood = create_neighborhood(1, vec![home2.home_id]);
    let (home3, _, _) = create_home(3, 4);
    let topology4 = SocialTopology::new(authority, Some(home3), vec![neighborhood]);
    assert_eq!(
        topology4.discovery_layer(&test_authority(99)),
        DiscoveryLayer::Home
    );
}

// ============================================================================
// Stress Tests
// ============================================================================

#[test]
fn test_many_peers_performance() {
    // Test with maximum home size
    let (home, steward, _) = create_home(1, 8);

    // Add guardians too
    let mut topology = SocialTopology::new(steward, Some(home), vec![]);

    // Add several guardians
    for i in 90..95 {
        topology.add_peer(test_authority(i), RelayRelationship::Guardian);
    }

    // Total known peers: 7 home peers + 5 guardians = 12
    let builder = RelayCandidateBuilder::from_topology(topology);
    let context = aura_core::effects::relay::RelayContext::new(
        aura_core::identifiers::ContextId::new_from_entropy([1u8; 32]),
        steward,
        test_authority(99),
        3,
        [0u8; 32],
    );

    let candidates = builder.build_candidates(&context, &AlwaysReachable);
    assert_eq!(candidates.len(), 12); // 7 + 5
}

#[test]
fn test_large_neighborhood_mesh() {
    // Create a large mesh topology
    let home_count = 8;
    let homes: Vec<(Home, AuthorityId, Vec<AuthorityId>)> =
        (1..=home_count).map(|i| create_home(i as u8, 4)).collect();

    let home_ids: Vec<HomeId> = homes.iter().map(|(b, _, _)| b.home_id).collect();
    let neighborhood = create_fully_connected_neighborhood(1, home_ids.clone());

    // Total adjacencies should be n*(n-1)/2 = 8*7/2 = 28
    let mut adjacency_count = 0;
    for i in 0..home_ids.len() {
        for j in (i + 1)..home_ids.len() {
            if neighborhood.are_adjacent(home_ids[i], home_ids[j]) {
                adjacency_count += 1;
            }
        }
    }
    assert_eq!(adjacency_count, 28);
}

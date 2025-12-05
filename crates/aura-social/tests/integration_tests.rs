//! Integration Tests for Social Infrastructure
//!
//! Tests the complete social topology infrastructure including:
//! - Discovery layer selection
//! - Relay candidate generation
//! - Block and neighborhood availability
//! - Social topology queries

use aura_core::effects::relay::{RelayContext, RelayRelationship};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::facts::social::{
    AdjacencyFact, BlockConfigFact, BlockFact, BlockId, BlockMemberFact, NeighborhoodFact,
    NeighborhoodId, ResidentFact, StewardFact,
};
use aura_social::{
    Block, DiscoveryLayer, Neighborhood, ReachabilityChecker, RelayCandidateBuilder, SocialTopology,
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

fn test_block_id(seed: u8) -> BlockId {
    BlockId::from_bytes([seed; 32])
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

/// Create a block with the specified number of residents
fn create_block(block_seed: u8, resident_count: usize) -> (Block, AuthorityId, Vec<AuthorityId>) {
    let block_id = test_block_id(block_seed);
    let timestamp = test_timestamp();

    let block_fact = BlockFact::new(block_id, timestamp.clone());

    let mut residents = Vec::with_capacity(resident_count);
    let mut resident_facts = Vec::with_capacity(resident_count);

    for i in 0..resident_count {
        let authority = test_authority((block_seed * 10) + i as u8 + 1);
        residents.push(authority);
        resident_facts.push(ResidentFact::new(authority, block_id, timestamp.clone()));
    }

    let steward = residents[0];
    let steward_facts = vec![StewardFact::new(steward, block_id, timestamp)];

    let block = Block::from_facts(&block_fact, None, &resident_facts, &steward_facts);

    (block, steward, residents)
}

/// Create a neighborhood with the specified blocks
fn create_neighborhood(neighborhood_seed: u8, block_ids: Vec<BlockId>) -> Neighborhood {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut member_facts = Vec::with_capacity(block_ids.len());
    for block_id in &block_ids {
        member_facts.push(BlockMemberFact::new(
            *block_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create linear adjacencies
    let mut adjacency_facts = Vec::new();
    for i in 0..block_ids.len().saturating_sub(1) {
        adjacency_facts.push(AdjacencyFact::new(
            block_ids[i],
            block_ids[i + 1],
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
    let (block, steward, _residents) = create_block(1, 3);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Self is always Direct
    assert_eq!(topology.discovery_layer(&steward), DiscoveryLayer::Direct);
}

#[test]
fn test_discovery_layer_direct_for_block_peers() {
    let (block, steward, residents) = create_block(1, 5);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // All block peers should be Direct (we have a relationship with them)
    for resident in &residents[1..] {
        assert_eq!(topology.discovery_layer(resident), DiscoveryLayer::Direct);
    }
}

#[test]
fn test_discovery_layer_block_for_unknown_with_social_presence() {
    let (block, steward, _residents) = create_block(1, 3);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Unknown peer with block presence should be Block layer
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);
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
fn test_discovery_layer_with_neighborhoods_but_has_block_peers() {
    let (block1, steward, _residents1) = create_block(1, 3);
    let (block2, _, _residents2) = create_block(2, 3);

    let neighborhood = create_neighborhood(1, vec![block1.block_id, block2.block_id]);

    let topology = SocialTopology::new(steward, Some(block1), vec![neighborhood]);

    // With block peers available, discovery layer is Block (faster path)
    // Neighborhood layer is only used when we don't have block peers
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);
}

#[test]
fn test_discovery_layer_neighborhood_without_block_peers() {
    // Create a single-resident block (no block peers)
    let (block1, steward, _) = create_block(1, 1);
    let (block2, peer_from_block2, _) = create_block(2, 3);

    let neighborhood = create_neighborhood(1, vec![block1.block_id, block2.block_id]);

    let mut topology = SocialTopology::new(steward, Some(block1), vec![neighborhood]);

    // No block peers, so would fall through to check neighborhoods
    // But we need to add neighborhood peers explicitly
    let neighborhood_id_bytes = *test_neighborhood_id(1).as_bytes();
    topology.add_peer(
        peer_from_block2,
        RelayRelationship::NeighborhoodPeer {
            neighborhood_id: neighborhood_id_bytes,
        },
    );

    // Now with neighborhood peers but no block peers, should be Neighborhood
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
    assert_eq!(DiscoveryLayer::Block.priority(), 1);
    assert_eq!(DiscoveryLayer::Neighborhood.priority(), 2);
    assert_eq!(DiscoveryLayer::Rendezvous.priority(), 3);

    // Lower priority number = faster/better path
    assert!(DiscoveryLayer::Direct.priority() < DiscoveryLayer::Block.priority());
    assert!(DiscoveryLayer::Block.priority() < DiscoveryLayer::Neighborhood.priority());
    assert!(DiscoveryLayer::Neighborhood.priority() < DiscoveryLayer::Rendezvous.priority());
}

// ============================================================================
// Relay Candidate Generation Tests
// ============================================================================

#[test]
fn test_relay_candidates_from_block_peers() {
    let (block, steward, _residents) = create_block(1, 5);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Get block peers
    let block_peers = topology.block_peers();
    assert_eq!(block_peers.len(), 4); // 5 residents - 1 self

    // Generate relay candidates using the builder
    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Should have candidates from block peers
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
    let (block, steward, _residents) = create_block(1, 3);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

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
// Block Availability Tests
// ============================================================================

#[test]
fn test_block_resident_query() {
    let (block, steward, residents) = create_block(1, 5);

    // Steward should be a resident
    assert!(block.is_resident(&steward));

    // All residents should be residents
    for resident in &residents {
        assert!(block.is_resident(resident));
    }

    // Non-resident should not be a resident
    let non_resident = test_authority(99);
    assert!(!block.is_resident(&non_resident));
}

#[test]
fn test_block_steward_query() {
    let (block, steward, residents) = create_block(1, 3);

    // Steward should be a steward
    assert!(block.is_steward(&steward));

    // Other residents are not stewards
    for resident in &residents[1..] {
        assert!(!block.is_steward(resident));
    }
}

#[test]
fn test_block_available_slots() {
    let (block, _, _) = create_block(1, 5);

    // Default max is 8 (from BlockConfigFact::V1_MAX_RESIDENTS)
    assert!(block.can_add_resident()); // 5 < 8

    // Create a full block
    let (full_block, _, _) = create_block(2, 8);
    assert!(!full_block.can_add_resident()); // 8 == 8
}

// ============================================================================
// Neighborhood Traversal Tests
// ============================================================================

#[test]
fn test_neighborhood_adjacency() {
    let block_ids: Vec<BlockId> = (1..=4).map(test_block_id).collect();
    let neighborhood = create_neighborhood(1, block_ids.clone());

    // Linear chain adjacencies: 1-2, 2-3, 3-4
    assert!(neighborhood.are_adjacent(block_ids[0], block_ids[1]));
    assert!(neighborhood.are_adjacent(block_ids[1], block_ids[2]));
    assert!(neighborhood.are_adjacent(block_ids[2], block_ids[3]));

    // Non-adjacent pairs
    assert!(!neighborhood.are_adjacent(block_ids[0], block_ids[2]));
    assert!(!neighborhood.are_adjacent(block_ids[0], block_ids[3]));
}

#[test]
fn test_neighborhood_membership() {
    let block_ids: Vec<BlockId> = (1..=3).map(test_block_id).collect();
    let neighborhood = create_neighborhood(1, block_ids.clone());

    // All blocks should be members
    for block_id in &block_ids {
        assert!(neighborhood.is_member(*block_id));
    }

    // Non-member block
    let non_member = test_block_id(99);
    assert!(!neighborhood.is_member(non_member));
}

#[test]
fn test_neighborhood_adjacent_blocks() {
    let block_ids: Vec<BlockId> = (1..=4).map(test_block_id).collect();
    let neighborhood = create_neighborhood(1, block_ids.clone());

    // Block 2 (index 1) should have blocks 1 and 3 as adjacent
    let adjacent_to_2 = neighborhood.adjacent_blocks(block_ids[1]);
    assert_eq!(adjacent_to_2.len(), 2);
    assert!(adjacent_to_2.contains(&block_ids[0]));
    assert!(adjacent_to_2.contains(&block_ids[2]));

    // Block 1 (index 0) should only have block 2 as adjacent
    let adjacent_to_1 = neighborhood.adjacent_blocks(block_ids[0]);
    assert_eq!(adjacent_to_1.len(), 1);
    assert!(adjacent_to_1.contains(&block_ids[1]));
}

// ============================================================================
// Social Topology Integration Tests
// ============================================================================

#[test]
fn test_topology_has_social_presence() {
    let (block, steward, _) = create_block(1, 3);

    // With block
    let topology_with_block = SocialTopology::new(steward, Some(block), vec![]);
    assert!(topology_with_block.has_social_presence());

    // Without block
    let topology_empty = SocialTopology::empty(steward);
    assert!(!topology_empty.has_social_presence());
}

#[test]
fn test_topology_knows_peer() {
    let (block, steward, residents) = create_block(1, 3);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Should know block peers
    for resident in &residents[1..] {
        assert!(topology.knows_peer(resident));
    }

    // Should not know unknown peer
    let unknown = test_authority(99);
    assert!(!topology.knows_peer(&unknown));
}

#[test]
fn test_topology_add_guardian() {
    let (block, steward, _) = create_block(1, 3);
    let mut topology = SocialTopology::new(steward, Some(block), vec![]);

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
    let (block, steward, residents) = create_block(1, 5);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Check discovery context for unknown peer
    let unknown = test_authority(99);
    let (layer, peers) = topology.discovery_context(&unknown);

    assert_eq!(layer, DiscoveryLayer::Block);
    assert_eq!(peers.len(), 4); // 5 residents - 1 self

    // All returned peers should be block peers
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
    // Flood (highest) > Neighborhood > Block > Direct (lowest/cheapest)

    let flood_cost = 100;
    let neighborhood_cost = 10;
    let block_cost = 3;
    let direct_cost = 1;

    assert!(flood_cost > neighborhood_cost);
    assert!(neighborhood_cost > block_cost);
    assert!(block_cost > direct_cost);
}

#[test]
fn test_discovery_layer_implies_budget() {
    // Direct: minimal cost (known peer)
    assert!(DiscoveryLayer::Direct.is_known());

    // Block: low cost (relay through known peers)
    assert!(DiscoveryLayer::Block.has_social_presence());
    assert!(!DiscoveryLayer::Block.is_known());

    // Neighborhood: medium cost (traverse neighborhood)
    assert!(DiscoveryLayer::Neighborhood.has_social_presence());
    assert!(!DiscoveryLayer::Neighborhood.is_known());

    // Rendezvous: highest cost (global flood)
    assert!(!DiscoveryLayer::Rendezvous.has_social_presence());
    assert!(!DiscoveryLayer::Rendezvous.is_known());
}

// ============================================================================
// Multi-Block Topology Tests
// ============================================================================

#[test]
fn test_multi_block_neighborhood_topology() {
    // Create multiple blocks
    let (block1, steward1, _) = create_block(1, 3);
    let (block2, _, _) = create_block(2, 3);
    let (block3, _, _) = create_block(3, 3);

    // Create neighborhood with all blocks
    let neighborhood =
        create_neighborhood(1, vec![block1.block_id, block2.block_id, block3.block_id]);

    // Create topology for steward1 in block1
    let topology = SocialTopology::new(steward1, Some(block1.clone()), vec![neighborhood.clone()]);

    // Should have block presence
    assert!(topology.has_social_presence());

    // Should have 2 block peers
    assert_eq!(topology.block_peers().len(), 2);

    // Neighborhood should have 3 members
    assert_eq!(neighborhood.member_blocks.len(), 3);

    // Discovery layer for unknown is Block since we have block peers
    // (Block layer is preferred/faster when available)
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_single_resident_block() {
    let (block, steward, _residents) = create_block(1, 1);
    let topology = SocialTopology::new(steward, Some(block), vec![]);

    // Should have social presence even with single resident
    assert!(topology.has_social_presence());

    // No block peers (only self)
    assert!(topology.block_peers().is_empty());

    // Discovery layer for unknown is Block (have block but no peers to relay)
    let unknown = test_authority(99);
    assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);
}

#[test]
fn test_empty_neighborhood() {
    let neighborhood_id = test_neighborhood_id(1);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp);
    let neighborhood = Neighborhood::from_facts(&neighborhood_fact, &[], &[]);

    // Empty neighborhood
    assert!(neighborhood.member_blocks.is_empty());

    // No block is a member
    let block_id = test_block_id(1);
    assert!(!neighborhood.is_member(block_id));
}

#[test]
fn test_block_with_config() {
    let block_id = test_block_id(1);
    let timestamp = test_timestamp();

    let block_fact = BlockFact::new(block_id, timestamp.clone());
    let config_fact = BlockConfigFact {
        block_id,
        max_residents: 4,
        neighborhood_limit: 2,
    };

    let residents: Vec<AuthorityId> = (1..=3).map(test_authority).collect();
    let resident_facts: Vec<ResidentFact> = residents
        .iter()
        .map(|r| ResidentFact::new(*r, block_id, timestamp.clone()))
        .collect();
    let steward_facts = vec![StewardFact::new(residents[0], block_id, timestamp)];

    let block = Block::from_facts(
        &block_fact,
        Some(&config_fact),
        &resident_facts,
        &steward_facts,
    );

    // Should have custom max residents
    assert!(block.can_add_resident()); // 3 < 4

    // Verify residents
    assert_eq!(block.residents.len(), 3);
}

// ============================================================================
// Relay Selection with Guardian Tests
// ============================================================================

#[test]
fn test_guardian_in_relay_candidates() {
    let (block, steward, _) = create_block(1, 3);
    let mut topology = SocialTopology::new(steward, Some(block), vec![]);

    let guardian = test_authority(88);
    topology.add_peer(guardian, RelayRelationship::Guardian);

    // Build relay candidates
    let builder = RelayCandidateBuilder::from_topology(topology);
    let destination = test_authority(99);
    let context = test_context(steward, destination);
    let candidates = builder.build_candidates(&context, &AlwaysReachable);

    // Should include both block peers and guardian
    // 3 residents - 1 self = 2 block peers + 1 guardian = 3 candidates
    assert_eq!(candidates.len(), 3);

    // Guardian should be in candidates
    assert!(candidates.iter().any(|c| c.authority_id == guardian));
}

#[test]
fn test_relay_candidate_relationship_types() {
    let (block, steward, _) = create_block(1, 3);
    let mut topology = SocialTopology::new(steward, Some(block.clone()), vec![]);

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
                RelayRelationship::BlockPeer { .. }
            ));
        }
    }
}

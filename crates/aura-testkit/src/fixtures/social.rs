//! Social Topology Test Fixtures
//!
//! Provides reusable test fixtures for social topology testing.
//! Includes helpers for creating blocks, neighborhoods, and social topologies.

use aura_core::effects::relay::RelayRelationship;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::facts::social::{
    AdjacencyFact, BlockConfigFact, BlockFact, BlockId, BlockMemberFact, NeighborhoodFact,
    NeighborhoodId, ResidentFact, StewardFact,
};
use aura_social::{Block, Neighborhood, SocialTopology};

/// Create a test timestamp for fixtures.
pub fn test_timestamp() -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1700000000000,
        uncertainty: None,
    })
}

/// Create a test authority ID with a given seed.
pub fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Create a test block ID with a given seed.
pub fn test_block_id(seed: u8) -> BlockId {
    BlockId::from_bytes([seed; 32])
}

/// Create a test neighborhood ID with a given seed.
pub fn test_neighborhood_id(seed: u8) -> NeighborhoodId {
    NeighborhoodId::from_bytes([seed; 32])
}

/// Create a test block with a given number of residents.
///
/// # Arguments
/// * `block_seed` - Seed for the block ID
/// * `resident_count` - Number of residents to create (first resident is the steward)
///
/// # Returns
/// A tuple of (Block, steward_authority, resident_authorities)
pub fn create_test_block(
    block_seed: u8,
    resident_count: usize,
) -> (Block, AuthorityId, Vec<AuthorityId>) {
    let block_id = test_block_id(block_seed);
    let timestamp = test_timestamp();

    // Create block fact
    let block_fact = BlockFact::new(block_id, timestamp.clone());

    // Create residents (first one is the steward)
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

/// Create a test block with custom configuration.
pub fn create_test_block_with_config(
    block_seed: u8,
    resident_count: usize,
    max_residents: u8,
    neighborhood_limit: u8,
) -> (Block, AuthorityId, Vec<AuthorityId>) {
    let block_id = test_block_id(block_seed);
    let timestamp = test_timestamp();

    let block_fact = BlockFact::new(block_id, timestamp.clone());
    let config_fact = BlockConfigFact {
        block_id,
        max_residents,
        neighborhood_limit,
    };

    let mut residents = Vec::with_capacity(resident_count);
    let mut resident_facts = Vec::with_capacity(resident_count);

    for i in 0..resident_count {
        let authority = test_authority((block_seed * 10) + i as u8 + 1);
        residents.push(authority);
        resident_facts.push(ResidentFact::new(authority, block_id, timestamp.clone()));
    }

    let steward = residents[0];
    let steward_facts = vec![StewardFact::new(steward, block_id, timestamp)];

    let block = Block::from_facts(
        &block_fact,
        Some(&config_fact),
        &resident_facts,
        &steward_facts,
    );

    (block, steward, residents)
}

/// Create a test neighborhood with a given number of member blocks.
///
/// # Arguments
/// * `neighborhood_seed` - Seed for the neighborhood ID
/// * `block_count` - Number of blocks to include
///
/// # Returns
/// A tuple of (Neighborhood, member_block_ids)
pub fn create_test_neighborhood(
    neighborhood_seed: u8,
    block_count: usize,
) -> (Neighborhood, Vec<BlockId>) {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut block_ids = Vec::with_capacity(block_count);
    let mut member_facts = Vec::with_capacity(block_count);

    for i in 0..block_count {
        let block_id = test_block_id((neighborhood_seed * 10) + i as u8 + 1);
        block_ids.push(block_id);
        member_facts.push(BlockMemberFact::new(
            block_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create adjacencies between consecutive blocks (linear chain)
    let mut adjacency_facts = Vec::new();
    for i in 0..block_count.saturating_sub(1) {
        adjacency_facts.push(AdjacencyFact::new(
            block_ids[i],
            block_ids[i + 1],
            neighborhood_id,
        ));
    }

    let neighborhood =
        Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts);

    (neighborhood, block_ids)
}

/// Create a test neighborhood with fully connected adjacencies.
pub fn create_fully_connected_neighborhood(
    neighborhood_seed: u8,
    block_count: usize,
) -> (Neighborhood, Vec<BlockId>) {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut block_ids = Vec::with_capacity(block_count);
    let mut member_facts = Vec::with_capacity(block_count);

    for i in 0..block_count {
        let block_id = test_block_id((neighborhood_seed * 10) + i as u8 + 1);
        block_ids.push(block_id);
        member_facts.push(BlockMemberFact::new(
            block_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create adjacencies between all pairs of blocks
    let mut adjacency_facts = Vec::new();
    for i in 0..block_count {
        for j in (i + 1)..block_count {
            adjacency_facts.push(AdjacencyFact::new(
                block_ids[i],
                block_ids[j],
                neighborhood_id,
            ));
        }
    }

    let neighborhood =
        Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts);

    (neighborhood, block_ids)
}

/// Create a test social topology for a given authority.
///
/// # Arguments
/// * `local_authority` - The local authority's ID
/// * `block` - Optional block the authority resides in
/// * `neighborhoods` - Neighborhoods the block belongs to
pub fn create_test_topology(
    local_authority: AuthorityId,
    block: Option<Block>,
    neighborhoods: Vec<Neighborhood>,
) -> SocialTopology {
    SocialTopology::new(local_authority, block, neighborhoods)
}

/// Create a social topology with a single block (no neighborhoods).
pub fn create_single_block_topology(
    block_seed: u8,
    resident_count: usize,
) -> (SocialTopology, Block, AuthorityId, Vec<AuthorityId>) {
    let (block, steward, residents) = create_test_block(block_seed, resident_count);
    let topology = SocialTopology::new(steward, Some(block.clone()), vec![]);
    (topology, block, steward, residents)
}

/// Create a social topology with a block in a neighborhood.
pub fn create_neighborhood_topology(
    block_seed: u8,
    resident_count: usize,
    neighborhood_seed: u8,
    neighbor_block_count: usize,
) -> (SocialTopology, Block, Neighborhood, AuthorityId) {
    let (block, steward, _residents) = create_test_block(block_seed, resident_count);
    let (mut neighborhood, _block_ids) =
        create_test_neighborhood(neighborhood_seed, neighbor_block_count);

    // Add our block to the neighborhood
    let timestamp = test_timestamp();
    neighborhood.member_blocks.push(block.block_id);
    let member_fact = BlockMemberFact::new(block.block_id, neighborhood.neighborhood_id, timestamp);
    let _ = member_fact; // Use fact in production code

    let topology = SocialTopology::new(steward, Some(block.clone()), vec![neighborhood.clone()]);

    (topology, block, neighborhood, steward)
}

/// Builder for complex social topology test scenarios.
pub struct SocialTopologyBuilder {
    local_authority: AuthorityId,
    block: Option<Block>,
    neighborhoods: Vec<Neighborhood>,
    additional_peers: Vec<(AuthorityId, RelayRelationship)>,
}

impl SocialTopologyBuilder {
    /// Create a new builder for a given authority.
    pub fn new(local_authority: AuthorityId) -> Self {
        Self {
            local_authority,
            block: None,
            neighborhoods: Vec::new(),
            additional_peers: Vec::new(),
        }
    }

    /// Add a block for the authority.
    pub fn with_block(mut self, block: Block) -> Self {
        self.block = Some(block);
        self
    }

    /// Add a block with the given number of residents.
    pub fn with_new_block(mut self, block_seed: u8, resident_count: usize) -> Self {
        let (block, _steward, _residents) = create_test_block(block_seed, resident_count);
        self.block = Some(block);
        self
    }

    /// Add a neighborhood.
    pub fn with_neighborhood(mut self, neighborhood: Neighborhood) -> Self {
        self.neighborhoods.push(neighborhood);
        self
    }

    /// Add an additional peer relationship.
    pub fn with_peer(mut self, peer: AuthorityId, relationship: RelayRelationship) -> Self {
        self.additional_peers.push((peer, relationship));
        self
    }

    /// Add a guardian.
    pub fn with_guardian(mut self, guardian: AuthorityId) -> Self {
        self.additional_peers
            .push((guardian, RelayRelationship::Guardian));
        self
    }

    /// Build the topology.
    pub fn build(self) -> SocialTopology {
        let mut topology =
            SocialTopology::new(self.local_authority, self.block, self.neighborhoods);

        for (peer, relationship) in self.additional_peers {
            topology.add_peer(peer, relationship);
        }

        topology
    }
}

/// Mock data availability for testing.
pub struct MockDataAvailability {
    /// Stored data (hash -> content)
    pub stored_data: std::collections::HashMap<[u8; 32], Vec<u8>>,
    /// Replication peers
    pub peers: Vec<AuthorityId>,
}

impl MockDataAvailability {
    /// Create a new mock data availability.
    pub fn new(peers: Vec<AuthorityId>) -> Self {
        Self {
            stored_data: std::collections::HashMap::new(),
            peers,
        }
    }

    /// Store data and return its hash.
    pub fn store(&mut self, content: &[u8]) -> [u8; 32] {
        let hash = aura_core::crypto::hash::hash(content);
        self.stored_data.insert(hash, content.to_vec());
        hash
    }

    /// Retrieve data by hash.
    pub fn retrieve(&self, hash: &[u8; 32]) -> Option<Vec<u8>> {
        self.stored_data.get(hash).cloned()
    }

    /// Check if data is available locally.
    pub fn is_available(&self, hash: &[u8; 32]) -> bool {
        self.stored_data.contains_key(hash)
    }
}

/// Mock relay selector for testing.
pub struct MockRelaySelector {
    /// Pre-configured relay order
    pub relay_order: Vec<AuthorityId>,
}

impl MockRelaySelector {
    /// Create a new mock relay selector.
    pub fn new(relay_order: Vec<AuthorityId>) -> Self {
        Self { relay_order }
    }

    /// Select relays (returns pre-configured order).
    pub fn select(&self, _candidates: &[AuthorityId]) -> Vec<AuthorityId> {
        self.relay_order.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_social::DiscoveryLayer;

    #[test]
    fn test_create_test_block() {
        let (block, steward, residents) = create_test_block(1, 3);

        assert_eq!(residents.len(), 3);
        assert_eq!(steward, residents[0]);
        assert!(block.is_resident(&steward));
        assert!(block.is_steward(&steward));
        assert_eq!(block.residents.len(), 3);
    }

    #[test]
    fn test_create_test_neighborhood() {
        let (neighborhood, block_ids) = create_test_neighborhood(1, 3);

        assert_eq!(block_ids.len(), 3);
        assert_eq!(neighborhood.member_blocks.len(), 3);
        // Linear chain: 0-1, 1-2
        assert!(neighborhood.are_adjacent(block_ids[0], block_ids[1]));
        assert!(neighborhood.are_adjacent(block_ids[1], block_ids[2]));
        assert!(!neighborhood.are_adjacent(block_ids[0], block_ids[2]));
    }

    #[test]
    fn test_create_fully_connected_neighborhood() {
        let (neighborhood, block_ids) = create_fully_connected_neighborhood(1, 3);

        assert_eq!(block_ids.len(), 3);
        // All pairs should be adjacent
        assert!(neighborhood.are_adjacent(block_ids[0], block_ids[1]));
        assert!(neighborhood.are_adjacent(block_ids[1], block_ids[2]));
        assert!(neighborhood.are_adjacent(block_ids[0], block_ids[2]));
    }

    #[test]
    fn test_single_block_topology() {
        let (topology, _block, steward, residents) = create_single_block_topology(1, 3);

        assert!(topology.has_social_presence());
        let block_peers = topology.block_peers();
        assert_eq!(block_peers.len(), 2); // 3 residents - 1 self = 2 peers

        // All other residents should be block peers
        for resident in &residents[1..] {
            assert!(block_peers.contains(resident));
        }
        assert!(!block_peers.contains(&steward));
    }

    #[test]
    fn test_topology_builder() {
        let local = test_authority(1);
        let guardian = test_authority(99);

        let topology = SocialTopologyBuilder::new(local)
            .with_new_block(1, 3)
            .with_guardian(guardian)
            .build();

        assert!(topology.has_social_presence());
        assert!(topology.knows_peer(&guardian));
    }

    #[test]
    fn test_mock_data_availability() {
        let peers = vec![test_authority(1), test_authority(2)];
        let mut da = MockDataAvailability::new(peers);

        let content = b"test data";
        let hash = da.store(content);

        assert!(da.is_available(&hash));
        assert_eq!(da.retrieve(&hash), Some(content.to_vec()));
    }

    #[test]
    fn test_discovery_layer_with_fixtures() {
        let (topology, _block, steward, residents) = create_single_block_topology(1, 3);

        // Local authority (steward) should be direct to self
        assert_eq!(topology.discovery_layer(&steward), DiscoveryLayer::Direct);

        // Block peers are Direct because we have a relationship with them
        assert_eq!(
            topology.discovery_layer(&residents[1]),
            DiscoveryLayer::Direct
        );

        // Unknown peer with block presence = Block (can relay through block peers)
        let unknown = test_authority(99);
        assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);

        // Test topology without social presence
        let empty_topology = SocialTopologyBuilder::new(steward).build();
        assert_eq!(
            empty_topology.discovery_layer(&unknown),
            DiscoveryLayer::Rendezvous
        );
    }
}

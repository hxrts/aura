//! Social Topology Test Fixtures
//!
//! Provides reusable test fixtures for social topology testing.
//! Includes helpers for creating homes, neighborhoods, and social topologies.

use aura_core::effects::relay::RelayRelationship;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_social::facts::{
    AdjacencyFact, HomeConfigFact, HomeFact, HomeId, HomeMemberFact, NeighborhoodFact,
    NeighborhoodId, ResidentFact, StewardFact,
};
use aura_social::{Home, Neighborhood, SocialTopology};

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

/// Create a test home ID with a given seed.
pub fn test_home_id(seed: u8) -> HomeId {
    HomeId::from_bytes([seed; 32])
}

/// Create a test neighborhood ID with a given seed.
pub fn test_neighborhood_id(seed: u8) -> NeighborhoodId {
    NeighborhoodId::from_bytes([seed; 32])
}

/// Create a test home with a given number of residents.
///
/// # Arguments
/// * `home_seed` - Seed for the home ID
/// * `resident_count` - Number of residents to create (first resident is the steward)
///
/// # Returns
/// A tuple of (Home, steward_authority, resident_authorities)
pub fn create_test_home(
    home_seed: u8,
    resident_count: usize,
) -> (Home, AuthorityId, Vec<AuthorityId>) {
    let home_id = test_home_id(home_seed);
    let timestamp = test_timestamp();

    // Create home fact
    let home_fact = HomeFact::new(home_id, timestamp.clone());

    // Create residents (first one is the steward)
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

/// Create a test home with custom configuration.
pub fn create_test_home_with_config(
    home_seed: u8,
    resident_count: usize,
    max_residents: u8,
    neighborhood_limit: u8,
) -> (Home, AuthorityId, Vec<AuthorityId>) {
    let home_id = test_home_id(home_seed);
    let timestamp = test_timestamp();

    let home_fact = HomeFact::new(home_id, timestamp.clone());
    let config_fact = HomeConfigFact {
        home_id,
        max_residents,
        neighborhood_limit,
    };

    let mut residents = Vec::with_capacity(resident_count);
    let mut resident_facts = Vec::with_capacity(resident_count);

    for i in 0..resident_count {
        let authority = test_authority((home_seed * 10) + i as u8 + 1);
        residents.push(authority);
        resident_facts.push(ResidentFact::new(authority, home_id, timestamp.clone()));
    }

    let steward = residents[0];
    let steward_facts = vec![StewardFact::new(steward, home_id, timestamp)];

    let home = Home::from_facts(
        &home_fact,
        Some(&config_fact),
        &resident_facts,
        &steward_facts,
    );

    (home, steward, residents)
}

/// Create a test neighborhood with a given number of member homes.
///
/// # Arguments
/// * `neighborhood_seed` - Seed for the neighborhood ID
/// * `home_count` - Number of homes to include
///
/// # Returns
/// A tuple of (Neighborhood, member_home_ids)
pub fn create_test_neighborhood(
    neighborhood_seed: u8,
    home_count: usize,
) -> (Neighborhood, Vec<HomeId>) {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut home_ids = Vec::with_capacity(home_count);
    let mut member_facts = Vec::with_capacity(home_count);

    for i in 0..home_count {
        let home_id = test_home_id((neighborhood_seed * 10) + i as u8 + 1);
        home_ids.push(home_id);
        member_facts.push(HomeMemberFact::new(
            home_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create adjacencies between consecutive homes (linear chain)
    let mut adjacency_facts = Vec::new();
    for i in 0..home_count.saturating_sub(1) {
        adjacency_facts.push(AdjacencyFact::new(
            home_ids[i],
            home_ids[i + 1],
            neighborhood_id,
        ));
    }

    let neighborhood =
        Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts);

    (neighborhood, home_ids)
}

/// Create a test neighborhood with fully connected adjacencies.
pub fn create_fully_connected_neighborhood(
    neighborhood_seed: u8,
    home_count: usize,
) -> (Neighborhood, Vec<HomeId>) {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();

    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let mut home_ids = Vec::with_capacity(home_count);
    let mut member_facts = Vec::with_capacity(home_count);

    for i in 0..home_count {
        let home_id = test_home_id((neighborhood_seed * 10) + i as u8 + 1);
        home_ids.push(home_id);
        member_facts.push(HomeMemberFact::new(
            home_id,
            neighborhood_id,
            timestamp.clone(),
        ));
    }

    // Create adjacencies between all pairs of homes
    let mut adjacency_facts = Vec::new();
    for i in 0..home_count {
        for j in (i + 1)..home_count {
            adjacency_facts.push(AdjacencyFact::new(
                home_ids[i],
                home_ids[j],
                neighborhood_id,
            ));
        }
    }

    let neighborhood =
        Neighborhood::from_facts(&neighborhood_fact, &member_facts, &adjacency_facts);

    (neighborhood, home_ids)
}

/// Create a test social topology for a given authority.
///
/// # Arguments
/// * `local_authority` - The local authority's ID
/// * `home` - Optional home the authority resides in
/// * `neighborhoods` - Neighborhoods the home belongs to
pub fn create_test_topology(
    local_authority: AuthorityId,
    home: Option<Home>,
    neighborhoods: Vec<Neighborhood>,
) -> SocialTopology {
    SocialTopology::new(local_authority, home, neighborhoods)
}

/// Create a social topology with a single home (no neighborhoods).
pub fn create_single_home_topology(
    home_seed: u8,
    resident_count: usize,
) -> (SocialTopology, Home, AuthorityId, Vec<AuthorityId>) {
    let (home, steward, residents) = create_test_home(home_seed, resident_count);
    let topology = SocialTopology::new(steward, Some(home.clone()), vec![]);
    (topology, home, steward, residents)
}

/// Create a social topology with a home in a neighborhood.
pub fn create_neighborhood_topology(
    home_seed: u8,
    resident_count: usize,
    neighborhood_seed: u8,
    neighbor_home_count: usize,
) -> (SocialTopology, Home, Neighborhood, AuthorityId) {
    let (home, steward, _residents) = create_test_home(home_seed, resident_count);
    let (mut neighborhood, _home_ids) =
        create_test_neighborhood(neighborhood_seed, neighbor_home_count);

    // Add our home to the neighborhood
    let timestamp = test_timestamp();
    neighborhood.member_homes.push(home.home_id);
    let member_fact =
        HomeMemberFact::new(home.home_id, neighborhood.neighborhood_id, timestamp);
    let _ = member_fact; // Use fact in production code

    let topology = SocialTopology::new(steward, Some(home.clone()), vec![neighborhood.clone()]);

    (topology, home, neighborhood, steward)
}

/// Builder for complex social topology test scenarios.
pub struct SocialTopologyBuilder {
    local_authority: AuthorityId,
    home: Option<Home>,
    neighborhoods: Vec<Neighborhood>,
    additional_peers: Vec<(AuthorityId, RelayRelationship)>,
}

impl SocialTopologyBuilder {
    /// Create a new builder for a given authority.
    pub fn new(local_authority: AuthorityId) -> Self {
        Self {
            local_authority,
            home: None,
            neighborhoods: Vec::new(),
            additional_peers: Vec::new(),
        }
    }

    /// Add a home for the authority.
    pub fn with_home(mut self, home: Home) -> Self {
        self.home = Some(home);
        self
    }

    /// Add a home with the given number of residents.
    pub fn with_new_home(mut self, home_seed: u8, resident_count: usize) -> Self {
        let (home, _steward, _residents) = create_test_home(home_seed, resident_count);
        self.home = Some(home);
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
            SocialTopology::new(self.local_authority, self.home, self.neighborhoods);

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
    fn test_create_test_home() {
        let (home, steward, residents) = create_test_home(1, 3);

        assert_eq!(residents.len(), 3);
        assert_eq!(steward, residents[0]);
        assert!(home.is_resident(&steward));
        assert!(home.is_steward(&steward));
        assert_eq!(home.residents.len(), 3);
    }

    #[test]
    fn test_create_test_neighborhood() {
        let (neighborhood, home_ids) = create_test_neighborhood(1, 3);

        assert_eq!(home_ids.len(), 3);
        assert_eq!(neighborhood.member_homes.len(), 3);
        // Linear chain: 0-1, 1-2
        assert!(neighborhood.are_adjacent(home_ids[0], home_ids[1]));
        assert!(neighborhood.are_adjacent(home_ids[1], home_ids[2]));
        assert!(!neighborhood.are_adjacent(home_ids[0], home_ids[2]));
    }

    #[test]
    fn test_create_fully_connected_neighborhood() {
        let (neighborhood, home_ids) = create_fully_connected_neighborhood(1, 3);

        assert_eq!(home_ids.len(), 3);
        // All pairs should be adjacent
        assert!(neighborhood.are_adjacent(home_ids[0], home_ids[1]));
        assert!(neighborhood.are_adjacent(home_ids[1], home_ids[2]));
        assert!(neighborhood.are_adjacent(home_ids[0], home_ids[2]));
    }

    #[test]
    fn test_single_home_topology() {
        let (topology, _home, steward, residents) = create_single_home_topology(1, 3);

        assert!(topology.has_social_presence());
        let home_peers = topology.home_peers();
        assert_eq!(home_peers.len(), 2); // 3 residents - 1 self = 2 peers

        // All other residents should be home peers
        for resident in &residents[1..] {
            assert!(home_peers.contains(resident));
        }
        assert!(!home_peers.contains(&steward));
    }

    #[test]
    fn test_topology_builder() {
        let local = test_authority(1);
        let guardian = test_authority(99);

        let topology = SocialTopologyBuilder::new(local)
            .with_new_home(1, 3)
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
        let (topology, _home, steward, residents) = create_single_home_topology(1, 3);

        // Local authority (steward) should be direct to self
        assert_eq!(topology.discovery_layer(&steward), DiscoveryLayer::Direct);

        // Home peers are Direct because we have a relationship with them
        assert_eq!(
            topology.discovery_layer(&residents[1]),
            DiscoveryLayer::Direct
        );

        // Unknown peer with home presence = Home (can relay through home peers)
        let unknown = test_authority(99);
        assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);

        // Test topology without social presence
        let empty_topology = SocialTopologyBuilder::new(steward).build();
        assert_eq!(
            empty_topology.discovery_layer(&unknown),
            DiscoveryLayer::Rendezvous
        );
    }
}

//! Social Topology - Aggregated view for relay and discovery
//!
//! Provides a unified view of the social topology for use in relay selection
//! and peer discovery.

use crate::{Home, Neighborhood};
use aura_core::{
    effects::relay::{RelayCandidate, RelayRelationship},
    identifiers::AuthorityId,
};
use crate::facts::HomeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Aggregated view of social topology for relay and discovery.
///
/// This view provides efficient queries for:
/// - Finding relay candidates based on social relationships
/// - Discovering peers at different trust levels
/// - Building candidate lists for relay selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialTopology {
    /// Our authority
    local_authority: AuthorityId,
    /// Home we reside in (if any)
    home: Option<Home>,
    /// Neighborhoods our home belongs to
    neighborhoods: Vec<Neighborhood>,
    /// Cached peer relationships for efficient lookup
    peer_relationships: HashMap<AuthorityId, RelayRelationship>,
}

impl SocialTopology {
    /// Create a new social topology.
    ///
    /// # Arguments
    /// * `local_authority` - Our authority ID
    /// * `home` - The home we reside in (if any)
    /// * `neighborhoods` - Neighborhoods our home belongs to
    pub fn new(
        local_authority: AuthorityId,
        home: Option<Home>,
        neighborhoods: Vec<Neighborhood>,
    ) -> Self {
        let mut topology = Self {
            local_authority,
            home,
            neighborhoods,
            peer_relationships: HashMap::new(),
        };
        topology.rebuild_peer_cache();
        topology
    }

    /// Create an empty topology for an authority with no social presence.
    pub fn empty(local_authority: AuthorityId) -> Self {
        Self {
            local_authority,
            home: None,
            neighborhoods: Vec::new(),
            peer_relationships: HashMap::new(),
        }
    }

    /// Rebuild the peer relationship cache from blocks and neighborhoods.
    fn rebuild_peer_cache(&mut self) {
        self.peer_relationships.clear();

        // Add home peers
        if let Some(home_ref) = &self.home {
            let home_id_bytes = *home_ref.home_id.as_bytes();
            for resident in &home_ref.residents {
                if resident != &self.local_authority {
                    self.peer_relationships.insert(
                        *resident,
                        RelayRelationship::HomePeer {
                            home_id: home_id_bytes,
                        },
                    );
                }
            }

            // Add neighborhood peers (excluding home peers already added)
            for neighborhood in &self.neighborhoods {
                if neighborhood.is_member(home_ref.home_id) {
                    let _neighborhood_id_bytes = *neighborhood.neighborhood_id.as_bytes();

                    // We would need to know residents of other blocks
                    // For now, we track the neighborhoods we're in but can't enumerate
                    // all neighborhood peers without additional data
                    // This will be populated by higher-level code that has access
                    // to all home residents in the neighborhood
                }
            }
        }
    }

    /// Add a peer relationship manually.
    ///
    /// This is used to populate neighborhood peer relationships which require
    /// knowledge of residents in other blocks.
    pub fn add_peer(&mut self, peer: AuthorityId, relationship: RelayRelationship) {
        // Don't downgrade home peer to neighborhood peer
        if let Some(existing) = self.peer_relationships.get(&peer) {
            if existing.priority() < relationship.priority() {
                return; // Keep higher priority relationship
            }
        }
        self.peer_relationships.insert(peer, relationship);
    }

    /// Get the relationship type with a peer.
    pub fn relationship_with(&self, peer: &AuthorityId) -> Option<RelayRelationship> {
        self.peer_relationships.get(peer).copied()
    }

    /// Check if we have any relationship with a peer.
    pub fn knows_peer(&self, peer: &AuthorityId) -> bool {
        self.peer_relationships.contains_key(peer)
    }

    /// Get all home peers (co-residents in our home).
    pub fn home_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships
            .iter()
            .filter(|(_, rel)| rel.is_home_peer())
            .map(|(auth, _)| *auth)
            .collect()
    }

    /// Get all neighborhood peers (members of adjacent homes in shared neighborhoods).
    pub fn neighborhood_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships
            .iter()
            .filter(|(_, rel)| rel.is_neighborhood_peer())
            .map(|(auth, _)| *auth)
            .collect()
    }

    /// Get all known peers (both home and neighborhood).
    pub fn all_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships.keys().copied().collect()
    }

    /// Get our home (if any).
    pub fn our_block(&self) -> Option<&Home> {
        self.home.as_ref()
    }

    /// Get our home ID (if any).
    pub fn our_home_id(&self) -> Option<HomeId> {
        self.home.as_ref().map(|b| b.home_id)
    }

    /// Get neighborhoods we belong to.
    pub fn our_neighborhoods(&self) -> &[Neighborhood] {
        &self.neighborhoods
    }

    /// Get our local authority.
    pub fn local_authority(&self) -> AuthorityId {
        self.local_authority
    }

    /// Check if we have social presence (are in a home).
    pub fn has_social_presence(&self) -> bool {
        self.home.is_some()
    }

    /// Build relay candidates for a target destination.
    ///
    /// Returns candidates in priority order (home peers first, then neighborhood peers).
    ///
    /// # Arguments
    /// * `destination` - The target authority we're trying to reach
    /// * `reachability` - Function to check if a peer is currently reachable
    pub fn build_relay_candidates<F>(
        &self,
        destination: &AuthorityId,
        mut reachability: F,
    ) -> Vec<RelayCandidate>
    where
        F: FnMut(&AuthorityId) -> bool,
    {
        let mut candidates = Vec::new();

        // Add all known peers as candidates
        for (peer, relationship) in &self.peer_relationships {
            // Skip the destination itself (no need to relay to yourself)
            if peer == destination {
                continue;
            }

            let reachable = reachability(peer);
            candidates.push(RelayCandidate::new(*peer, *relationship, reachable));
        }

        // Sort by relationship priority (lower = higher priority)
        candidates.sort_by_key(|c| c.relationship.priority());

        candidates
    }

    /// Get the count of peers by relationship type.
    pub fn peer_counts(&self) -> PeerCounts {
        let mut counts = PeerCounts::default();
        for rel in self.peer_relationships.values() {
            match rel {
                RelayRelationship::HomePeer { .. } => counts.home_peers += 1,
                RelayRelationship::NeighborhoodPeer { .. } => counts.neighborhood_peers += 1,
                RelayRelationship::Guardian => counts.guardians += 1,
            }
        }
        counts
    }

    /// Check if a peer can relay for a target within this topology.
    ///
    /// Both home peers and neighborhood peers can relay for anyone in the neighborhood.
    pub fn can_relay_for(&self, relay: &AuthorityId, _target: &AuthorityId) -> bool {
        // If we don't know the relay, they can't relay for us
        if !self.knows_peer(relay) {
            return false;
        }

        // All known peers (home or neighborhood) can relay for any target
        // within the neighborhood scope
        // The actual validation happens at the relay node
        true
    }

    /// Determine the discovery layer for reaching a target.
    ///
    /// Returns the most appropriate discovery strategy based on our
    /// relationship with the target:
    ///
    /// - `Direct`: Target is in our peer relationships (we know them)
    /// - `Home`: Target unknown, but we have home presence to relay through
    /// - `Neighborhood`: Target unknown, but we have neighborhood presence
    /// - `Rendezvous`: No social presence, must use external discovery
    ///
    /// # Arguments
    /// * `target` - The authority we're trying to discover/reach
    ///
    /// # Example
    ///
    /// ```ignore
    /// let layer = topology.discovery_layer(&target);
    /// match layer {
    ///     DiscoveryLayer::Direct => connect_directly(&target),
    ///     DiscoveryLayer::Home => relay_through_home_peers(&target),
    ///     DiscoveryLayer::Neighborhood => relay_through_neighborhood(&target),
    ///     DiscoveryLayer::Rendezvous => use_rendezvous_discovery(&target),
    /// }
    /// ```
    pub fn discovery_layer(&self, target: &AuthorityId) -> DiscoveryLayer {
        // If target is self, consider it direct
        if target == &self.local_authority {
            return DiscoveryLayer::Direct;
        }

        // Check if we have a direct relationship with the target
        if let Some(relationship) = self.peer_relationships.get(target) {
            // Known peer - determine layer based on relationship type
            match relationship {
                RelayRelationship::HomePeer { .. } => return DiscoveryLayer::Direct,
                RelayRelationship::NeighborhoodPeer { .. } => return DiscoveryLayer::Direct,
                RelayRelationship::Guardian => return DiscoveryLayer::Direct,
            }
        }

        // Target unknown - what resources do we have?

        // Check if we have home presence (can relay through home peers)
        if self.home.is_some() && !self.home_peers().is_empty() {
            return DiscoveryLayer::Home;
        }

        // Check if we have neighborhood presence (can relay through neighborhood)
        if !self.neighborhoods.is_empty() && !self.neighborhood_peers().is_empty() {
            return DiscoveryLayer::Neighborhood;
        }

        // Check if we have any social presence at all
        if self.has_social_presence() {
            // We're in a home but have no peers yet
            return DiscoveryLayer::Home;
        }

        // No social presence - must use rendezvous/flooding
        DiscoveryLayer::Rendezvous
    }

    /// Get the discovery layer with detailed context.
    ///
    /// Returns both the layer and the relevant peers for that layer.
    pub fn discovery_context(&self, target: &AuthorityId) -> (DiscoveryLayer, Vec<AuthorityId>) {
        let layer = self.discovery_layer(target);

        let peers = match layer {
            DiscoveryLayer::Direct => {
                // Return the target itself if known
                if self.knows_peer(target) || target == &self.local_authority {
                    vec![*target]
                } else {
                    vec![]
                }
            }
            DiscoveryLayer::Home => self.home_peers(),
            DiscoveryLayer::Neighborhood => {
                // Include both home and neighborhood peers for neighborhood-level relay
                let mut peers = self.home_peers();
                peers.extend(self.neighborhood_peers());
                peers
            }
            DiscoveryLayer::Rendezvous => {
                // No known peers - empty vector
                vec![]
            }
        };

        (layer, peers)
    }
}

/// Discovery layer indicating the best strategy to reach a target.
///
/// Ordered from least to most specific relationship:
/// - Rendezvous: No relationship, need external discovery
/// - Neighborhood: Have traversal capability via neighborhood
/// - Home: Target is in same home (can relay through co-residents)
/// - Direct: Target is personally known (in peer relationships)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscoveryLayer {
    /// No relationship with target - must use rendezvous/flooding discovery.
    Rendezvous,
    /// We have neighborhood presence and can use traversal.
    /// Target may be reachable via multi-hop neighborhood relay.
    Neighborhood,
    /// Target is reachable via home-level relay.
    /// We have co-residents who may be able to forward.
    Home,
    /// Target is personally known - we have a direct relationship.
    /// Can attempt direct connection without relay.
    Direct,
}

impl DiscoveryLayer {
    /// Get the priority of this layer (lower = closer relationship).
    pub fn priority(self) -> u8 {
        match self {
            DiscoveryLayer::Direct => 0,
            DiscoveryLayer::Home => 1,
            DiscoveryLayer::Neighborhood => 2,
            DiscoveryLayer::Rendezvous => 3,
        }
    }

    /// Check if this layer represents a known relationship.
    pub fn is_known(self) -> bool {
        matches!(self, DiscoveryLayer::Direct)
    }

    /// Check if this layer has social presence (home or neighborhood).
    pub fn has_social_presence(self) -> bool {
        matches!(
            self,
            DiscoveryLayer::Direct | DiscoveryLayer::Home | DiscoveryLayer::Neighborhood
        )
    }
}

/// Counts of peers by relationship type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerCounts {
    /// Number of home peers
    pub home_peers: usize,
    /// Number of neighborhood peers
    pub neighborhood_peers: usize,
    /// Number of guardians
    pub guardians: usize,
}

impl PeerCounts {
    /// Get total peer count.
    pub fn total(&self) -> usize {
        self.home_peers + self.neighborhood_peers + self.guardians
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::NeighborhoodId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_empty_topology() {
        let authority = test_authority(1);
        let topology = SocialTopology::empty(authority);

        assert!(!topology.has_social_presence());
        assert!(topology.home_peers().is_empty());
        assert!(topology.neighborhood_peers().is_empty());
    }

    #[test]
    fn test_topology_with_block() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut home_state = Home::new_empty(home_id);
        home_state.residents = vec![local, peer1, peer2];

        let topology = SocialTopology::new(local, Some(home_state), vec![]);

        assert!(topology.has_social_presence());
        let home_peers = topology.home_peers();
        assert_eq!(home_peers.len(), 2);
        assert!(home_peers.contains(&peer1));
        assert!(home_peers.contains(&peer2));
        assert!(!home_peers.contains(&local)); // Self excluded
    }

    #[test]
    fn test_relationship_priority() {
        let local = test_authority(1);
        let peer = test_authority(2);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);

        // Add as neighborhood peer first
        topology.add_peer(
            peer,
            RelayRelationship::NeighborhoodPeer {
                neighborhood_id: *neighborhood_id.as_bytes(),
            },
        );
        assert!(topology
            .relationship_with(&peer)
            .unwrap()
            .is_neighborhood_peer());

        // Try to "upgrade" to home peer - should succeed
        topology.add_peer(
            peer,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        assert!(topology.relationship_with(&peer).unwrap().is_home_peer());

        // Try to "downgrade" back to neighborhood peer - should be ignored
        topology.add_peer(
            peer,
            RelayRelationship::NeighborhoodPeer {
                neighborhood_id: *neighborhood_id.as_bytes(),
            },
        );
        assert!(topology.relationship_with(&peer).unwrap().is_home_peer());
    }

    #[test]
    fn test_build_relay_candidates() {
        let local = test_authority(1);
        let home_peer = test_authority(2);
        let neighborhood_peer = test_authority(3);
        let target = test_authority(4);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            home_peer,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        topology.add_peer(
            neighborhood_peer,
            RelayRelationship::NeighborhoodPeer {
                neighborhood_id: *neighborhood_id.as_bytes(),
            },
        );

        // All peers are reachable
        let candidates = topology.build_relay_candidates(&target, |_| true);

        assert_eq!(candidates.len(), 2);
        // Home peer should be first (higher priority)
        assert!(candidates[0].relationship.is_home_peer());
        assert!(candidates[1].relationship.is_neighborhood_peer());
    }

    #[test]
    fn test_peer_counts() {
        let local = test_authority(1);
        let home_id = HomeId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            test_authority(2),
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        topology.add_peer(
            test_authority(3),
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );
        topology.add_peer(
            test_authority(4),
            RelayRelationship::NeighborhoodPeer {
                neighborhood_id: *neighborhood_id.as_bytes(),
            },
        );
        topology.add_peer(test_authority(5), RelayRelationship::Guardian);

        let counts = topology.peer_counts();
        assert_eq!(counts.home_peers, 2);
        assert_eq!(counts.neighborhood_peers, 1);
        assert_eq!(counts.guardians, 1);
        assert_eq!(counts.total(), 4);
    }

    #[test]
    fn test_discovery_layer_self() {
        let local = test_authority(1);
        let topology = SocialTopology::empty(local);

        // Discovering self should always be Direct
        assert_eq!(topology.discovery_layer(&local), DiscoveryLayer::Direct);
    }

    #[test]
    fn test_discovery_layer_known_peer() {
        let local = test_authority(1);
        let peer = test_authority(2);
        let home_id = HomeId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer,
            RelayRelationship::HomePeer {
                home_id: *home_id.as_bytes(),
            },
        );

        // Known peer should be Direct
        assert_eq!(topology.discovery_layer(&peer), DiscoveryLayer::Direct);
    }

    #[test]
    fn test_discovery_layer_unknown_with_block() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let unknown = test_authority(99);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut home_state = Home::new_empty(home_id);
        home_state.residents = vec![local, peer1];

        let topology = SocialTopology::new(local, Some(home_state), vec![]);

        // Unknown target with home presence should be Home layer
        assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Home);
    }

    #[test]
    fn test_discovery_layer_no_social_presence() {
        let local = test_authority(1);
        let unknown = test_authority(99);

        let topology = SocialTopology::empty(local);

        // No social presence should be Rendezvous
        assert_eq!(
            topology.discovery_layer(&unknown),
            DiscoveryLayer::Rendezvous
        );
    }

    #[test]
    fn test_discovery_layer_priority() {
        // Test that DiscoveryLayer priorities are correct
        assert!(DiscoveryLayer::Direct.priority() < DiscoveryLayer::Home.priority());
        assert!(DiscoveryLayer::Home.priority() < DiscoveryLayer::Neighborhood.priority());
        assert!(DiscoveryLayer::Neighborhood.priority() < DiscoveryLayer::Rendezvous.priority());
    }

    #[test]
    fn test_discovery_layer_predicates() {
        assert!(DiscoveryLayer::Direct.is_known());
        assert!(!DiscoveryLayer::Home.is_known());
        assert!(!DiscoveryLayer::Neighborhood.is_known());
        assert!(!DiscoveryLayer::Rendezvous.is_known());

        assert!(DiscoveryLayer::Direct.has_social_presence());
        assert!(DiscoveryLayer::Home.has_social_presence());
        assert!(DiscoveryLayer::Neighborhood.has_social_presence());
        assert!(!DiscoveryLayer::Rendezvous.has_social_presence());
    }

    #[test]
    fn test_discovery_context() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);
        let unknown = test_authority(99);

        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut home_state = Home::new_empty(home_id);
        home_state.residents = vec![local, peer1, peer2];

        let topology = SocialTopology::new(local, Some(home_state), vec![]);

        // Known peer should return Direct with the peer
        let (layer, peers) = topology.discovery_context(&peer1);
        assert_eq!(layer, DiscoveryLayer::Direct);
        assert_eq!(peers, vec![peer1]);

        // Unknown target should return Home with home peers
        let (layer, peers) = topology.discovery_context(&unknown);
        assert_eq!(layer, DiscoveryLayer::Home);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }
}

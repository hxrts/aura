//! Social Topology - Aggregated view for relay and discovery
//!
//! Provides a unified view of the social topology for use in relay selection
//! and peer discovery.

use crate::{Block, Neighborhood};
use aura_core::{
    effects::relay::{RelayCandidate, RelayRelationship},
    identifiers::AuthorityId,
};
use aura_journal::facts::social::BlockId;
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
    /// Block we reside in (if any)
    block: Option<Block>,
    /// Neighborhoods our block belongs to
    neighborhoods: Vec<Neighborhood>,
    /// Cached peer relationships for efficient lookup
    peer_relationships: HashMap<AuthorityId, RelayRelationship>,
}

impl SocialTopology {
    /// Create a new social topology.
    ///
    /// # Arguments
    /// * `local_authority` - Our authority ID
    /// * `block` - The block we reside in (if any)
    /// * `neighborhoods` - Neighborhoods our block belongs to
    pub fn new(
        local_authority: AuthorityId,
        block: Option<Block>,
        neighborhoods: Vec<Neighborhood>,
    ) -> Self {
        let mut topology = Self {
            local_authority,
            block,
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
            block: None,
            neighborhoods: Vec::new(),
            peer_relationships: HashMap::new(),
        }
    }

    /// Rebuild the peer relationship cache from blocks and neighborhoods.
    fn rebuild_peer_cache(&mut self) {
        self.peer_relationships.clear();

        // Add block peers
        if let Some(block) = &self.block {
            let block_id_bytes = *block.block_id.as_bytes();
            for resident in &block.residents {
                if resident != &self.local_authority {
                    self.peer_relationships.insert(
                        *resident,
                        RelayRelationship::BlockPeer {
                            block_id: block_id_bytes,
                        },
                    );
                }
            }

            // Add neighborhood peers (excluding block peers already added)
            for neighborhood in &self.neighborhoods {
                if neighborhood.is_member(block.block_id) {
                    let _neighborhood_id_bytes = *neighborhood.neighborhood_id.as_bytes();

                    // We would need to know residents of other blocks
                    // For now, we track the neighborhoods we're in but can't enumerate
                    // all neighborhood peers without additional data
                    // This will be populated by higher-level code that has access
                    // to all block residents in the neighborhood
                }
            }
        }
    }

    /// Add a peer relationship manually.
    ///
    /// This is used to populate neighborhood peer relationships which require
    /// knowledge of residents in other blocks.
    pub fn add_peer(&mut self, peer: AuthorityId, relationship: RelayRelationship) {
        // Don't downgrade block peer to neighborhood peer
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

    /// Get all block peers (co-residents in our block).
    pub fn block_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships
            .iter()
            .filter(|(_, rel)| rel.is_block_peer())
            .map(|(auth, _)| *auth)
            .collect()
    }

    /// Get all neighborhood peers (members of adjacent blocks in shared neighborhoods).
    pub fn neighborhood_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships
            .iter()
            .filter(|(_, rel)| rel.is_neighborhood_peer())
            .map(|(auth, _)| *auth)
            .collect()
    }

    /// Get all known peers (both block and neighborhood).
    pub fn all_peers(&self) -> Vec<AuthorityId> {
        self.peer_relationships.keys().copied().collect()
    }

    /// Get our block (if any).
    pub fn our_block(&self) -> Option<&Block> {
        self.block.as_ref()
    }

    /// Get our block ID (if any).
    pub fn our_block_id(&self) -> Option<BlockId> {
        self.block.as_ref().map(|b| b.block_id)
    }

    /// Get neighborhoods we belong to.
    pub fn our_neighborhoods(&self) -> &[Neighborhood] {
        &self.neighborhoods
    }

    /// Get our local authority.
    pub fn local_authority(&self) -> AuthorityId {
        self.local_authority
    }

    /// Check if we have social presence (are in a block).
    pub fn has_social_presence(&self) -> bool {
        self.block.is_some()
    }

    /// Build relay candidates for a target destination.
    ///
    /// Returns candidates in priority order (block peers first, then neighborhood peers).
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
                RelayRelationship::BlockPeer { .. } => counts.block_peers += 1,
                RelayRelationship::NeighborhoodPeer { .. } => counts.neighborhood_peers += 1,
                RelayRelationship::Guardian => counts.guardians += 1,
            }
        }
        counts
    }

    /// Check if a peer can relay for a target within this topology.
    ///
    /// Both block peers and neighborhood peers can relay for anyone in the neighborhood.
    pub fn can_relay_for(&self, relay: &AuthorityId, _target: &AuthorityId) -> bool {
        // If we don't know the relay, they can't relay for us
        if !self.knows_peer(relay) {
            return false;
        }

        // All known peers (block or neighborhood) can relay for any target
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
    /// - `Block`: Target unknown, but we have block presence to relay through
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
    ///     DiscoveryLayer::Block => relay_through_block_peers(&target),
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
                RelayRelationship::BlockPeer { .. } => return DiscoveryLayer::Direct,
                RelayRelationship::NeighborhoodPeer { .. } => return DiscoveryLayer::Direct,
                RelayRelationship::Guardian => return DiscoveryLayer::Direct,
            }
        }

        // Target unknown - what resources do we have?

        // Check if we have block presence (can relay through block peers)
        if self.block.is_some() && !self.block_peers().is_empty() {
            return DiscoveryLayer::Block;
        }

        // Check if we have neighborhood presence (can relay through neighborhood)
        if !self.neighborhoods.is_empty() && !self.neighborhood_peers().is_empty() {
            return DiscoveryLayer::Neighborhood;
        }

        // Check if we have any social presence at all
        if self.has_social_presence() {
            // We're in a block but have no peers yet
            return DiscoveryLayer::Block;
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
            DiscoveryLayer::Block => self.block_peers(),
            DiscoveryLayer::Neighborhood => {
                // Include both block and neighborhood peers for neighborhood-level relay
                let mut peers = self.block_peers();
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
/// - Block: Target is in same block (can relay through co-residents)
/// - Direct: Target is personally known (in peer relationships)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscoveryLayer {
    /// No relationship with target - must use rendezvous/flooding discovery.
    Rendezvous,
    /// We have neighborhood presence and can use traversal.
    /// Target may be reachable via multi-hop neighborhood relay.
    Neighborhood,
    /// Target is reachable via block-level relay.
    /// We have co-residents who may be able to forward.
    Block,
    /// Target is personally known - we have a direct relationship.
    /// Can attempt direct connection without relay.
    Direct,
}

impl DiscoveryLayer {
    /// Get the priority of this layer (lower = closer relationship).
    pub fn priority(self) -> u8 {
        match self {
            DiscoveryLayer::Direct => 0,
            DiscoveryLayer::Block => 1,
            DiscoveryLayer::Neighborhood => 2,
            DiscoveryLayer::Rendezvous => 3,
        }
    }

    /// Check if this layer represents a known relationship.
    pub fn is_known(self) -> bool {
        matches!(self, DiscoveryLayer::Direct)
    }

    /// Check if this layer has social presence (block or neighborhood).
    pub fn has_social_presence(self) -> bool {
        matches!(
            self,
            DiscoveryLayer::Direct | DiscoveryLayer::Block | DiscoveryLayer::Neighborhood
        )
    }
}

/// Counts of peers by relationship type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerCounts {
    /// Number of block peers
    pub block_peers: usize,
    /// Number of neighborhood peers
    pub neighborhood_peers: usize,
    /// Number of guardians
    pub guardians: usize,
}

impl PeerCounts {
    /// Get total peer count.
    pub fn total(&self) -> usize {
        self.block_peers + self.neighborhood_peers + self.guardians
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_journal::facts::social::NeighborhoodId;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_empty_topology() {
        let authority = test_authority(1);
        let topology = SocialTopology::empty(authority);

        assert!(!topology.has_social_presence());
        assert!(topology.block_peers().is_empty());
        assert!(topology.neighborhood_peers().is_empty());
    }

    #[test]
    fn test_topology_with_block() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);

        let block_id = BlockId::from_bytes([1u8; 32]);
        let mut block = Block::new_empty(block_id);
        block.residents = vec![local, peer1, peer2];

        let topology = SocialTopology::new(local, Some(block), vec![]);

        assert!(topology.has_social_presence());
        let block_peers = topology.block_peers();
        assert_eq!(block_peers.len(), 2);
        assert!(block_peers.contains(&peer1));
        assert!(block_peers.contains(&peer2));
        assert!(!block_peers.contains(&local)); // Self excluded
    }

    #[test]
    fn test_relationship_priority() {
        let local = test_authority(1);
        let peer = test_authority(2);

        let block_id = BlockId::from_bytes([1u8; 32]);
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

        // Try to "upgrade" to block peer - should succeed
        topology.add_peer(
            peer,
            RelayRelationship::BlockPeer {
                block_id: *block_id.as_bytes(),
            },
        );
        assert!(topology.relationship_with(&peer).unwrap().is_block_peer());

        // Try to "downgrade" back to neighborhood peer - should be ignored
        topology.add_peer(
            peer,
            RelayRelationship::NeighborhoodPeer {
                neighborhood_id: *neighborhood_id.as_bytes(),
            },
        );
        assert!(topology.relationship_with(&peer).unwrap().is_block_peer());
    }

    #[test]
    fn test_build_relay_candidates() {
        let local = test_authority(1);
        let block_peer = test_authority(2);
        let neighborhood_peer = test_authority(3);
        let target = test_authority(4);

        let block_id = BlockId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            block_peer,
            RelayRelationship::BlockPeer {
                block_id: *block_id.as_bytes(),
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
        // Block peer should be first (higher priority)
        assert!(candidates[0].relationship.is_block_peer());
        assert!(candidates[1].relationship.is_neighborhood_peer());
    }

    #[test]
    fn test_peer_counts() {
        let local = test_authority(1);
        let block_id = BlockId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            test_authority(2),
            RelayRelationship::BlockPeer {
                block_id: *block_id.as_bytes(),
            },
        );
        topology.add_peer(
            test_authority(3),
            RelayRelationship::BlockPeer {
                block_id: *block_id.as_bytes(),
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
        assert_eq!(counts.block_peers, 2);
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
        let block_id = BlockId::from_bytes([1u8; 32]);

        let mut topology = SocialTopology::empty(local);
        topology.add_peer(
            peer,
            RelayRelationship::BlockPeer {
                block_id: *block_id.as_bytes(),
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

        let block_id = BlockId::from_bytes([1u8; 32]);
        let mut block = Block::new_empty(block_id);
        block.residents = vec![local, peer1];

        let topology = SocialTopology::new(local, Some(block), vec![]);

        // Unknown target with block presence should be Block layer
        assert_eq!(topology.discovery_layer(&unknown), DiscoveryLayer::Block);
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
        assert!(DiscoveryLayer::Direct.priority() < DiscoveryLayer::Block.priority());
        assert!(DiscoveryLayer::Block.priority() < DiscoveryLayer::Neighborhood.priority());
        assert!(DiscoveryLayer::Neighborhood.priority() < DiscoveryLayer::Rendezvous.priority());
    }

    #[test]
    fn test_discovery_layer_predicates() {
        assert!(DiscoveryLayer::Direct.is_known());
        assert!(!DiscoveryLayer::Block.is_known());
        assert!(!DiscoveryLayer::Neighborhood.is_known());
        assert!(!DiscoveryLayer::Rendezvous.is_known());

        assert!(DiscoveryLayer::Direct.has_social_presence());
        assert!(DiscoveryLayer::Block.has_social_presence());
        assert!(DiscoveryLayer::Neighborhood.has_social_presence());
        assert!(!DiscoveryLayer::Rendezvous.has_social_presence());
    }

    #[test]
    fn test_discovery_context() {
        let local = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);
        let unknown = test_authority(99);

        let block_id = BlockId::from_bytes([1u8; 32]);
        let mut block = Block::new_empty(block_id);
        block.residents = vec![local, peer1, peer2];

        let topology = SocialTopology::new(local, Some(block), vec![]);

        // Known peer should return Direct with the peer
        let (layer, peers) = topology.discovery_context(&peer1);
        assert_eq!(layer, DiscoveryLayer::Direct);
        assert_eq!(peers, vec![peer1]);

        // Unknown target should return Block with block peers
        let (layer, peers) = topology.discovery_context(&unknown);
        assert_eq!(layer, DiscoveryLayer::Block);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
    }
}

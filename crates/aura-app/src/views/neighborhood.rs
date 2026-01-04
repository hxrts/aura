//! # Neighborhood View State

use aura_core::identifiers::ChannelId;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Adjacency type between homes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum AdjacencyType {
    /// Direct neighbor (one hop)
    #[default]
    Direct,
    /// Two hops away
    TwoHop,
    /// Three or more hops
    Distant,
}

/// A neighboring home
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborHome {
    /// Home identifier
    pub id: ChannelId,
    /// Home name
    pub name: String,
    /// Type of adjacency
    pub adjacency: AdjacencyType,
    /// Number of shared contacts
    pub shared_contacts: u32,
    /// Resident count (if known)
    pub resident_count: Option<u32>,
    /// Whether we can traverse to this home
    pub can_traverse: bool,
}

/// Traversal position in the neighborhood
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TraversalPosition {
    /// Current home ID
    pub current_home_id: ChannelId,
    /// Current home name
    pub current_home_name: String,
    /// Depth from local home
    pub depth: u32,
    /// Path from home (home IDs)
    pub path: Vec<ChannelId>,
}

/// Neighborhood state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborhoodState {
    /// Home home ID
    pub home_home_id: ChannelId,
    /// Home name
    pub home_name: String,
    /// Current traversal position (if traversing)
    pub position: Option<TraversalPosition>,
    /// Visible neighbors from current position
    neighbors: Vec<NeighborHome>,
    /// Maximum traversal depth allowed
    pub max_depth: u32,
    /// Whether currently loading neighbors
    pub loading: bool,
    /// Currently connected peer IDs
    ///
    /// Managed by the network workflow - terminals should not maintain local peer state.
    #[serde(default)]
    connected_peers: HashSet<String>,
}

impl NeighborhoodState {
    // =========================================================================
    // Factory Constructors
    // =========================================================================

    /// Create a new NeighborhoodState from parts.
    ///
    /// Used by query results and tests.
    #[must_use]
    pub fn from_parts(
        home_home_id: ChannelId,
        home_name: String,
        neighbors: impl IntoIterator<Item = NeighborHome>,
    ) -> Self {
        Self {
            home_home_id,
            home_name,
            neighbors: neighbors.into_iter().collect(),
            ..Default::default()
        }
    }

    // =========================================================================
    // Neighbor Accessors
    // =========================================================================

    /// Get neighbor by ID
    #[must_use]
    pub fn neighbor(&self, id: &ChannelId) -> Option<&NeighborHome> {
        self.neighbors.iter().find(|n| n.id == *id)
    }

    /// Get all neighbors
    #[must_use]
    pub fn all_neighbors(&self) -> impl Iterator<Item = &NeighborHome> {
        self.neighbors.iter()
    }

    /// Get neighbor count
    #[must_use]
    pub fn neighbor_count(&self) -> usize {
        self.neighbors.len()
    }

    /// Check if there are no neighbors
    #[must_use]
    pub fn neighbors_is_empty(&self) -> bool {
        self.neighbors.is_empty()
    }

    /// Get direct neighbors
    #[must_use]
    pub fn direct_neighbors(&self) -> impl Iterator<Item = &NeighborHome> {
        self.neighbors
            .iter()
            .filter(|n| n.adjacency == AdjacencyType::Direct)
    }

    /// Add a neighbor
    pub fn add_neighbor(&mut self, neighbor: NeighborHome) {
        // Replace if exists, otherwise add
        if let Some(existing) = self.neighbors.iter_mut().find(|n| n.id == neighbor.id) {
            *existing = neighbor;
        } else {
            self.neighbors.push(neighbor);
        }
    }

    /// Remove a neighbor by ID
    pub fn remove_neighbor(&mut self, id: &ChannelId) -> Option<NeighborHome> {
        if let Some(pos) = self.neighbors.iter().position(|n| n.id == *id) {
            Some(self.neighbors.remove(pos))
        } else {
            None
        }
    }

    /// Clear all neighbors
    pub fn clear_neighbors(&mut self) {
        self.neighbors.clear();
    }

    /// Set neighbors, replacing all existing
    pub fn set_neighbors(&mut self, neighbors: impl IntoIterator<Item = NeighborHome>) {
        self.neighbors = neighbors.into_iter().collect();
    }

    // =========================================================================
    // Connected Peers Accessors
    // =========================================================================

    /// Get all connected peers
    #[must_use]
    pub fn connected_peers(&self) -> &HashSet<String> {
        &self.connected_peers
    }

    /// Get connected peer count
    #[must_use]
    pub fn connected_peer_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Check if a peer is connected
    #[must_use]
    pub fn has_connected_peer(&self, peer_id: &str) -> bool {
        self.connected_peers.contains(peer_id)
    }

    /// Add a connected peer. Returns true if newly added.
    pub fn add_connected_peer(&mut self, peer_id: String) -> bool {
        self.connected_peers.insert(peer_id)
    }

    /// Remove a connected peer. Returns true if was present.
    pub fn remove_connected_peer(&mut self, peer_id: &str) -> bool {
        self.connected_peers.remove(peer_id)
    }

    /// Clear all connected peers
    pub fn clear_connected_peers(&mut self) {
        self.connected_peers.clear();
    }

    // =========================================================================
    // Navigation Helpers
    // =========================================================================

    /// Check if we're at the local home
    #[must_use]
    pub fn is_at_home(&self) -> bool {
        self.position
            .as_ref()
            .map(|p| p.current_home_id == self.home_home_id)
            .unwrap_or(true)
    }

    /// Check if can go back
    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.position.as_ref().map(|p| p.depth > 0).unwrap_or(false)
    }
}

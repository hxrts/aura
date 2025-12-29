//! # Neighborhood View State

use aura_core::identifiers::ChannelId;
use serde::{Deserialize, Serialize};

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
    pub neighbors: Vec<NeighborHome>,
    /// Maximum traversal depth allowed
    pub max_depth: u32,
    /// Whether currently loading neighbors
    pub loading: bool,
}

impl NeighborhoodState {
    /// Check if we're at the local home
    pub fn is_at_home(&self) -> bool {
        self.position
            .as_ref()
            .map(|p| p.current_home_id == self.home_home_id)
            .unwrap_or(true)
    }

    /// Get neighbor by ID
    pub fn neighbor(&self, id: &ChannelId) -> Option<&NeighborHome> {
        self.neighbors.iter().find(|n| n.id == *id)
    }

    /// Get direct neighbors
    pub fn direct_neighbors(&self) -> Vec<&NeighborHome> {
        self.neighbors
            .iter()
            .filter(|n| n.adjacency == AdjacencyType::Direct)
            .collect()
    }

    /// Check if can go back
    pub fn can_go_back(&self) -> bool {
        self.position.as_ref().map(|p| p.depth > 0).unwrap_or(false)
    }
}

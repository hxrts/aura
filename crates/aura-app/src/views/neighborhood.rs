//! # Neighborhood View State

use aura_core::identifiers::ChannelId;
use serde::{Deserialize, Serialize};

/// Adjacency type between blocks
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

/// A neighboring block
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborBlock {
    /// Block identifier
    pub id: ChannelId,
    /// Block name
    pub name: String,
    /// Type of adjacency
    pub adjacency: AdjacencyType,
    /// Number of shared contacts
    pub shared_contacts: u32,
    /// Resident count (if known)
    pub resident_count: Option<u32>,
    /// Whether we can traverse to this block
    pub can_traverse: bool,
}

/// Traversal position in the neighborhood
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct TraversalPosition {
    /// Current block ID
    pub current_block_id: ChannelId,
    /// Current block name
    pub current_block_name: String,
    /// Depth from home block
    pub depth: u32,
    /// Path from home (block IDs)
    pub path: Vec<ChannelId>,
}

/// Neighborhood state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborhoodState {
    /// Home block ID
    pub home_block_id: ChannelId,
    /// Home block name
    pub home_block_name: String,
    /// Current traversal position (if traversing)
    pub position: Option<TraversalPosition>,
    /// Visible neighbors from current position
    pub neighbors: Vec<NeighborBlock>,
    /// Maximum traversal depth allowed
    pub max_depth: u32,
    /// Whether currently loading neighbors
    pub loading: bool,
}

impl NeighborhoodState {
    /// Check if we're at the home block
    pub fn is_at_home(&self) -> bool {
        self.position
            .as_ref()
            .map(|p| p.current_block_id == self.home_block_id)
            .unwrap_or(true)
    }

    /// Get neighbor by ID
    pub fn neighbor(&self, id: &ChannelId) -> Option<&NeighborBlock> {
        self.neighbors.iter().find(|n| n.id == *id)
    }

    /// Get direct neighbors
    pub fn direct_neighbors(&self) -> Vec<&NeighborBlock> {
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

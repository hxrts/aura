//! Neighborhood screen view state

use crate::tui::navigation::GridNav;
use crate::tui::types::TraversalDepth;

/// Neighborhood screen state
#[derive(Clone, Debug, Default)]
pub struct NeighborhoodViewState {
    /// Grid navigation state (handles 2D wrap-around)
    pub grid: GridNav,

    /// Desired traversal depth when entering a selected block.
    pub enter_depth: TraversalDepth,
}

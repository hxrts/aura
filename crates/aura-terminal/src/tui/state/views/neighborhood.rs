//! Neighborhood screen view state

use crate::tui::navigation::GridNav;

/// Neighborhood screen state
#[derive(Clone, Debug, Default)]
pub struct NeighborhoodViewState {
    /// Grid navigation state (handles 2D wrap-around)
    pub grid: GridNav,
}

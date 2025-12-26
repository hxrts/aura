//! Neighborhood screen view state

use crate::tui::navigation::GridNav;
use crate::tui::types::TraversalDepth;

/// Neighborhood screen mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NeighborhoodMode {
    /// Exploring the neighborhood map
    #[default]
    Map,
    /// Inside a block (detail view)
    Detail,
}

/// Focus area inside Detail mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailFocus {
    /// Channel list on the left
    #[default]
    Channels,
    /// Resident list on the left
    Residents,
    /// Message list on the right
    Messages,
    /// Input field on the right
    Input,
}

/// Neighborhood screen state
#[derive(Clone, Debug, Default)]
pub struct NeighborhoodViewState {
    /// Current mode (map vs detail)
    pub mode: NeighborhoodMode,

    /// Detail mode focus
    pub detail_focus: DetailFocus,

    /// Grid navigation state (handles 2D wrap-around)
    pub grid: GridNav,

    /// Desired traversal depth when entering a selected block.
    pub enter_depth: TraversalDepth,

    /// Selected neighborhood tab index
    pub selected_neighborhood: usize,
    /// Total neighborhoods (V1: 4)
    pub neighborhood_count: usize,

    /// Selected block index in map mode
    pub selected_block: usize,
    /// Total blocks in current neighborhood
    pub block_count: usize,

    /// Entered block id (detail mode)
    pub entered_block_id: Option<String>,

    /// Channel navigation
    pub selected_channel: usize,
    pub channel_count: usize,

    /// Resident navigation
    pub selected_resident: usize,
    pub resident_count: usize,

    /// Messaging
    pub insert_mode: bool,
    pub insert_mode_entry_char: Option<char>,
    pub input_buffer: String,
    pub message_scroll: usize,
    pub message_count: usize,

    /// Whether steward actions are enabled for current user in this block
    pub steward_actions_enabled: bool,
}

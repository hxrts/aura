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

/// State for block creation modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct BlockCreateModalState {
    /// Block name input
    pub name: String,
    /// Block description (optional)
    pub description: String,
    /// Active field (0 = name, 1 = description)
    pub active_field: usize,
    /// Error message if any
    pub error: Option<String>,
    /// Whether creation is in progress
    pub creating: bool,
}

impl BlockCreateModalState {
    /// Create new modal state
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.name.clear();
        self.description.clear();
        self.active_field = 0;
        self.error = None;
        self.creating = false;
    }

    /// Check if form can be submitted
    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty() && !self.creating
    }

    /// Move to next field
    pub fn next_field(&mut self) {
        self.active_field = (self.active_field + 1) % 2;
    }

    /// Move to previous field
    pub fn prev_field(&mut self) {
        self.active_field = if self.active_field == 0 { 1 } else { 0 };
    }

    /// Push a character to the active field
    pub fn push_char(&mut self, c: char) {
        match self.active_field {
            0 => self.name.push(c),
            1 => self.description.push(c),
            _ => {}
        }
        self.error = None;
    }

    /// Pop a character from the active field
    pub fn pop_char(&mut self) {
        match self.active_field {
            0 => {
                self.name.pop();
            }
            1 => {
                self.description.pop();
            }
            _ => {}
        }
        self.error = None;
    }

    /// Set an error
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.creating = false;
    }

    /// Mark as creating
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Get the name
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// Get the description
    pub fn get_description(&self) -> Option<&str> {
        if self.description.is_empty() {
            None
        } else {
            Some(&self.description)
        }
    }
}

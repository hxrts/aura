//! Block screen view state

/// Block screen focus
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockFocus {
    #[default]
    Residents,
    Messages,
    Input,
}

/// Block screen state
#[derive(Clone, Debug, Default)]
pub struct BlockViewState {
    /// Current focus panel
    pub focus: BlockFocus,
    /// Whether in insert mode (typing message)
    pub insert_mode: bool,
    /// Character used to enter insert mode (to prevent it being typed)
    pub insert_mode_entry_char: Option<char>,
    /// Input buffer for message composition
    pub input_buffer: String,
    /// Selected resident index (for resident list)
    pub selected_resident: usize,
    /// Total resident count (for wrap-around navigation)
    pub resident_count: usize,
    /// Message scroll position
    pub message_scroll: usize,
    /// Total message count (for wrap-around navigation)
    pub message_count: usize,
    /// Whether showing resident panel
    pub show_residents: bool,
}

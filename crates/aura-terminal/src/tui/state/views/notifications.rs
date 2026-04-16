//! Notifications screen view state

use crate::tui::navigation::TwoPanelFocus;

/// Notifications screen state
#[derive(Clone, Debug, Default)]
pub struct NotificationsViewState {
    /// Panel focus (list or detail)
    pub focus: TwoPanelFocus,
    /// Selected notification index
    pub selected_index: usize,
    /// Total notification count (for wrap-around navigation)
    pub item_count: usize,
    /// Base invitation + recovery notification count.
    pub base_item_count: usize,
    /// Stored runtime-event-backed notification count.
    pub runtime_item_count: usize,
}

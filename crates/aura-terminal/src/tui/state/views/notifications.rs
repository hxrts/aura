//! Notifications screen view state

use crate::tui::navigation::TwoPanelFocus;
use std::collections::HashSet;

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
    /// Session-scoped set of dismissed notification IDs. Notifications
    /// whose ID is in this set are filtered out during rendering.
    pub dismissed_ids: HashSet<String>,
    /// Shared write-back of the visible notification IDs in display
    /// order. The screen component writes to this on each render; the
    /// keyboard handler reads it to resolve the selected index into a
    /// concrete notification ID for dismissal.
    pub visible_ids: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

//! Recovery screen view state

use crate::tui::types::RecoveryTab;

/// Recovery screen state
#[derive(Clone, Debug, Default)]
pub struct RecoveryViewState {
    /// Current tab
    pub tab: RecoveryTab,
    /// Selected item index in current tab
    pub selected_index: usize,
    /// Item count for current tab (for wrap-around navigation)
    pub item_count: usize,
}

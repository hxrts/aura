//! Help screen view state

/// Help screen state
#[derive(Clone, Debug, Default)]
pub struct HelpViewState {
    /// Scroll position
    pub scroll: usize,
    /// Maximum scroll position (total content lines for wrap-around)
    pub scroll_max: usize,
    /// Filter text
    pub filter: String,
}

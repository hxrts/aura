//! # TUI View Types
//!
//! Data types for TUI view snapshots. These types are used by the snapshot
//! structs in `hooks.rs` for rendering.
//!
//! Note: The large reactive View classes (ChatView, GuardiansView, etc.) have
//! been removed. Screens now subscribe directly to AppCore signals for reactive
//! updates. See `aura-app` for the authoritative ViewState types.

// =============================================================================
// Generic View State Wrapper
// =============================================================================

/// Generic view state wrapper with loading/error tracking
#[derive(Debug, Clone)]
pub struct ViewState<T> {
    /// Current data
    data: T,
    /// Whether the view is loading
    loading: bool,
    /// Last error (if any)
    error: Option<String>,
    /// Last update timestamp
    last_updated: u64,
}

impl<T: Default> Default for ViewState<T> {
    fn default() -> Self {
        Self {
            data: T::default(),
            loading: false,
            error: None,
            last_updated: 0,
        }
    }
}

impl<T: Clone> ViewState<T> {
    /// Create a new view state with initial data
    pub fn new(data: T) -> Self {
        Self {
            data,
            loading: false,
            error: None,
            last_updated: now_millis(),
        }
    }

    /// Get the current data
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Check if loading
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Get the last error
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Get the last update timestamp
    pub fn last_updated(&self) -> u64 {
        self.last_updated
    }

    /// Update the data
    pub fn set_data(&mut self, data: T) {
        self.data = data;
        self.loading = false;
        self.error = None;
        self.last_updated = now_millis();
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Set error state
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.loading = false;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.error = None;
    }
}

/// Get current time in milliseconds
fn now_millis() -> u64 {
    use aura_effects::time::PhysicalTimeHandler;

    PhysicalTimeHandler::new().physical_time_now_ms()
}

// =============================================================================
// Guardians Types
// =============================================================================

/// Threshold configuration for guardians
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    /// Required number of guardians for recovery
    pub threshold: u32,
    /// Total number of guardians
    pub total: u32,
}

// =============================================================================
// Block Types
// =============================================================================

/// Block information for display
#[derive(Debug, Clone, Default)]
pub struct BlockInfo {
    /// Block identifier
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// Description or topic
    pub description: Option<String>,
    /// When the block was created
    pub created_at: u64,
}

/// A resident of a block
#[derive(Debug, Clone, Default)]
pub struct Resident {
    /// Authority ID
    pub authority_id: String,
    /// Display name
    pub name: String,
    /// Whether this is the current user
    pub is_self: bool,
    /// Whether this resident is online
    pub is_online: bool,
    /// Role in the block (resident, steward)
    pub role: ResidentRole,
}

/// Resident role in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResidentRole {
    /// Regular resident with no elevated privileges
    #[default]
    Resident,
    /// Steward with elevated maintenance privileges
    Steward,
}

/// Storage usage info
#[derive(Debug, Clone, Default)]
pub struct StorageInfo {
    /// Used storage in bytes
    pub used_bytes: u64,
    /// Total available storage in bytes
    pub total_bytes: u64,
}

// =============================================================================
// Contacts Types
// =============================================================================

/// A contact in the contacts list
#[derive(Debug, Clone, Default)]
pub struct Contact {
    /// Authority ID
    pub authority_id: String,
    /// Petname (local display name)
    pub petname: String,
    /// Their suggested display name
    pub suggested_name: Option<String>,
    /// Whether contact is online
    pub is_online: Option<bool>,
    /// When added
    pub added_at: u64,
    /// Last interaction time
    pub last_seen: Option<u64>,
    /// Whether there's a pending suggestion
    pub has_pending_suggestion: bool,
}

/// Suggestion policy for contacts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SuggestionPolicy {
    /// Automatically accept contact suggestions
    #[default]
    AutoAccept,
    /// Prompt the user before accepting suggestions
    PromptFirst,
    /// Ignore incoming suggestions
    Ignore,
}

/// Contact suggestion (what the user shares about themselves)
#[derive(Debug, Clone, Default)]
pub struct MySuggestion {
    /// Display name
    pub display_name: Option<String>,
    /// Status message
    pub status: Option<String>,
}

// =============================================================================
// Neighborhood Types
// =============================================================================

/// Block summary for neighborhood display
#[derive(Debug, Clone, Default)]
pub struct NeighborhoodBlock {
    /// Block ID
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// Resident count
    pub resident_count: u8,
    /// Max residents (usually 8)
    pub max_residents: u8,
    /// Whether this is the user's home block
    pub is_home: bool,
    /// Whether user can enter this block
    pub can_enter: bool,
    /// Whether user is currently at this block
    pub is_current: bool,
}

/// Adjacency between blocks
#[derive(Debug, Clone, Default)]
pub struct BlockAdjacency {
    /// First block ID
    pub block_a: String,
    /// Second block ID
    pub block_b: String,
}

/// Traversal depth in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalDepth {
    /// Street-level access
    #[default]
    Street,
    /// Frontage (immediate neighboring blocks)
    Frontage,
    /// Interior (deep traversal)
    Interior,
}

/// Current position in neighborhood traversal
#[derive(Debug, Clone, Default)]
pub struct TraversalPosition {
    /// Current neighborhood ID
    pub neighborhood_id: Option<String>,
    /// Current block ID (None = on street)
    pub block_id: Option<String>,
    /// Depth of access
    pub depth: TraversalDepth,
    /// When this position was entered
    pub entered_at: u64,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_state_default() {
        let state: ViewState<Vec<String>> = ViewState::default();
        assert!(!state.is_loading());
        assert!(state.error().is_none());
        assert!(state.data().is_empty());
    }

    #[test]
    fn test_view_state_set_data() {
        let mut state = ViewState::default();
        state.set_data(vec!["hello".to_string()]);
        assert_eq!(state.data().len(), 1);
        assert!(!state.is_loading());
    }

    #[test]
    fn test_view_state_loading() {
        let mut state: ViewState<Vec<String>> = ViewState::default();
        state.set_loading(true);
        assert!(state.is_loading());
        state.set_loading(false);
        assert!(!state.is_loading());
    }

    #[test]
    fn test_view_state_error() {
        let mut state: ViewState<Vec<String>> = ViewState::default();
        state.set_error("Something went wrong");
        assert_eq!(state.error(), Some("Something went wrong"));
        state.clear_error();
        assert!(state.error().is_none());
    }
}

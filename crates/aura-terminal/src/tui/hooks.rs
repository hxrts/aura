//! # Custom Hooks for iocraft
//!
//! Bridges futures-signals reactive state with iocraft's component system.
//!
//! ## Overview
//!
//! These hooks allow iocraft components to subscribe to futures-signals
//! and automatically re-render when data changes.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::tui::hooks::snapshot_state;
//!
//! #[component]
//! fn MyComponent(props: &Props) -> impl Into<AnyElement<'static>> {
//!     let value = snapshot_state(&props.reactive_state);
//!
//!     element! {
//!         Text(content: format!("Value: {}", value))
//!     }
//! }
//! ```

use crate::tui::reactive::signals::{ReactiveState, ReactiveVec};

/// Trait for types that can be used with reactive hooks
pub trait ReactiveValue: Clone + Send + Sync + 'static {}
impl<T: Clone + Send + Sync + 'static> ReactiveValue for T {}

/// Snapshot of a ReactiveState for use in iocraft components
///
/// Since iocraft doesn't have a built-in way to subscribe to external signals,
/// we use a snapshot approach: read the current value when rendering.
///
/// For real-time updates, the parent component should poll or use
/// iocraft's use_future hook to periodically check for changes.
pub fn snapshot_state<T: Clone>(state: &ReactiveState<T>) -> T {
    state.get()
}

/// Snapshot of a ReactiveVec for use in iocraft components
///
/// Returns a cloned vector of all current items.
pub fn snapshot_vec<T: Clone>(vec: &ReactiveVec<T>) -> Vec<T> {
    vec.get_cloned()
}

/// Helper to check if a ReactiveVec is empty
pub fn is_vec_empty<T: Clone>(vec: &ReactiveVec<T>) -> bool {
    vec.is_empty()
}

/// Helper to get the length of a ReactiveVec
pub fn vec_len<T: Clone>(vec: &ReactiveVec<T>) -> usize {
    vec.len()
}

// =============================================================================
// Props Helpers
// =============================================================================

/// Trait for props that contain reactive data
///
/// Implement this trait to enable automatic snapshot extraction in components.
pub trait HasReactiveData {
    /// Type of the snapshot data
    type Snapshot;

    /// Create a snapshot of all reactive data for rendering
    fn snapshot(&self) -> Self::Snapshot;
}

// =============================================================================
// View Snapshot Extraction
// =============================================================================

use crate::tui::reactive::views::{
    BlockView, ChatView, ContactsView, GuardiansView, InvitationsView, NeighborhoodView,
    ReactiveViewModel, RecoveryView,
};

/// Snapshot of chat-related data for rendering
#[derive(Debug, Clone)]
pub struct ChatSnapshot {
    /// Current channels list
    pub channels: Vec<crate::tui::reactive::queries::Channel>,
    /// Currently selected channel ID
    pub selected_channel: Option<String>,
    /// Messages for the selected channel
    pub messages: Vec<crate::tui::reactive::queries::Message>,
}

impl Default for ChatSnapshot {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            selected_channel: None,
            messages: Vec::new(),
        }
    }
}

/// Snapshot of guardian-related data for rendering
#[derive(Debug, Clone)]
pub struct GuardiansSnapshot {
    /// Guardian list
    pub guardians: Vec<crate::tui::reactive::queries::Guardian>,
    /// Threshold configuration
    pub threshold: Option<crate::tui::reactive::views::ThresholdConfig>,
}

impl Default for GuardiansSnapshot {
    fn default() -> Self {
        Self {
            guardians: Vec::new(),
            threshold: None,
        }
    }
}

/// Snapshot of recovery-related data for rendering
#[derive(Debug, Clone)]
pub struct RecoverySnapshot {
    /// Recovery status
    pub status: crate::tui::reactive::queries::RecoveryStatus,
    /// Progress percentage (0-100)
    pub progress_percent: u32,
    /// Whether recovery is in progress
    pub is_in_progress: bool,
}

impl Default for RecoverySnapshot {
    fn default() -> Self {
        Self {
            status: crate::tui::reactive::queries::RecoveryStatus::default(),
            progress_percent: 0,
            is_in_progress: false,
        }
    }
}

/// Snapshot of invitation-related data for rendering
#[derive(Debug, Clone)]
pub struct InvitationsSnapshot {
    /// All invitations
    pub invitations: Vec<crate::tui::reactive::queries::Invitation>,
    /// Count of pending invitations
    pub pending_count: usize,
}

impl Default for InvitationsSnapshot {
    fn default() -> Self {
        Self {
            invitations: Vec::new(),
            pending_count: 0,
        }
    }
}

/// Snapshot of block-related data for rendering
#[derive(Debug, Clone)]
pub struct BlockSnapshot {
    /// Block information
    pub block: Option<crate::tui::reactive::views::BlockInfo>,
    /// Residents list
    pub residents: Vec<crate::tui::reactive::views::Resident>,
    /// Storage information
    pub storage: crate::tui::reactive::views::StorageInfo,
    /// Whether user is a resident
    pub is_resident: bool,
    /// Whether user is a steward
    pub is_steward: bool,
}

impl Default for BlockSnapshot {
    fn default() -> Self {
        Self {
            block: None,
            residents: Vec::new(),
            storage: crate::tui::reactive::views::StorageInfo::default(),
            is_resident: false,
            is_steward: false,
        }
    }
}

/// Snapshot of contacts-related data for rendering
#[derive(Debug, Clone)]
pub struct ContactsSnapshot {
    /// Contacts list
    pub contacts: Vec<crate::tui::reactive::views::Contact>,
    /// Suggestion policy
    pub policy: crate::tui::reactive::views::SuggestionPolicy,
}

impl Default for ContactsSnapshot {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            policy: crate::tui::reactive::views::SuggestionPolicy::default(),
        }
    }
}

/// Snapshot of neighborhood-related data for rendering
#[derive(Debug, Clone)]
pub struct NeighborhoodSnapshot {
    /// Neighborhood ID
    pub neighborhood_id: Option<String>,
    /// Neighborhood name
    pub neighborhood_name: Option<String>,
    /// Blocks in neighborhood
    pub blocks: Vec<crate::tui::reactive::views::NeighborhoodBlock>,
    /// Current traversal position
    pub position: crate::tui::reactive::views::TraversalPosition,
}

impl Default for NeighborhoodSnapshot {
    fn default() -> Self {
        Self {
            neighborhood_id: None,
            neighborhood_name: None,
            blocks: Vec::new(),
            position: crate::tui::reactive::views::TraversalPosition::default(),
        }
    }
}

/// Extract a snapshot from ChatView
pub fn snapshot_chat(view: &ChatView) -> ChatSnapshot {
    let selected = view.get_selected_channel();
    let messages = selected
        .as_ref()
        .and_then(|id| view.get_channel_state(id))
        .map(|state| state.messages)
        .unwrap_or_default();

    ChatSnapshot {
        channels: view.get_channels(),
        selected_channel: selected,
        messages,
    }
}

/// Extract a snapshot from GuardiansView
pub fn snapshot_guardians(view: &GuardiansView) -> GuardiansSnapshot {
    GuardiansSnapshot {
        guardians: view.get_guardians(),
        threshold: view.get_threshold(),
    }
}

/// Extract a snapshot from RecoveryView
pub fn snapshot_recovery(view: &RecoveryView) -> RecoverySnapshot {
    RecoverySnapshot {
        status: view.get_status(),
        progress_percent: view.progress_percent(),
        is_in_progress: view.is_in_progress(),
    }
}

/// Extract a snapshot from InvitationsView
pub fn snapshot_invitations(view: &InvitationsView) -> InvitationsSnapshot {
    InvitationsSnapshot {
        invitations: view.get_invitations(),
        pending_count: view.pending_count(),
    }
}

/// Extract a snapshot from BlockView
pub fn snapshot_block(view: &BlockView) -> BlockSnapshot {
    BlockSnapshot {
        block: view.get_block(),
        residents: view.get_residents(),
        storage: view.get_storage(),
        is_resident: view.get_is_resident(),
        is_steward: view.get_is_steward(),
    }
}

/// Extract a snapshot from ContactsView
pub fn snapshot_contacts(view: &ContactsView) -> ContactsSnapshot {
    ContactsSnapshot {
        contacts: view.contacts(),
        policy: view.policy(),
    }
}

/// Extract a snapshot from NeighborhoodView
pub fn snapshot_neighborhood(view: &NeighborhoodView) -> NeighborhoodSnapshot {
    NeighborhoodSnapshot {
        neighborhood_id: view.neighborhood_id(),
        neighborhood_name: view.neighborhood_name(),
        blocks: view.blocks(),
        position: view.position(),
    }
}

/// Complete snapshot of the entire view model
#[derive(Debug, Clone)]
pub struct ViewModelSnapshot {
    /// Chat data
    pub chat: ChatSnapshot,
    /// Guardians data
    pub guardians: GuardiansSnapshot,
    /// Recovery data
    pub recovery: RecoverySnapshot,
    /// Invitations data
    pub invitations: InvitationsSnapshot,
    /// Block data
    pub block: BlockSnapshot,
    /// Contacts data
    pub contacts: ContactsSnapshot,
    /// Neighborhood data
    pub neighborhood: NeighborhoodSnapshot,
    /// Total pending notifications
    pub pending_notifications: usize,
    /// Whether any critical action is required
    pub has_critical_notifications: bool,
}

impl Default for ViewModelSnapshot {
    fn default() -> Self {
        Self {
            chat: ChatSnapshot::default(),
            guardians: GuardiansSnapshot::default(),
            recovery: RecoverySnapshot::default(),
            invitations: InvitationsSnapshot::default(),
            block: BlockSnapshot::default(),
            contacts: ContactsSnapshot::default(),
            neighborhood: NeighborhoodSnapshot::default(),
            pending_notifications: 0,
            has_critical_notifications: false,
        }
    }
}

/// Extract a complete snapshot from ReactiveViewModel
pub fn snapshot_view_model(vm: &ReactiveViewModel) -> ViewModelSnapshot {
    ViewModelSnapshot {
        chat: snapshot_chat(&vm.chat),
        guardians: snapshot_guardians(&vm.guardians),
        recovery: snapshot_recovery(&vm.recovery),
        invitations: snapshot_invitations(&vm.invitations),
        block: snapshot_block(&vm.block),
        contacts: snapshot_contacts(&vm.contacts),
        neighborhood: snapshot_neighborhood(&vm.neighborhood),
        pending_notifications: vm.pending_notifications_count(),
        has_critical_notifications: vm.has_critical_notifications(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_state() {
        let state = ReactiveState::new(42);
        assert_eq!(snapshot_state(&state), 42);

        state.set(100);
        assert_eq!(snapshot_state(&state), 100);
    }

    #[test]
    fn test_snapshot_vec() {
        let vec = ReactiveVec::new();
        vec.push(1);
        vec.push(2);
        vec.push(3);

        let snapshot = snapshot_vec(&vec);
        assert_eq!(snapshot, vec![1, 2, 3]);
    }

    #[test]
    fn test_vec_helpers() {
        let vec: ReactiveVec<i32> = ReactiveVec::new();
        assert!(is_vec_empty(&vec));
        assert_eq!(vec_len(&vec), 0);

        vec.push(1);
        assert!(!is_vec_empty(&vec));
        assert_eq!(vec_len(&vec), 1);
    }

    #[test]
    fn test_chat_snapshot() {
        let view = ChatView::new();
        let snapshot = snapshot_chat(&view);

        assert!(snapshot.channels.is_empty());
        assert!(snapshot.selected_channel.is_none());
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn test_view_model_snapshot() {
        let vm = ReactiveViewModel::new();
        let snapshot = snapshot_view_model(&vm);

        assert!(snapshot.chat.channels.is_empty());
        assert!(snapshot.guardians.guardians.is_empty());
        assert!(!snapshot.recovery.is_in_progress);
        assert!(snapshot.invitations.invitations.is_empty());
    }
}

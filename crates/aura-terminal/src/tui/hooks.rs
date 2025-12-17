//! # Custom Hooks for iocraft
//!
//! Bridges reactive state with iocraft's component system using the unified
//! `ReactiveEffects` system from aura-core.
//!
//! ## Overview
//!
//! These hooks allow iocraft components to subscribe to application signals
//! and automatically re-render when data changes.
//!
//! ## Push-Based Signal Subscription
//!
//! iocraft's `use_future` hook enables true push-based reactive updates by
//! spawning async tasks that subscribe to `ReactiveEffects` signals. When a
//! signal emits a new value, the task updates iocraft's `State<T>`, which
//! triggers a re-render.
//!
//! ```ignore
//! use iocraft::prelude::*;
//! use aura_app::signal_defs::CHAT_SIGNAL;
//! use aura_core::effects::reactive::ReactiveEffects;
//!
//! #[component]
//! fn ChatScreen(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
//!     // Get AppCore from context
//!     let ctx = hooks.use_context::<AppCoreContext>();
//!
//!     // Initialize state from current value
//!     let chat_state = hooks.use_state(|| Default::default());
//!
//!     // Subscribe to signal updates via use_future
//!     hooks.use_future({
//!         let mut chat_state = chat_state.clone();
//!         let app_core = ctx.app_core.clone();
//!         async move {
//!             // Get subscription via ReactiveEffects
//!             let mut stream = {
//!                 let core = app_core.read().await;
//!                 core.subscribe(&*CHAT_SIGNAL)
//!             };
//!
//!             // Process updates until component unmounts
//!             while let Ok(new_value) = stream.recv().await {
//!                 chat_state.set(new_value);
//!             }
//!         }
//!     });
//!
//!     element! {
//!         Text(content: format!("Messages: {}", chat_state.read().messages.len()))
//!     }
//! }
//! ```
//!
//! ## Snapshot Utilities
//!
//! For components that don't need live updates, snapshot functions provide
//! point-in-time reads of reactive state.

use std::sync::Arc;

use aura_app::{AppCore, ReactiveState, ReactiveVec};
use tokio::sync::RwLock;

use crate::tui::context::IoContext;

// =============================================================================
// AppCore Context for iocraft
// =============================================================================

/// Context type for sharing AppCore with iocraft components.
///
/// This enables components to access AppCore via `hooks.use_context::<AppCoreContext>()`.
/// Components can then use `use_future` to subscribe to signals for reactive updates
/// via the unified `ReactiveEffects` system.
///
/// ## Example
///
/// ```ignore
/// use crate::tui::hooks::AppCoreContext;
/// use aura_app::signal_defs::CHAT_SIGNAL;
/// use aura_core::effects::reactive::ReactiveEffects;
///
/// #[component]
/// fn MyComponent(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
///     let ctx = hooks.use_context::<AppCoreContext>();
///
///     // Initialize state from current value
///     let messages = hooks.use_state(|| Vec::new());
///
///     // Subscribe to signal updates via ReactiveEffects
///     hooks.use_future({
///         let mut messages = messages.clone();
///         let app_core = ctx.app_core.clone();
///         async move {
///             let mut stream = {
///                 let core = app_core.read().await;
///                 core.subscribe(&*CHAT_SIGNAL)
///             };
///             while let Ok(state) = stream.recv().await {
///                 messages.set(state.messages.clone());
///             }
///         }
///     });
///
///     element! { ... }
/// }
/// ```
#[derive(Clone)]
pub struct AppCoreContext {
    /// The shared AppCore instance
    pub app_core: Arc<RwLock<AppCore>>,

    /// The IoContext for effect dispatch
    pub io_context: Arc<IoContext>,
}

impl AppCoreContext {
    /// Create a new AppCoreContext
    pub fn new(app_core: Arc<RwLock<AppCore>>, io_context: Arc<IoContext>) -> Self {
        Self {
            app_core,
            io_context,
        }
    }

    /// Get a snapshot of the current state
    ///
    /// This is useful for initializing iocraft State<T> values.
    pub fn snapshot(&self) -> aura_app::StateSnapshot {
        // Use try_read to avoid blocking in sync context
        // Fall back to default if lock is held
        self.app_core
            .try_read()
            .map(|guard| guard.snapshot())
            .unwrap_or_default()
    }

    /// Dispatch an effect command through IoContext
    pub async fn dispatch(&self, cmd: crate::tui::effects::EffectCommand) -> Result<(), String> {
        self.io_context.dispatch(cmd).await
    }
}

/// Trait for types that can be used with reactive hooks
pub trait ReactiveValue: Clone + Send + Sync + 'static {}
impl<T: Clone + Send + Sync + 'static> ReactiveValue for T {}

/// Snapshot of a ReactiveState for use in iocraft components
///
/// Returns the current value. For real-time push-based updates, use `use_future`
/// with signal subscription (see module documentation).
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
// View Snapshot Types
// =============================================================================
//
// These snapshot structs are populated from AppCore's ViewState. The old
// View classes (ChatView, GuardiansView, etc.) have been removed - screens
// now subscribe directly to AppCore signals for reactive updates.

/// Snapshot of chat-related data for rendering
#[derive(Debug, Clone)]
pub struct ChatSnapshot {
    /// Current channels list
    pub channels: Vec<aura_app::views::chat::Channel>,
    /// Currently selected channel ID
    pub selected_channel: Option<String>,
    /// Messages for the selected channel
    pub messages: Vec<aura_app::views::chat::Message>,
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
    pub guardians: Vec<aura_app::views::recovery::Guardian>,
    /// Threshold configuration
    pub threshold: Option<aura_core::threshold::ThresholdConfig>,
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
    /// Recovery state
    pub status: aura_app::views::recovery::RecoveryState,
    /// Progress percentage (0-100)
    pub progress_percent: u32,
    /// Whether recovery is in progress
    pub is_in_progress: bool,
}

impl Default for RecoverySnapshot {
    fn default() -> Self {
        Self {
            status: aura_app::views::recovery::RecoveryState::default(),
            progress_percent: 0,
            is_in_progress: false,
        }
    }
}

/// Snapshot of invitation-related data for rendering
#[derive(Debug, Clone)]
pub struct InvitationsSnapshot {
    /// All invitations
    pub invitations: Vec<aura_app::views::invitations::Invitation>,
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
    /// Block state (contains id, name, residents, storage, etc.)
    pub block: Option<aura_app::views::block::BlockState>,
    /// Whether user is a resident
    pub is_resident: bool,
    /// Whether user is a steward
    pub is_steward: bool,
}

impl Default for BlockSnapshot {
    fn default() -> Self {
        Self {
            block: None,
            is_resident: false,
            is_steward: false,
        }
    }
}

impl BlockSnapshot {
    /// Get residents list from block state
    pub fn residents(&self) -> &[aura_app::views::block::Resident] {
        self.block.as_ref().map(|b| b.residents.as_slice()).unwrap_or(&[])
    }

    /// Get storage info from block state
    pub fn storage(&self) -> aura_app::BlockFlowBudget {
        self.block.as_ref().map(|b| b.storage.clone()).unwrap_or_default()
    }
}

/// Snapshot of contacts-related data for rendering
#[derive(Debug, Clone)]
pub struct ContactsSnapshot {
    /// Contacts list
    pub contacts: Vec<aura_app::views::contacts::Contact>,
    /// Suggestion policy
    pub policy: aura_app::views::contacts::SuggestionPolicy,
}

impl Default for ContactsSnapshot {
    fn default() -> Self {
        Self {
            contacts: Vec::new(),
            policy: aura_app::views::contacts::SuggestionPolicy::default(),
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
    pub blocks: Vec<aura_app::views::neighborhood::NeighborBlock>,
    /// Current traversal position
    pub position: aura_app::views::neighborhood::TraversalPosition,
}

impl Default for NeighborhoodSnapshot {
    fn default() -> Self {
        Self {
            neighborhood_id: None,
            neighborhood_name: None,
            blocks: Vec::new(),
            position: aura_app::views::neighborhood::TraversalPosition::default(),
        }
    }
}

/// Snapshot of device-related data for rendering
#[derive(Debug, Clone)]
pub struct DevicesSnapshot {
    /// List of registered devices
    pub devices: Vec<crate::tui::types::Device>,
    /// ID of the current device (for highlighting)
    pub current_device_id: Option<String>,
}

impl Default for DevicesSnapshot {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            current_device_id: None,
        }
    }
}

// Note: The old View-based snapshot functions have been removed.
// Snapshots are now created directly from AppCore's ViewState in IoContext.
// See context.rs for the snapshot_* implementations.

// =============================================================================
// Callback Context for iocraft
// =============================================================================

use crate::tui::callbacks::CallbackRegistry;

/// Context type for sharing callbacks with iocraft components.
///
/// This enables components to access domain-specific callbacks via
/// `hooks.use_context::<CallbackContext>()` instead of passing them
/// through props at every level.
///
/// ## Example
///
/// ```ignore
/// use crate::tui::hooks::CallbackContext;
///
/// #[component]
/// fn ChatScreen(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
///     let callbacks = hooks.use_context::<CallbackContext>();
///
///     // Access chat-specific callbacks
///     let on_send = callbacks.registry.chat.on_send.clone();
///
///     element! { ... }
/// }
/// ```
#[derive(Clone)]
pub struct CallbackContext {
    /// The callback registry containing all domain callbacks
    pub registry: CallbackRegistry,
}

impl CallbackContext {
    /// Create a new CallbackContext with the given registry
    pub fn new(registry: CallbackRegistry) -> Self {
        Self { registry }
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
    fn test_chat_snapshot_default() {
        let snapshot = ChatSnapshot::default();

        assert!(snapshot.channels.is_empty());
        assert!(snapshot.selected_channel.is_none());
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn test_snapshot_defaults() {
        // All snapshot types should have sensible defaults
        let chat = ChatSnapshot::default();
        assert!(chat.channels.is_empty());

        let guardians = GuardiansSnapshot::default();
        assert!(guardians.guardians.is_empty());

        let recovery = RecoverySnapshot::default();
        assert!(!recovery.is_in_progress);

        let invitations = InvitationsSnapshot::default();
        assert!(invitations.invitations.is_empty());

        let block = BlockSnapshot::default();
        assert!(block.block.is_none());

        let contacts = ContactsSnapshot::default();
        assert!(contacts.contacts.is_empty());

        let neighborhood = NeighborhoodSnapshot::default();
        assert!(neighborhood.blocks.is_empty());
    }
}

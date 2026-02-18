// Allow patterns that are clearer than their clippy-suggested alternatives
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::field_reassign_with_default)]

//! # TUI State Machine
//!
//! Pure state machine model for the TUI, enabling deterministic testing.
//!
//! The TUI is modeled as:
//! ```text
//! TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
//! ```
//!
//! This module provides:
//! - `TuiState`: Complete UI state
//! - `TuiCommand`: Side effects to be executed
//! - `transition()`: Pure state transition function
//!
//! ## Modal and Toast Queue System
//!
//! All modals and toasts go through type-enforced queues:
//! - `ModalQueue`: Ensures exactly 0 or 1 modal is visible at a time (FIFO)
//! - `ToastQueue`: Ensures exactly 0 or 1 toast is visible at a time (FIFO)
//!
//! **IMPORTANT**: Do NOT use `visible: bool` fields on modal structs.
//! All modals MUST be shown via `modal_queue.enqueue()`.

pub mod commands;
pub mod form;
mod handlers;
pub mod modal_queue;
pub mod toast;
mod transition;
pub mod views;

// Re-export all public types for backwards compatibility
pub use commands::{DispatchCommand, TuiCommand};
pub use form::{FormDraft, FormPhase, Validatable, ValidationError};
pub use modal_queue::{
    ChatMemberSelectModalState, ConfirmAction, ContactSelectModalState, ModalQueue, ModalType,
    QueuedModal,
};
pub use toast::{QueuedToast, Toast, ToastLevel, ToastQueue};
pub use transition::transition;
pub use views::*;
pub use views::{ChatMemberCandidate, CreateChannelModalState, CreateChannelStep};

use crate::tui::screens::{Router, Screen};
use crate::tui::types::AuthorityInfo;

/// Complete TUI state
///
/// This struct captures all state needed to render the TUI and process events.
/// It is designed to be:
/// - Clone: Can be copied for state comparison
/// - Debug: Can be inspected for debugging
/// - Serializable: Can be saved for trace replay (via serde)
#[derive(Clone, Debug, Default)]
pub struct TuiState {
    /// Screen navigation state
    pub router: Router,

    // ========================================================================
    // NEW: Queue-Based Modal/Toast System
    // ========================================================================
    /// Modal queue - type-enforced single modal at a time (FIFO)
    /// **USE THIS** instead of scattered `visible: bool` fields
    pub modal_queue: ModalQueue,

    /// Toast queue - type-enforced single toast at a time (FIFO)
    /// **USE THIS** instead of `Vec<Toast>`
    pub toast_queue: ToastQueue,

    /// Counter for generating unique toast IDs
    pub next_toast_id: u64,

    // ========================================================================
    // Screen-Specific State
    // ========================================================================
    /// Chat screen state
    pub chat: ChatViewState,

    /// Contacts screen state
    pub contacts: ContactsViewState,

    /// Notifications screen state
    pub notifications: NotificationsViewState,

    /// Settings screen state
    pub settings: SettingsViewState,

    /// Neighborhood screen state
    pub neighborhood: NeighborhoodViewState,

    /// Help screen state
    pub help: HelpViewState,

    // ========================================================================
    // Global State
    // ========================================================================
    /// Terminal size
    pub terminal_size: (u16, u16),

    /// Whether the TUI should exit
    pub should_exit: bool,

    /// Whether the terminal window has focus
    /// Used to pause animations and show visual indicator when unfocused
    pub window_focused: bool,

    // ========================================================================
    // Authority Context (app-global, affects all screens)
    // ========================================================================
    /// Available authorities for this device (populated by signal)
    pub authorities: Vec<AuthorityInfo>,

    /// Index of the currently active authority in the authorities list
    /// This is app-global context, not screen-specific state.
    pub current_authority_index: usize,
}

impl TuiState {
    /// Create a new TUI state with default values
    #[must_use]
    pub fn new() -> Self {
        Self {
            terminal_size: (80, 24),
            ..Default::default()
        }
    }

    /// Create a TUI state with specific terminal size
    #[must_use]
    pub fn with_size(width: u16, height: u16) -> Self {
        Self {
            terminal_size: (width, height),
            ..Default::default()
        }
    }

    /// Create a TUI state with the account setup modal visible (via queue)
    #[must_use]
    pub fn with_account_setup() -> Self {
        let mut state = Self::new();
        state.show_account_setup_queued();
        state
    }

    /// Get the current screen
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.router.current()
    }

    // ========================================================================
    // NEW: Queue-Based Modal/Toast Methods
    // ========================================================================

    /// Show a modal via the queue (type-enforced)
    pub fn show_modal(&mut self, modal: QueuedModal) {
        self.modal_queue.enqueue(modal);
    }

    /// Dismiss the current modal (shows next in queue if any)
    pub fn dismiss_modal(&mut self) -> Option<QueuedModal> {
        self.modal_queue.dismiss()
    }

    /// Check if any modal is active (queue-based)
    #[must_use]
    pub fn has_queued_modal(&self) -> bool {
        self.modal_queue.is_active()
    }

    /// Show a toast via the queue (type-enforced)
    pub fn show_toast_queued(&mut self, message: impl Into<String>, level: ToastLevel) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        self.toast_queue
            .enqueue(QueuedToast::new(id, message, level));
    }

    /// Show a success toast
    pub fn toast_success(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Success);
    }

    /// Show an error toast
    pub fn toast_error(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Error);
    }

    /// Show an info toast
    pub fn toast_info(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Info);
    }

    /// Show a warning toast
    pub fn toast_warning(&mut self, message: impl Into<String>) {
        self.show_toast_queued(message, ToastLevel::Warning);
    }

    /// Dismiss the current toast (shows next in queue if any)
    pub fn dismiss_toast(&mut self) -> Option<QueuedToast> {
        self.toast_queue.dismiss()
    }

    /// Check if any toast is active (queue-based)
    #[must_use]
    pub fn has_queued_toast(&self) -> bool {
        self.toast_queue.is_active()
    }

    /// Process a tick for the toast queue (auto-dismiss expired toasts)
    pub fn tick_toasts(&mut self) -> bool {
        self.toast_queue.tick()
    }

    /// Show the account setup modal via queue
    pub fn show_account_setup_queued(&mut self) {
        self.modal_queue
            .enqueue(QueuedModal::AccountSetup(AccountSetupModalState::default()));
    }

    /// Signal that account creation succeeded (queue-based)
    /// Dismisses modal and shows a success toast instead of a success screen
    pub fn account_created_queued(&mut self) {
        // Get the nickname suggestion before dismissing
        let nickname_suggestion =
            if let Some(QueuedModal::AccountSetup(ref state)) = self.modal_queue.current() {
                state.nickname_suggestion.clone()
            } else {
                String::new()
            };

        // Dismiss the modal
        self.modal_queue.dismiss();

        // Show a success toast
        let message = if nickname_suggestion.is_empty() {
            "Account created successfully".to_string()
        } else {
            format!("Welcome, {nickname_suggestion}!")
        };
        self.next_toast_id += 1;
        self.toast_queue
            .enqueue(QueuedToast::success(self.next_toast_id, message));
    }

    /// Signal that account creation failed (queue-based)
    pub fn account_creation_failed_queued(&mut self, error: String) {
        if let Some(QueuedModal::AccountSetup(ref mut state)) = self.modal_queue.current_mut() {
            state.set_error(error);
        }
    }

    /// Check if the current modal is a text input modal (queue-based)
    #[must_use]
    pub fn is_queued_modal_text_input(&self) -> bool {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(_)) => true,
            Some(QueuedModal::ChatCreate(_)) => true,
            Some(QueuedModal::ChatTopic(_)) => true,
            Some(QueuedModal::ContactsNickname(_)) => true,
            Some(QueuedModal::ContactsImport(_)) => true,
            Some(QueuedModal::SettingsNicknameSuggestion(_)) => true,
            Some(QueuedModal::SettingsAddDevice(_)) => true,
            _ => false,
        }
    }

    /// Check if in insert mode (for text input)
    #[must_use]
    pub fn is_insert_mode(&self) -> bool {
        match self.screen() {
            Screen::Chat => self.chat.insert_mode,
            _ => false,
        }
    }

    // ========================================================================
    // Modal Helper Methods
    // ========================================================================

    /// Check if any modal is active
    #[must_use]
    pub fn has_modal(&self) -> bool {
        self.has_queued_modal()
    }

    /// Get the current modal type (for backwards compatibility in tests)
    #[must_use]
    pub fn current_modal_type(&self) -> ModalType {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(_)) => ModalType::AccountSetup,
            Some(QueuedModal::Help { .. }) => ModalType::Help,
            Some(QueuedModal::GuardianSelect(_)) => ModalType::GuardianSelect,
            Some(QueuedModal::ContactSelect(_)) => ModalType::ContactSelect,
            Some(QueuedModal::Confirm { .. }) => ModalType::Confirm,
            Some(_) => ModalType::None, // Screen-specific modals
            None => ModalType::None,
        }
    }

    /// Get reference to account setup state if it's the active modal
    #[must_use]
    pub fn account_setup_state(&self) -> Option<&AccountSetupModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(state)) => Some(state),
            _ => None,
        }
    }

    /// Get mutable reference to account setup state if it's the active modal
    pub fn account_setup_state_mut(&mut self) -> Option<&mut AccountSetupModalState> {
        match self.modal_queue.current_mut() {
            Some(QueuedModal::AccountSetup(state)) => Some(state),
            _ => None,
        }
    }

    /// Signal that account creation succeeded
    pub fn account_created(&mut self) {
        self.account_created_queued();
    }

    /// Signal that account creation failed
    pub fn account_creation_failed(&mut self, error: String) {
        self.account_creation_failed_queued(error);
    }

    // ========================================================================
    // Modal Type Checking (for tests and rendering)
    // ========================================================================

    /// Check if chat create modal is active
    #[must_use]
    pub fn is_chat_create_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatCreate(_)))
    }

    /// Get chat create modal state if active
    #[must_use]
    pub fn chat_create_modal_state(&self) -> Option<&CreateChannelModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::ChatCreate(state)) => Some(state),
            _ => None,
        }
    }

    /// Get mutable chat create modal state if active
    pub fn chat_create_modal_state_mut(&mut self) -> Option<&mut CreateChannelModalState> {
        match self.modal_queue.current_mut() {
            Some(QueuedModal::ChatCreate(state)) => Some(state),
            _ => None,
        }
    }

    /// Check if chat topic modal is active
    #[must_use]
    pub fn is_chat_topic_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatTopic(_)))
    }

    /// Get chat topic modal state if active
    #[must_use]
    pub fn chat_topic_modal_state(&self) -> Option<&TopicModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::ChatTopic(state)) => Some(state),
            _ => None,
        }
    }

    /// Check if chat info modal is active
    #[must_use]
    pub fn is_chat_info_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatInfo(_)))
    }

    /// Check if guardian setup modal is active
    #[must_use]
    pub fn is_guardian_setup_modal_active(&self) -> bool {
        matches!(
            self.modal_queue.current(),
            Some(QueuedModal::GuardianSetup(_))
        )
    }

    /// Get guardian setup modal state if active
    #[must_use]
    pub fn guardian_setup_modal_state(&self) -> Option<&GuardianSetupModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::GuardianSetup(state)) => Some(state),
            _ => None,
        }
    }
}

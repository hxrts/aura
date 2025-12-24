//! Modal Queue System
//!
//! Type-enforced modal queue that ensures only one modal is visible at a time.

use std::collections::VecDeque;

use crate::tui::screens::Screen;

use super::views::{
    AccountSetupModalState, AddDeviceModalState, ChannelInfoModalState, ConfirmRemoveModalState,
    CreateChannelModalState, CreateInvitationModalState, DeviceEnrollmentCeremonyModalState,
    DisplayNameModalState, GuardianSetupModalState, ImportInvitationModalState,
    InvitationCodeModalState, NicknameModalState, ThresholdModalState, TopicModalState,
};

/// Unified modal enum - ALL modals MUST be one of these variants.
///
/// This enum enforces that all modals go through the queue system.
/// Each variant carries its own state, eliminating scattered `visible: bool` fields.
///
/// ## Adding New Modals
///
/// When adding a new modal:
/// 1. Add a variant here with the appropriate state struct
/// 2. Add rendering in `shell.rs` via the `render_queued_modal` match
/// 3. Use `modal_queue.enqueue(QueuedModal::YourModal(...))` to show it
///
/// **DO NOT** add `visible: bool` fields to modal state structs.
#[derive(Clone, Debug)]
pub enum QueuedModal {
    // ========================================================================
    // Global Modals (can appear from any screen)
    // ========================================================================
    /// Account setup wizard (shown before main UI)
    AccountSetup(AccountSetupModalState),

    /// Help modal with optional screen context
    Help { current_screen: Option<Screen> },

    /// Generic confirmation dialog
    Confirm {
        title: String,
        message: String,
        on_confirm: Option<ConfirmAction>,
    },

    /// Guardian selection from contacts
    GuardianSelect(ContactSelectModalState),

    /// Contact selection (generic)
    ContactSelect(ContactSelectModalState),

    // ========================================================================
    // Chat Screen Modals
    // ========================================================================
    /// Create a new channel
    ChatCreate(CreateChannelModalState),

    /// Select channel members (multi-select contact picker)
    ChatMemberSelect(ChatMemberSelectModalState),

    /// Edit channel topic
    ChatTopic(TopicModalState),

    /// View channel info
    ChatInfo(ChannelInfoModalState),

    // ========================================================================
    // Contacts Screen Modals
    // ========================================================================
    /// Edit contact nickname
    ContactsNickname(NicknameModalState),

    /// Import invitation (contacts screen)
    ContactsImport(ImportInvitationModalState),

    /// Create invitation (contacts screen)
    ContactsCreate(CreateInvitationModalState),

    /// Show invitation code (contacts screen)
    ContactsCode(InvitationCodeModalState),

    /// Guardian setup wizard (multi-select + threshold + ceremony)
    GuardianSetup(GuardianSetupModalState),

    // ========================================================================
    // Settings Screen Modals
    // ========================================================================
    /// Edit display name
    SettingsDisplayName(DisplayNameModalState),

    /// Configure threshold
    SettingsThreshold(ThresholdModalState),

    /// Add device
    SettingsAddDevice(AddDeviceModalState),

    /// Device enrollment ceremony (code + progress)
    SettingsDeviceEnrollment(DeviceEnrollmentCeremonyModalState),

    /// Confirm device removal
    SettingsRemoveDevice(ConfirmRemoveModalState),

    // ========================================================================
    // Block Screen Modals
    // ========================================================================
    /// Invite contact to block
    BlockInvite(ContactSelectModalState),
}

/// Action to perform on confirmation
#[derive(Clone, Debug)]
pub enum ConfirmAction {
    /// Remove a device
    RemoveDevice { device_id: String },
    /// Delete a channel
    DeleteChannel { channel_id: String },
    /// Remove a contact
    RemoveContact { contact_id: String },
    /// Revoke an invitation
    RevokeInvitation { invitation_id: String },
}

/// State for generic contact selection modal
#[derive(Clone, Debug, Default)]
pub struct ContactSelectModalState {
    /// Title for the modal
    pub title: String,
    /// Available contacts (id, name)
    pub contacts: Vec<(String, String)>,
    /// Currently focused index
    pub selected_index: usize,
    /// Selected contact IDs (for multi-select)
    pub selected_ids: Vec<String>,
    /// Whether multi-select is enabled
    pub multi_select: bool,
}

impl ContactSelectModalState {
    /// Create a single-select contact picker
    pub fn single(title: impl Into<String>, contacts: Vec<(String, String)>) -> Self {
        Self {
            title: title.into(),
            contacts,
            selected_index: 0,
            selected_ids: Vec::new(),
            multi_select: false,
        }
    }

    /// Create a multi-select contact picker
    pub fn multi(title: impl Into<String>, contacts: Vec<(String, String)>) -> Self {
        Self {
            title: title.into(),
            contacts,
            selected_index: 0,
            selected_ids: Vec::new(),
            multi_select: true,
        }
    }

    /// Toggle selection of currently focused contact
    pub fn toggle_selection(&mut self) {
        if let Some((id, _)) = self.contacts.get(self.selected_index) {
            if let Some(pos) = self.selected_ids.iter().position(|i| i == id) {
                self.selected_ids.remove(pos);
            } else {
                self.selected_ids.push(id.clone());
            }
        }
    }

    /// Get the currently focused contact ID
    pub fn focused_contact_id(&self) -> Option<&str> {
        self.contacts
            .get(self.selected_index)
            .map(|(id, _)| id.as_str())
    }
}

/// State for chat member selection modal (wraps a multi-select contact picker plus the draft create-channel state)
#[derive(Clone, Debug, Default)]
pub struct ChatMemberSelectModalState {
    pub picker: ContactSelectModalState,
    pub draft: CreateChannelModalState,
}

/// Modal queue that ensures only one modal is visible at a time.
///
/// **Type Enforcement**: This is the ONLY way to show modals.
/// All `visible: bool` fields have been removed from modal state structs.
///
/// ## Usage
///
/// ```rust,ignore
/// // Show a modal
/// state.modal_queue.enqueue(QueuedModal::Help { current_screen: Some(Screen::Chat) });
///
/// // Dismiss current modal (shows next in queue)
/// state.modal_queue.dismiss();
///
/// // Check if modal is active
/// if state.modal_queue.is_active() {
///     // Render modal_queue.current()
/// }
/// ```
#[derive(Clone, Debug, Default)]
pub struct ModalQueue {
    /// Queue of pending modals (FIFO - first in, first out)
    pending: VecDeque<QueuedModal>,
    /// Currently active modal (if any)
    active: Option<QueuedModal>,
}

impl ModalQueue {
    /// Create a new empty modal queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a modal. If no modal is active, it becomes active immediately.
    pub fn enqueue(&mut self, modal: QueuedModal) {
        if self.active.is_none() {
            self.active = Some(modal);
        } else {
            self.pending.push_back(modal);
        }
    }

    /// Dismiss the active modal and activate the next one in the queue (if any).
    /// Returns the dismissed modal.
    pub fn dismiss(&mut self) -> Option<QueuedModal> {
        let dismissed = self.active.take();
        self.active = self.pending.pop_front();
        dismissed
    }

    /// Get a reference to the currently active modal (for rendering).
    pub fn current(&self) -> Option<&QueuedModal> {
        self.active.as_ref()
    }

    /// Get a mutable reference to the currently active modal (for input handling).
    pub fn current_mut(&mut self) -> Option<&mut QueuedModal> {
        self.active.as_mut()
    }

    /// Check if any modal is currently active.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Clear all modals (active and pending). Use for emergency reset.
    pub fn clear(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    /// Get the number of pending modals (not including active).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Replace the active modal's state in place (for updating modal state during interaction).
    /// Returns false if no active modal.
    pub fn update_active<F>(&mut self, f: F) -> bool
    where
        F: FnOnce(&mut QueuedModal),
    {
        if let Some(modal) = &mut self.active {
            f(modal);
            true
        } else {
            false
        }
    }
}

/// Type of modal currently displayed (legacy enum for backwards compatibility)
///
/// ## Compile-Time Safety: Screen-Specific Modals
///
/// Many modal types that were previously here have been **intentionally removed**
/// to prevent a class of bugs where the generic modal system is used but the
/// props extraction expects screen-specific modal state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ModalType {
    /// No modal displayed
    #[default]
    None,
    /// Account setup wizard (global, shown before any screen is active)
    AccountSetup,
    /// Guardian selection from contacts (global modal)
    GuardianSelect,
    /// Contact selection (global modal)
    ContactSelect,
    /// Help modal (global, can be shown from any screen)
    Help,
    /// Confirm action modal (generic confirmation dialog)
    Confirm,
}

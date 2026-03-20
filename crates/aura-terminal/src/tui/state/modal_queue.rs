//! Modal Queue System
//!
//! Type-enforced modal queue that ensures only one modal is visible at a time.

use std::collections::VecDeque;

use super::ids::{AuthorityRef, ChannelId, ContactId, DeviceId, InvitationId};
use crate::tui::screens::Screen;

use super::views::{
    AccessOverrideModalState, AccountSetupModalState, AddDeviceModalState, ChannelInfoModalState,
    ConfirmRemoveModalState, CreateChannelModalState, CreateInvitationModalState,
    DeviceEnrollmentCeremonyModalState, DeviceSelectModalState, GuardianSetupModalState,
    HomeCapabilityConfigModalState, HomeCreateModalState, ImportInvitationModalState,
    InvitationCodeModalState, ModeratorAssignmentModalState, NicknameModalState,
    NicknameSuggestionModalState, TopicModalState,
};

// Re-export portable modal queue constants and types from aura-app
pub use aura_app::ui::types::{
    modal_can_user_dismiss, should_interrupt_modal, ModalPriority, MAX_PENDING_MODALS,
};

// Use the portable constant from aura-app
use aura_app::ui::types::MAX_PENDING_MODALS as PORTABLE_MAX_PENDING;

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

    /// Show invite code (contacts screen)
    ContactsCode(InvitationCodeModalState),

    /// Guardian setup wizard (multi-select + threshold + ceremony)
    GuardianSetup(GuardianSetupModalState),
    /// MFA setup wizard (devices + threshold + ceremony)
    MfaSetup(GuardianSetupModalState),

    // ========================================================================
    // Settings Screen Modals
    // ========================================================================
    /// Edit nickname suggestion (what you want to be called)
    SettingsNicknameSuggestion(NicknameSuggestionModalState),

    /// Add device
    SettingsAddDevice(AddDeviceModalState),

    /// Import device enrollment code (demo/new device)
    SettingsDeviceImport(ImportInvitationModalState),

    /// Device enrollment ceremony (code + progress)
    SettingsDeviceEnrollment(DeviceEnrollmentCeremonyModalState),

    /// Select device for removal
    SettingsDeviceSelect(DeviceSelectModalState),

    /// Confirm device removal
    SettingsRemoveDevice(ConfirmRemoveModalState),

    /// Authority picker (switch between authorities on this device)
    AuthorityPicker(ContactSelectModalState),

    // ========================================================================
    // Neighborhood Screen Modals
    // ========================================================================
    /// Create a new home
    NeighborhoodHomeCreate(HomeCreateModalState),
    /// Assign/revoke moderator designations for home members
    NeighborhoodModeratorAssignment(ModeratorAssignmentModalState),
    /// Apply bounded access-level overrides (Partial/Limited)
    NeighborhoodAccessOverride(AccessOverrideModalState),
    /// Configure Full/Partial/Limited capability sets
    NeighborhoodCapabilityConfig(HomeCapabilityConfigModalState),
}

/// Action to perform on confirmation
#[derive(Clone, Debug)]
pub enum ConfirmAction {
    /// Remove a device
    RemoveDevice { device_id: DeviceId },
    /// Delete a channel
    DeleteChannel { channel_id: ChannelId },
    /// Remove a contact
    RemoveContact { contact_id: ContactId },
    /// Revoke an invitation
    RevokeInvitation { invitation_id: InvitationId },
}

/// State for generic contact selection modal
#[derive(Clone, Debug, Default)]
pub struct ContactSelectModalState {
    /// Title for the modal
    pub title: String,
    /// Available contacts (id, name)
    pub contacts: Vec<(AuthorityRef, String)>,
    /// Currently focused index
    pub selected_index: usize,
    /// Selected contact IDs (for multi-select)
    pub selected_ids: Vec<AuthorityRef>,
    /// Whether multi-select is enabled
    pub multi_select: bool,
}

impl ContactSelectModalState {
    /// Create a single-select contact picker
    pub fn single(title: impl Into<String>, contacts: Vec<(AuthorityRef, String)>) -> Self {
        Self {
            title: title.into(),
            contacts,
            selected_index: 0,
            selected_ids: Vec::new(),
            multi_select: false,
        }
    }

    /// Create a multi-select contact picker
    pub fn multi(title: impl Into<String>, contacts: Vec<(AuthorityRef, String)>) -> Self {
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
    #[must_use]
    pub fn focused_contact_id(&self) -> Option<&AuthorityRef> {
        self.contacts.get(self.selected_index).map(|(id, _)| id)
    }

    /// Get total number of contacts
    #[must_use]
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Check if there are no contacts
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a modal. If no modal is active, it becomes active immediately.
    pub fn enqueue(&mut self, modal: QueuedModal) {
        if self.active.is_none() {
            self.active = Some(modal);
        } else {
            // Use portable constant from aura-app
            if self.pending.len() >= PORTABLE_MAX_PENDING {
                // Drop the oldest pending modal to keep memory bounded.
                let _ = self.pending.pop_front();
            }
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
    #[must_use]
    pub fn current(&self) -> Option<&QueuedModal> {
        self.active.as_ref()
    }

    /// Get a mutable reference to the currently active modal (for input handling).
    pub fn current_mut(&mut self) -> Option<&mut QueuedModal> {
        self.active.as_mut()
    }

    /// Check if any modal is currently active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Clear all modals (active and pending). Use for emergency reset.
    pub fn clear(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    /// Get the number of pending modals (not including active).
    #[must_use]
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

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
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_terminal::tui::state_machine::{TuiState, TuiCommand, transition};
//! use aura_core::effects::terminal::events;
//!
//! let mut state = TuiState::default();
//! let (new_state, commands) = transition(&state, events::char('1'));
//! // new_state.screen == Screen::Block
//! ```

use crate::tui::navigation::{navigate_list, GridNav, NavKey};
use crate::tui::types::{InvitationFilter, MfaPolicy, RecoveryTab, SettingsSection};
use crate::tui::{Router, Screen};
use aura_core::effects::terminal::{KeyCode, KeyEvent, TerminalEvent};
use std::collections::VecDeque;

// ============================================================================
// Modal Queue System (Type-Enforced)
// ============================================================================

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

    /// Edit channel topic
    ChatTopic(TopicModalState),

    /// View channel info
    ChatInfo(ChannelInfoModalState),

    // ========================================================================
    // Contacts Screen Modals
    // ========================================================================
    /// Edit contact petname
    ContactsPetname(PetnameModalState),

    /// Import invitation (contacts screen)
    ContactsImport(ImportInvitationModalState),

    /// Create invitation (contacts screen)
    ContactsCreate(CreateInvitationModalState),

    /// Show invitation code (contacts screen)
    ContactsCode(InvitationCodeModalState),

    /// Guardian setup wizard (multi-select + threshold + ceremony)
    GuardianSetup(GuardianSetupModalState),

    // ========================================================================
    // Invitations Screen Modals
    // ========================================================================
    /// Create invitation
    InvitationsCreate(CreateInvitationModalState),

    /// Import invitation
    InvitationsImport(ImportInvitationModalState),

    /// Show invitation code
    InvitationsCode(InvitationCodeModalState),

    // ========================================================================
    // Settings Screen Modals
    // ========================================================================
    /// Edit nickname
    SettingsNickname(NicknameModalState),

    /// Configure threshold
    SettingsThreshold(ThresholdModalState),

    /// Add device
    SettingsAddDevice(AddDeviceModalState),

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

// ============================================================================
// Toast Queue System (Type-Enforced)
// ============================================================================

/// Toast queue that ensures only one toast is visible at a time.
///
/// **Type Enforcement**: This is the ONLY way to show toasts.
/// Remove `Vec<Toast>` fields and use this queue instead.
///
/// ## Behavior
///
/// - Toasts are shown in FIFO order
/// - Auto-dismiss via `tick()` when `ticks_remaining` reaches 0
/// - Manual dismiss via `dismiss()` or Escape key
/// - One modal + one toast can coexist (different screen regions)
#[derive(Clone, Debug, Default)]
pub struct ToastQueue {
    /// Queue of pending toasts (FIFO)
    pending: VecDeque<QueuedToast>,
    /// Currently active toast (if any)
    active: Option<QueuedToast>,
}

/// A queued toast notification
#[derive(Clone, Debug)]
pub struct QueuedToast {
    /// Unique ID for this toast
    pub id: u64,
    /// Toast message
    pub message: String,
    /// Severity level
    pub level: ToastLevel,
    /// Ticks remaining before auto-dismiss
    pub ticks_remaining: u32,
}

impl QueuedToast {
    /// Create a new toast with default duration (30 ticks = ~3 seconds at 100ms/tick)
    pub fn new(id: u64, message: impl Into<String>, level: ToastLevel) -> Self {
        Self {
            id,
            message: message.into(),
            level,
            ticks_remaining: 30,
        }
    }

    /// Create with custom duration
    pub fn with_duration(mut self, ticks: u32) -> Self {
        self.ticks_remaining = ticks;
        self
    }

    /// Create an info toast
    pub fn info(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Info)
    }

    /// Create a success toast
    pub fn success(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Success)
    }

    /// Create a warning toast
    pub fn warning(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Warning)
    }

    /// Create an error toast
    pub fn error(id: u64, message: impl Into<String>) -> Self {
        Self::new(id, message, ToastLevel::Error)
    }
}

impl ToastQueue {
    /// Create a new empty toast queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue a toast. If no toast is active, it becomes active immediately.
    pub fn enqueue(&mut self, toast: QueuedToast) {
        if self.active.is_none() {
            self.active = Some(toast);
        } else {
            self.pending.push_back(toast);
        }
    }

    /// Dismiss the active toast and activate the next one in the queue (if any).
    /// Returns the dismissed toast.
    pub fn dismiss(&mut self) -> Option<QueuedToast> {
        let dismissed = self.active.take();
        self.active = self.pending.pop_front();
        dismissed
    }

    /// Get a reference to the currently active toast (for rendering).
    pub fn current(&self) -> Option<&QueuedToast> {
        self.active.as_ref()
    }

    /// Check if any toast is currently active.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Process a tick: decrement timer and auto-dismiss expired toasts.
    /// Returns true if a toast was auto-dismissed.
    pub fn tick(&mut self) -> bool {
        if let Some(toast) = &mut self.active {
            toast.ticks_remaining = toast.ticks_remaining.saturating_sub(1);
            if toast.ticks_remaining == 0 {
                self.active = self.pending.pop_front();
                return true;
            }
        }
        false
    }

    /// Clear all toasts (active and pending).
    pub fn clear(&mut self) {
        self.active = None;
        self.pending.clear();
    }

    /// Get the number of pending toasts (not including active).
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

// ============================================================================
// Legacy Modal State (to be migrated)
// ============================================================================

/// Type of modal currently displayed
///
/// ## Compile-Time Safety: Screen-Specific Modals
///
/// Many modal types that were previously here have been **intentionally removed**
/// to prevent a class of bugs where the generic modal system is used but the
/// props extraction expects screen-specific modal state.
///
/// **REMOVED** (use screen-specific modals instead):
/// - `CreateChannel` → use `state.chat.create_modal`
/// - `ChannelInfo` → use `state.chat.info_modal`
/// - `SetTopic` → use `state.chat.topic_modal`
/// - `CreateInvitation` → use `state.invitations.create_modal`
/// - `ImportInvitation` → use `state.invitations.import_modal`
/// - `ExportInvitation` → use `state.invitations.code_modal`
/// - `InvitationCode` → use `state.invitations.code_modal`
/// - `ThresholdConfig` → use `state.settings.threshold_modal`
/// - `TextInput` → use screen-specific modal (e.g., `petname_modal`, `nickname_modal`)
///
/// If you need to add a new modal, add it as a screen-specific modal struct
/// in the appropriate screen state (e.g., `ChatState`, `SettingsState`), NOT here.
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

/// State for account setup modal
#[derive(Clone, Debug, Default)]
pub struct AccountSetupModalState {
    /// Current display name input
    pub display_name: String,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Timestamp (ms since epoch) when creating started - for debounced spinner
    pub creating_started_ms: Option<u64>,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

/// Debounce threshold for showing spinner (ms)
pub const SPINNER_DEBOUNCE_MS: u64 = 300;

impl AccountSetupModalState {
    /// Whether we can submit the form
    pub fn can_submit(&self) -> bool {
        !self.display_name.trim().is_empty() && !self.creating && !self.success
    }

    /// Start the creating state with timestamp for debounced spinner
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.creating_started_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        );
        self.error = None;
    }

    /// Check if spinner should be shown (creating AND elapsed > debounce threshold)
    pub fn should_show_spinner(&self) -> bool {
        if !self.creating {
            return false;
        }
        let Some(started) = self.creating_started_ms else {
            return false;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now.saturating_sub(started) >= SPINNER_DEBOUNCE_MS
    }

    /// Set success state
    pub fn set_success(&mut self) {
        self.creating = false;
        self.creating_started_ms = None;
        self.success = true;
    }

    /// Set error state
    pub fn set_error(&mut self, msg: String) {
        self.creating = false;
        self.creating_started_ms = None;
        self.error = Some(msg);
    }

    /// Reset to input state (for retry after error)
    pub fn reset_to_input(&mut self) {
        self.creating = false;
        self.creating_started_ms = None;
        self.success = false;
        self.error = None;
    }
}

// ============================================================================
// Screen-Specific State
// ============================================================================

/// Focus for two-panel screens
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelFocus {
    #[default]
    List,
    Detail,
}

/// Block screen focus
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockFocus {
    #[default]
    Residents,
    Messages,
    Input,
}

/// Block screen state
#[derive(Clone, Debug, Default)]
pub struct BlockViewState {
    /// Current focus panel
    pub focus: BlockFocus,
    /// Whether in insert mode (typing message)
    pub insert_mode: bool,
    /// Character used to enter insert mode (to prevent it being typed)
    pub insert_mode_entry_char: Option<char>,
    /// Input buffer for message composition
    pub input_buffer: String,
    /// Selected resident index (for resident list)
    pub selected_resident: usize,
    /// Total resident count (for wrap-around navigation)
    pub resident_count: usize,
    /// Message scroll position
    pub message_scroll: usize,
    /// Total message count (for wrap-around navigation)
    pub message_count: usize,
    /// Whether showing resident panel
    pub show_residents: bool,
    /// Whether invite modal is open
    pub invite_modal_open: bool,
    /// Selected contact index in invite modal
    pub invite_selection: usize,
    /// Total contacts available to invite (for wrap-around navigation)
    pub invite_contact_count: usize,
}

/// Chat screen state
#[derive(Clone, Debug, Default)]
pub struct ChatViewState {
    /// Current focus (channels, messages, input)
    pub focus: ChatFocus,
    /// Selected channel index
    pub selected_channel: usize,
    /// Total channel count (for wrap-around navigation)
    pub channel_count: usize,
    /// Scroll position in message list
    pub message_scroll: usize,
    /// Total message count (for wrap-around navigation)
    pub message_count: usize,
    /// Input buffer for message composition
    pub input_buffer: String,
    /// Whether in insert mode
    pub insert_mode: bool,
    /// Character used to enter insert mode (to prevent it being typed)
    pub insert_mode_entry_char: Option<char>,
    /// Create channel modal state
    pub create_modal: CreateChannelModalState,
    /// Topic edit modal state
    pub topic_modal: TopicModalState,
    /// Channel info modal state
    pub info_modal: ChannelInfoModalState,
}

/// State for create channel modal
#[derive(Clone, Debug, Default)]
pub struct CreateChannelModalState {
    /// Whether visible
    pub visible: bool,
    /// Channel name input
    pub name: String,
    /// Optional topic input
    pub topic: String,
    /// Current input field (0 = name, 1 = topic)
    pub active_field: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl CreateChannelModalState {
    pub fn show(&mut self) {
        self.visible = true;
        self.name.clear();
        self.topic.clear();
        self.active_field = 0;
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.name.clear();
        self.topic.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

/// State for topic edit modal
#[derive(Clone, Debug, Default)]
pub struct TopicModalState {
    /// Whether visible
    pub visible: bool,
    /// Topic input value
    pub value: String,
    /// Channel ID being edited
    pub channel_id: String,
    /// Error message if any
    pub error: Option<String>,
}

impl TopicModalState {
    pub fn show(&mut self, channel_id: &str, current_topic: &str) {
        self.visible = true;
        self.channel_id = channel_id.to_string();
        self.value = current_topic.to_string();
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.value.clear();
        self.channel_id.clear();
        self.error = None;
    }
}

/// State for channel info modal
#[derive(Clone, Debug, Default)]
pub struct ChannelInfoModalState {
    /// Whether visible
    pub visible: bool,
    /// Channel ID
    pub channel_id: String,
    /// Channel name
    pub channel_name: String,
    /// Channel topic
    pub topic: String,
    /// Participants
    pub participants: Vec<String>,
}

impl ChannelInfoModalState {
    pub fn show(&mut self, channel_id: &str, name: &str, topic: Option<&str>) {
        self.visible = true;
        self.channel_id = channel_id.to_string();
        self.channel_name = name.to_string();
        self.topic = topic.unwrap_or("").to_string();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.channel_id.clear();
        self.channel_name.clear();
        self.topic.clear();
        self.participants.clear();
    }
}

/// Chat screen focus
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatFocus {
    /// Channel list has focus
    #[default]
    Channels,
    /// Message list has focus
    Messages,
    /// Input field has focus
    Input,
}

/// Contacts screen state
#[derive(Clone, Debug, Default)]
pub struct ContactsViewState {
    /// Panel focus (list or detail)
    pub focus: PanelFocus,
    /// Selected contact index
    pub selected_index: usize,
    /// Total contact count (for wrap-around navigation)
    pub contact_count: usize,
    /// Filter text
    pub filter: String,
    /// Petname edit modal state
    pub petname_modal: PetnameModalState,
    /// Import invitation modal state (accept an invitation code)
    pub import_modal: ImportInvitationModalState,
    /// Create invitation modal state (send an invitation)
    pub create_modal: CreateInvitationModalState,
    /// Invitation code display modal state (show generated code)
    pub code_modal: InvitationCodeModalState,
    /// Guardian setup modal state (multi-select guardians + threshold)
    pub guardian_setup_modal: GuardianSetupModalState,
    /// Demo mode: Alice's invitation code (for Ctrl+a shortcut)
    pub demo_alice_code: String,
    /// Demo mode: Carol's invitation code (for Ctrl+l shortcut)
    pub demo_carol_code: String,
}

/// State for petname edit modal
#[derive(Clone, Debug, Default)]
pub struct PetnameModalState {
    /// Whether visible
    pub visible: bool,
    /// Contact ID being edited
    pub contact_id: String,
    /// Current petname value
    pub value: String,
    /// Error message if any
    pub error: Option<String>,
}

impl PetnameModalState {
    pub fn show(&mut self, contact_id: &str, current_name: &str) {
        self.visible = true;
        self.contact_id = contact_id.to_string();
        self.value = current_name.to_string();
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.contact_id.clear();
        self.value.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.value.trim().is_empty()
    }
}

/// Invitations screen state
#[derive(Clone, Debug, Default)]
pub struct InvitationsViewState {
    /// Panel focus (list or detail)
    pub focus: PanelFocus,
    /// Selected invitation index
    pub selected_index: usize,
    /// Total invitation count (for wrap-around navigation)
    pub invitation_count: usize,
    /// Current filter
    pub filter: InvitationFilter,
    /// Create invitation modal state
    pub create_modal: CreateInvitationModalState,
    /// Import invitation modal state
    pub import_modal: ImportInvitationModalState,
    /// Invitation code display modal state
    pub code_modal: InvitationCodeModalState,
    /// Demo mode: Alice's invitation code (for Ctrl+a shortcut)
    pub demo_alice_code: String,
    /// Demo mode: Carol's invitation code (for Ctrl+l shortcut)
    pub demo_carol_code: String,
}

/// State for create invitation modal
#[derive(Clone, Debug, Default)]
pub struct CreateInvitationModalState {
    /// Whether visible
    pub visible: bool,
    /// Invitation type selection index
    pub type_index: usize,
    /// Optional message
    pub message: String,
    /// TTL in hours
    pub ttl_hours: u64,
    /// Current step (0 = type, 1 = message, 2 = ttl)
    pub step: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl CreateInvitationModalState {
    pub fn show(&mut self) {
        self.visible = true;
        self.type_index = 0;
        self.message.clear();
        self.ttl_hours = 24; // Default 24 hours
        self.step = 0;
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.message.clear();
        self.error = None;
    }

    pub fn next_step(&mut self) {
        if self.step < 2 {
            self.step += 1;
        }
    }

    pub fn prev_step(&mut self) {
        if self.step > 0 {
            self.step -= 1;
        }
    }
}

/// State for import invitation modal
#[derive(Clone, Debug, Default)]
pub struct ImportInvitationModalState {
    /// Whether visible
    pub visible: bool,
    /// Code input buffer
    pub code: String,
    /// Error message if any
    pub error: Option<String>,
    /// Whether import is in progress
    pub importing: bool,
}

impl ImportInvitationModalState {
    pub fn show(&mut self) {
        self.visible = true;
        self.code.clear();
        self.error = None;
        self.importing = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.code.clear();
        self.error = None;
        self.importing = false;
    }

    pub fn can_submit(&self) -> bool {
        !self.code.trim().is_empty() && !self.importing
    }
}

/// State for invitation code display modal
#[derive(Clone, Debug, Default)]
pub struct InvitationCodeModalState {
    /// Whether visible
    pub visible: bool,
    /// Invitation ID
    pub invitation_id: String,
    /// The code to display
    pub code: String,
    /// Whether code is loading
    pub loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

impl InvitationCodeModalState {
    pub fn show(&mut self, invitation_id: &str) {
        self.visible = true;
        self.invitation_id = invitation_id.to_string();
        self.code.clear();
        self.loading = true;
        self.error = None;
    }

    pub fn set_code(&mut self, code: String) {
        self.code = code;
        self.loading = false;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.invitation_id.clear();
        self.code.clear();
        self.loading = false;
        self.error = None;
    }
}

// ============================================================================
// Guardian Setup Modal State
// ============================================================================

/// Step in the guardian setup wizard
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GuardianSetupStep {
    /// Step 1: Select contacts to become guardians
    #[default]
    SelectContacts,
    /// Step 2: Choose threshold (k of n)
    ChooseThreshold,
    /// Step 3: Ceremony in progress, waiting for responses
    CeremonyInProgress,
}

/// Response status for a guardian in a ceremony
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GuardianCeremonyResponse {
    /// Waiting for response
    Pending,
    /// Guardian accepted
    Accepted,
    /// Guardian declined
    Declined,
}

/// A contact that can be selected as a guardian
#[derive(Clone, Debug, Default)]
pub struct GuardianCandidate {
    /// Contact ID
    pub id: String,
    /// Display name
    pub name: String,
    /// Whether this contact is currently a guardian
    pub is_current_guardian: bool,
}

/// State for guardian setup modal (multi-select + threshold + ceremony)
#[derive(Clone, Debug, Default)]
pub struct GuardianSetupModalState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current step in the wizard
    pub step: GuardianSetupStep,
    /// Available contacts for selection
    pub contacts: Vec<GuardianCandidate>,
    /// Indices of selected contacts (using Vec for order preservation)
    pub selected_indices: Vec<usize>,
    /// Currently focused contact index
    pub focused_index: usize,
    /// Selected threshold k (required signers)
    pub threshold_k: u8,
    /// Ceremony ID (set when ceremony starts)
    pub ceremony_id: Option<String>,
    /// Responses from guardians during ceremony (contact_id -> response)
    pub ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>, // (id, name, response)
    /// Error message if any
    pub error: Option<String>,
    /// Whether there's already a pending ceremony (prevents starting another)
    pub has_pending_ceremony: bool,
}

impl GuardianSetupModalState {
    /// Get total selected guardians (n)
    pub fn threshold_n(&self) -> u8 {
        self.selected_indices.len() as u8
    }

    /// Show the modal with contacts and pre-select current guardians
    pub fn show(&mut self, contacts: Vec<GuardianCandidate>) {
        self.visible = true;
        self.step = GuardianSetupStep::SelectContacts;
        self.selected_indices.clear();
        // Pre-select current guardians
        for (idx, contact) in contacts.iter().enumerate() {
            if contact.is_current_guardian {
                self.selected_indices.push(idx);
            }
        }
        self.contacts = contacts;
        self.focused_index = 0;
        // Default threshold: majority (n/2 + 1) or 1 if no selection
        let n = self.selected_indices.len() as u8;
        self.threshold_k = if n > 0 { (n / 2) + 1 } else { 1 };
        self.ceremony_id = None;
        self.ceremony_responses.clear();
        self.error = None;
    }

    /// Hide the modal and reset state
    pub fn hide(&mut self) {
        self.visible = false;
        self.step = GuardianSetupStep::SelectContacts;
        self.contacts.clear();
        self.selected_indices.clear();
        self.focused_index = 0;
        self.threshold_k = 1;
        self.ceremony_id = None;
        self.ceremony_responses.clear();
        self.error = None;
    }

    /// Toggle selection of the currently focused contact
    pub fn toggle_selection(&mut self) {
        if let Some(pos) = self
            .selected_indices
            .iter()
            .position(|&i| i == self.focused_index)
        {
            self.selected_indices.remove(pos);
        } else {
            self.selected_indices.push(self.focused_index);
        }
        // Adjust threshold_k if it exceeds new n
        let n = self.threshold_n();
        if self.threshold_k > n && n > 0 {
            self.threshold_k = n;
        }
    }

    /// Check if a contact index is selected
    pub fn is_selected(&self, index: usize) -> bool {
        self.selected_indices.contains(&index)
    }

    /// Increment threshold k (up to n)
    pub fn increment_k(&mut self) {
        let n = self.threshold_n();
        if self.threshold_k < n {
            self.threshold_k += 1;
        }
    }

    /// Decrement threshold k (down to 1)
    pub fn decrement_k(&mut self) {
        if self.threshold_k > 1 {
            self.threshold_k -= 1;
        }
    }

    /// Check if can proceed from contact selection to threshold step
    pub fn can_proceed_to_threshold(&self) -> bool {
        self.selected_indices.len() >= 2 // Need at least 2 guardians
    }

    /// Check if can start ceremony
    pub fn can_start_ceremony(&self) -> bool {
        let n = self.threshold_n();
        self.threshold_k >= 1 && self.threshold_k <= n && n >= 2 && !self.has_pending_ceremony
    }

    /// Start the ceremony (called when user confirms)
    pub fn start_ceremony(&mut self, ceremony_id: String) {
        self.step = GuardianSetupStep::CeremonyInProgress;
        self.ceremony_id = Some(ceremony_id);
        self.has_pending_ceremony = true;
        // Initialize responses for all selected contacts
        self.ceremony_responses.clear();
        for &idx in &self.selected_indices {
            if let Some(contact) = self.contacts.get(idx) {
                self.ceremony_responses.push((
                    contact.id.clone(),
                    contact.name.clone(),
                    GuardianCeremonyResponse::Pending,
                ));
            }
        }
    }

    /// Record a guardian's response
    pub fn record_response(&mut self, guardian_id: &str, accepted: bool) {
        for (id, _, response) in &mut self.ceremony_responses {
            if id == guardian_id {
                *response = if accepted {
                    GuardianCeremonyResponse::Accepted
                } else {
                    GuardianCeremonyResponse::Declined
                };
                break;
            }
        }
    }

    /// Check if all guardians have accepted
    pub fn all_accepted(&self) -> bool {
        !self.ceremony_responses.is_empty()
            && self
                .ceremony_responses
                .iter()
                .all(|(_, _, r)| *r == GuardianCeremonyResponse::Accepted)
    }

    /// Check if any guardian has declined
    pub fn any_declined(&self) -> bool {
        self.ceremony_responses
            .iter()
            .any(|(_, _, r)| *r == GuardianCeremonyResponse::Declined)
    }

    /// Get list of selected contact IDs
    pub fn selected_contact_ids(&self) -> Vec<String> {
        self.selected_indices
            .iter()
            .filter_map(|&idx| self.contacts.get(idx).map(|c| c.id.clone()))
            .collect()
    }

    /// Complete the ceremony successfully
    pub fn complete_ceremony(&mut self) {
        self.has_pending_ceremony = false;
        self.hide();
    }

    /// Fail/cancel the ceremony
    pub fn fail_ceremony(&mut self, reason: &str) {
        self.has_pending_ceremony = false;
        self.error = Some(reason.to_string());
        self.step = GuardianSetupStep::SelectContacts;
        self.ceremony_id = None;
        self.ceremony_responses.clear();
    }
}

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

/// Settings screen state
#[derive(Clone, Debug, Default)]
pub struct SettingsViewState {
    /// Panel focus (menu or detail)
    pub focus: PanelFocus,
    /// Current section
    pub section: SettingsSection,
    /// Selected item in current section
    pub selected_index: usize,
    /// Current MFA policy
    pub mfa_policy: MfaPolicy,
    /// Nickname edit modal state
    pub nickname_modal: NicknameModalState,
    /// Threshold config modal state
    pub threshold_modal: ThresholdModalState,
    /// Add device modal state
    pub add_device_modal: AddDeviceModalState,
    /// Remove device confirm modal state
    pub confirm_remove_modal: ConfirmRemoveModalState,
}

/// State for nickname edit modal
#[derive(Clone, Debug, Default)]
pub struct NicknameModalState {
    /// Whether visible
    pub visible: bool,
    /// Nickname input buffer
    pub value: String,
    /// Error message if any
    pub error: Option<String>,
}

impl NicknameModalState {
    pub fn show(&mut self, current_name: &str) {
        self.visible = true;
        self.value = current_name.to_string();
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.value.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.value.trim().is_empty()
    }
}

/// State for threshold config modal
#[derive(Clone, Debug, Default)]
pub struct ThresholdModalState {
    /// Whether visible
    pub visible: bool,
    /// Threshold K (required signatures)
    pub k: u8,
    /// Threshold N (total guardians)
    pub n: u8,
    /// Active field (0 = k, 1 = n)
    pub active_field: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl ThresholdModalState {
    pub fn show(&mut self, current_k: u8, current_n: u8) {
        self.visible = true;
        self.k = current_k;
        self.n = current_n;
        self.active_field = 0;
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.error = None;
    }

    pub fn increment_k(&mut self) {
        if self.k < self.n {
            self.k += 1;
        }
    }

    pub fn decrement_k(&mut self) {
        if self.k > 1 {
            self.k -= 1;
        }
    }

    pub fn increment_n(&mut self) {
        self.n = self.n.saturating_add(1);
    }

    pub fn decrement_n(&mut self) {
        if self.n > self.k {
            self.n -= 1;
        }
    }

    pub fn can_submit(&self) -> bool {
        self.k > 0 && self.k <= self.n && self.n > 0
    }
}

/// State for add device modal
#[derive(Clone, Debug, Default)]
pub struct AddDeviceModalState {
    /// Whether visible
    pub visible: bool,
    /// Device name input
    pub name: String,
    /// Error message if any
    pub error: Option<String>,
}

impl AddDeviceModalState {
    pub fn show(&mut self) {
        self.visible = true;
        self.name.clear();
        self.error = None;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.name.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
    }
}

/// State for confirm remove device modal
#[derive(Clone, Debug, Default)]
pub struct ConfirmRemoveModalState {
    /// Whether visible
    pub visible: bool,
    /// Device ID to remove
    pub device_id: String,
    /// Device name (for display)
    pub device_name: String,
    /// Whether confirm button is focused (vs cancel)
    pub confirm_focused: bool,
}

impl ConfirmRemoveModalState {
    pub fn show(&mut self, device_id: &str, device_name: &str) {
        self.visible = true;
        self.device_id = device_id.to_string();
        self.device_name = device_name.to_string();
        self.confirm_focused = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.device_id.clear();
        self.device_name.clear();
        self.confirm_focused = false;
    }

    pub fn toggle_focus(&mut self) {
        self.confirm_focused = !self.confirm_focused;
    }
}

/// Neighborhood screen state
#[derive(Clone, Debug, Default)]
pub struct NeighborhoodViewState {
    /// Grid navigation state (handles 2D wrap-around)
    pub grid: GridNav,
}

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

// ============================================================================
// TUI State
// ============================================================================

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
    /// Block screen state
    pub block: BlockViewState,

    /// Chat screen state
    pub chat: ChatViewState,

    /// Contacts screen state
    pub contacts: ContactsViewState,

    /// Invitations screen state
    pub invitations: InvitationsViewState,

    /// Recovery screen state
    pub recovery: RecoveryViewState,

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
}

/// Toast notification
#[derive(Clone, Debug)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub level: ToastLevel,
    pub duration_ms: u64,
    pub created_at: u64,
    /// Ticks remaining before auto-dismiss (decremented on each Tick event)
    /// Default: 30 ticks (~3 seconds at 100ms/tick)
    pub ticks_remaining: u32,
}

/// Toast severity level
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl ToastLevel {
    /// Get the dismissal priority (higher = dismiss first on Escape)
    /// Priority: Error (3) > Warning (2) > Info/Success (1)
    pub fn priority(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warning => 2,
            Self::Info | Self::Success => 1,
        }
    }
}

impl TuiState {
    /// Create a new TUI state with default values
    pub fn new() -> Self {
        #[allow(deprecated)]
        Self {
            terminal_size: (80, 24),
            ..Default::default()
        }
    }

    /// Create a TUI state with specific terminal size
    pub fn with_size(width: u16, height: u16) -> Self {
        #[allow(deprecated)]
        Self {
            terminal_size: (width, height),
            ..Default::default()
        }
    }

    /// Create a TUI state with the account setup modal visible (via queue)
    pub fn with_account_setup() -> Self {
        let mut state = Self::new();
        state.show_account_setup_queued();
        state
    }

    /// Get the current screen
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
    pub fn account_created_queued(&mut self) {
        if let Some(QueuedModal::AccountSetup(ref mut state)) = self.modal_queue.current_mut() {
            state.set_success();
        }
    }

    /// Signal that account creation failed (queue-based)
    pub fn account_creation_failed_queued(&mut self, error: String) {
        if let Some(QueuedModal::AccountSetup(ref mut state)) = self.modal_queue.current_mut() {
            state.set_error(error);
        }
    }

    /// Check if the current modal is a text input modal (queue-based)
    pub fn is_queued_modal_text_input(&self) -> bool {
        match self.modal_queue.current() {
            Some(QueuedModal::AccountSetup(_)) => true,
            Some(QueuedModal::ChatCreate(_)) => true,
            Some(QueuedModal::ChatTopic(_)) => true,
            Some(QueuedModal::ContactsPetname(_)) => true,
            Some(QueuedModal::ContactsImport(_)) => true,
            Some(QueuedModal::InvitationsImport(_)) => true,
            Some(QueuedModal::SettingsNickname(_)) => true,
            Some(QueuedModal::SettingsAddDevice(_)) => true,
            _ => false,
        }
    }

    /// Check if in insert mode (for text input)
    pub fn is_insert_mode(&self) -> bool {
        match self.screen() {
            Screen::Block => self.block.insert_mode,
            Screen::Chat => self.chat.insert_mode,
            _ => false,
        }
    }

    // ========================================================================
    // Modal Helper Methods
    // ========================================================================

    /// Check if any modal is active
    pub fn has_modal(&self) -> bool {
        self.has_queued_modal()
    }

    /// Get the current modal type (for backwards compatibility in tests)
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

    /// Check if block invite modal is active
    pub fn is_block_invite_modal_active(&self) -> bool {
        matches!(
            self.modal_queue.current(),
            Some(QueuedModal::BlockInvite(_))
        )
    }

    /// Check if chat create modal is active
    pub fn is_chat_create_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatCreate(_)))
    }

    /// Get chat create modal state if active
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
    pub fn is_chat_topic_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatTopic(_)))
    }

    /// Get chat topic modal state if active
    pub fn chat_topic_modal_state(&self) -> Option<&TopicModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::ChatTopic(state)) => Some(state),
            _ => None,
        }
    }

    /// Check if chat info modal is active
    pub fn is_chat_info_modal_active(&self) -> bool {
        matches!(self.modal_queue.current(), Some(QueuedModal::ChatInfo(_)))
    }

    /// Check if guardian setup modal is active
    pub fn is_guardian_setup_modal_active(&self) -> bool {
        matches!(
            self.modal_queue.current(),
            Some(QueuedModal::GuardianSetup(_))
        )
    }

    /// Get guardian setup modal state if active
    pub fn guardian_setup_modal_state(&self) -> Option<&GuardianSetupModalState> {
        match self.modal_queue.current() {
            Some(QueuedModal::GuardianSetup(state)) => Some(state),
            _ => None,
        }
    }
}

// ============================================================================
// TUI Commands
// ============================================================================

/// Command representing a side effect
///
/// Commands are produced by state transitions and executed by the runtime.
/// They represent all effects that cannot be handled purely.
#[derive(Clone, Debug)]
pub enum TuiCommand {
    /// Exit the TUI
    Exit,

    /// Show a toast notification
    ShowToast { message: String, level: ToastLevel },

    /// Dismiss a toast notification
    DismissToast { id: u64 },

    /// Clear all toast notifications (e.g., on Escape)
    ClearAllToasts,

    /// Dispatch an effect command to the app core
    Dispatch(DispatchCommand),

    /// Request a re-render
    Render,
}

/// Commands to dispatch to the app core
#[derive(Clone, Debug)]
pub enum DispatchCommand {
    // Navigation
    NavigateTo(Screen),

    // Block screen
    SendBlockMessage {
        content: String,
    },
    InviteToBlock {
        contact_id: String,
    },
    GrantSteward {
        resident_id: String,
    },
    RevokeSteward {
        resident_id: String,
    },

    // Chat screen
    SelectChannel {
        channel_id: String,
    },
    SendChatMessage {
        channel_id: String,
        content: String,
    },
    RetryMessage {
        message_id: String,
    },
    CreateChannel {
        name: String,
    },
    SetChannelTopic {
        channel_id: String,
        topic: String,
    },
    DeleteChannel {
        channel_id: String,
    },

    // Contacts screen
    UpdatePetname {
        contact_id: String,
        petname: String,
    },
    StartChat {
        contact_id: String,
    },
    RemoveContact {
        contact_id: String,
    },
    /// Contact selection by index (for generic contact select modals)
    SelectContactByIndex {
        index: usize,
    },

    // Guardian ceremony
    /// Start a guardian ceremony with selected contacts and threshold
    StartGuardianCeremony {
        contact_ids: Vec<String>,
        threshold_k: u8,
    },
    /// Cancel an in-progress guardian ceremony
    CancelGuardianCeremony,

    // Invitations screen
    AcceptInvitation {
        invitation_id: String,
    },
    DeclineInvitation {
        invitation_id: String,
    },
    CreateInvitation {
        invitation_type: String,
        message: Option<String>,
    },
    ImportInvitation {
        code: String,
    },
    ExportInvitation {
        invitation_id: String,
    },
    RevokeInvitation {
        invitation_id: String,
    },

    // Recovery screen
    StartRecovery,
    AddGuardian {
        contact_id: String,
    },
    ApproveRecovery {
        request_id: String,
    },

    // Settings screen
    UpdateNickname {
        nickname: String,
    },
    UpdateThreshold {
        k: u8,
        n: u8,
    },
    UpdateMfaPolicy {
        policy: MfaPolicy,
    },
    AddDevice {
        name: String,
    },
    RemoveDevice {
        device_id: String,
    },

    // Neighborhood screen
    EnterBlock {
        block_id: String,
    },
    GoHome,
    BackToStreet,

    // Account setup
    CreateAccount {
        name: String,
    },
}

// ============================================================================
// Pure Transition Function
// ============================================================================

/// Pure state transition function
///
/// Given the current state and an input event, produces a new state and
/// a list of commands to execute. This function has no side effects.
///
/// # Arguments
///
/// * `state` - Current TUI state
/// * `event` - Terminal event to process
///
/// # Returns
///
/// A tuple of (new state, commands to execute)
pub fn transition(state: &TuiState, event: TerminalEvent) -> (TuiState, Vec<TuiCommand>) {
    let mut new_state = state.clone();
    let mut commands = Vec::new();

    match event {
        TerminalEvent::Key(key) => {
            handle_key_event(&mut new_state, &mut commands, key);
        }
        TerminalEvent::Resize { width, height } => {
            new_state.terminal_size = (width, height);
        }
        TerminalEvent::Tick => {
            // Time-based updates: tick the toast queue (handles decrement and auto-dismiss)
            new_state.toast_queue.tick();
        }
        _ => {
            // Ignore other events for now (mouse, focus, paste)
        }
    }

    (new_state, commands)
}

/// Handle a key event
fn handle_key_event(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Queued modal gets priority (all modals are now queue-based)
    if state.has_queued_modal() {
        handle_modal_key(state, commands, key);
        return;
    }

    // Insert mode gets priority
    if state.is_insert_mode() {
        handle_insert_mode_key(state, commands, key);
        return;
    }

    // Global keys
    if handle_global_key(state, commands, &key) {
        return;
    }

    // Screen-specific keys
    match state.screen() {
        Screen::Block => handle_block_key(state, commands, key),
        Screen::Chat => handle_chat_key(state, commands, key),
        Screen::Contacts => handle_contacts_key(state, commands, key),
        Screen::Neighborhood => handle_neighborhood_key(state, commands, key),
        Screen::Settings => handle_settings_key(state, commands, key),
        Screen::Recovery => handle_recovery_key(state, commands, key),
    }
}

/// Handle global keys (available in all screens)
fn handle_global_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: &KeyEvent) -> bool {
    // Quit
    if key.code == KeyCode::Char('q') && !key.modifiers.shift() {
        state.should_exit = true;
        commands.push(TuiCommand::Exit);
        return true;
    }

    // Ctrl+C - force quit
    if key.code == KeyCode::Char('c') && key.modifiers.ctrl() {
        state.should_exit = true;
        commands.push(TuiCommand::Exit);
        return true;
    }

    // Escape - dismiss ONE toast at a time (when no modal is open)
    // Note: Modal escape handling is in handle_modal_key, so this only fires
    // when there's no modal open
    if key.code == KeyCode::Esc {
        if state.toast_queue.is_active() {
            // Dismiss the current toast (queue automatically shows next one)
            state.toast_queue.dismiss();
        }
        // If no toasts, Esc does nothing here (modals handled in handle_modal_key)
        return true;
    }

    // Help (?)
    if key.code == KeyCode::Char('?') {
        state.modal_queue.enqueue(QueuedModal::Help {
            current_screen: Some(state.screen()),
        });
        return true;
    }

    // Number keys for screen navigation (1-7)
    if let KeyCode::Char(c) = key.code {
        if let Some(digit) = c.to_digit(10) {
            if let Some(screen) = Screen::from_key(digit as u8) {
                state.router.go_to(screen);
                return true;
            }
        }
    }

    // Tab - next screen
    if key.code == KeyCode::Tab && !key.modifiers.shift() {
        state.router.next_tab();
        return true;
    }

    // Shift+Tab - previous screen
    if key.code == KeyCode::BackTab || (key.code == KeyCode::Tab && key.modifiers.shift()) {
        state.router.prev_tab();
        return true;
    }

    false
}

/// Handle modal key events (queue-based only)
fn handle_modal_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Handle queued modal key events
    if let Some(queued_modal) = state.modal_queue.current().cloned() {
        handle_queued_modal_key(state, commands, key, queued_modal);
    }
}

/// Handle insert mode key events
fn handle_insert_mode_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Capture screen type once to avoid borrow conflicts
    let screen = state.screen();

    // Escape exits insert mode
    if key.code == KeyCode::Esc {
        match screen {
            Screen::Block => {
                state.block.insert_mode = false;
                state.block.insert_mode_entry_char = None;
            }
            Screen::Chat => {
                state.chat.insert_mode = false;
                state.chat.insert_mode_entry_char = None;
            }
            _ => {}
        }
        return;
    }

    // Get the entry char to check if we need to consume it
    let entry_char = match screen {
        Screen::Block => state.block.insert_mode_entry_char,
        Screen::Chat => state.chat.insert_mode_entry_char,
        _ => None,
    };

    match key.code {
        KeyCode::Char(c) => {
            // If this char matches the entry char, consume it but don't add to buffer
            if entry_char == Some(c) {
                match screen {
                    Screen::Block => state.block.insert_mode_entry_char = None,
                    Screen::Chat => state.chat.insert_mode_entry_char = None,
                    _ => {}
                }
            } else {
                // Clear entry char and add char to buffer
                match screen {
                    Screen::Block => {
                        state.block.insert_mode_entry_char = None;
                        state.block.input_buffer.push(c);
                    }
                    Screen::Chat => {
                        state.chat.insert_mode_entry_char = None;
                        state.chat.input_buffer.push(c);
                    }
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => match screen {
            Screen::Block => {
                state.block.insert_mode_entry_char = None;
                state.block.input_buffer.pop();
            }
            Screen::Chat => {
                state.chat.insert_mode_entry_char = None;
                state.chat.input_buffer.pop();
            }
            _ => {}
        },
        KeyCode::Enter => {
            match screen {
                Screen::Block => {
                    if !state.block.input_buffer.is_empty() {
                        let content = state.block.input_buffer.clone();
                        state.block.input_buffer.clear();
                        commands.push(TuiCommand::Dispatch(DispatchCommand::SendBlockMessage {
                            content,
                        }));
                        // Exit insert mode after sending
                        state.block.insert_mode = false;
                        state.block.insert_mode_entry_char = None;
                        state.block.focus = BlockFocus::Residents;
                    }
                }
                Screen::Chat => {
                    if !state.chat.input_buffer.is_empty() {
                        let content = state.chat.input_buffer.clone();
                        state.chat.input_buffer.clear();
                        commands.push(TuiCommand::Dispatch(DispatchCommand::SendChatMessage {
                            channel_id: String::new(), // Will be filled by runtime
                            content,
                        }));
                        // Exit insert mode after sending
                        state.chat.insert_mode = false;
                        state.chat.insert_mode_entry_char = None;
                        state.chat.focus = ChatFocus::Messages;
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Handle account setup modal keys (queue-based)
fn handle_account_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    current_state: AccountSetupModalState,
) {
    // If we're in success state, Enter dismisses
    if current_state.success {
        if key.code == KeyCode::Enter {
            state.modal_queue.dismiss();
        }
        return;
    }

    // If we're in error state, Enter resets to input
    if current_state.error.is_some() {
        if key.code == KeyCode::Enter {
            // Reset to input state
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.reset_to_input();
                }
            });
        }
        return;
    }

    // If we're creating, don't process input
    if current_state.creating {
        return;
    }

    // Normal input handling
    match key.code {
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.display_name.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::AccountSetup(ref mut s) = modal {
                    s.display_name.pop();
                }
            });
        }
        KeyCode::Enter => {
            if current_state.can_submit() {
                let name = current_state.display_name.clone();
                state.modal_queue.update_active(|modal| {
                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                        s.start_creating();
                    }
                });
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateAccount {
                    name,
                }));
            }
        }
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle queue-based modal key events (unified dispatcher)
///
/// This routes key events to the appropriate handler based on the QueuedModal variant.
/// All new modal handlers should use this queue-based system.
fn handle_queued_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal: QueuedModal,
) {
    // First, check for toast dismissal on Esc (toasts have priority)
    if key.code == KeyCode::Esc {
        if let Some(toast_id) = state.toast_queue.current().map(|t| t.id.clone()) {
            state.toast_queue.dismiss();
            commands.push(TuiCommand::DismissToast { id: toast_id });
            return;
        }
    }

    // Route to specific handlers based on modal type
    match modal {
        QueuedModal::AccountSetup(modal_state) => {
            handle_account_setup_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::Help { .. } => {
            handle_help_modal_key_queue(state, key);
        }
        QueuedModal::Confirm { on_confirm, .. } => {
            handle_confirm_modal_key_queue(state, commands, key, on_confirm);
        }
        QueuedModal::GuardianSelect(modal_state) => {
            handle_guardian_select_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ContactSelect(modal_state) => {
            handle_contact_select_key_queue(state, commands, key, modal_state);
        }
        // Block screen modals
        QueuedModal::BlockInvite(modal_state) => {
            handle_block_invite_key_queue(state, commands, key, modal_state);
        }
        // Chat screen modals
        QueuedModal::ChatCreate(modal_state) => {
            handle_chat_create_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatTopic(modal_state) => {
            handle_chat_topic_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ChatInfo(_) => {
            // Info modal is read-only - just Esc to dismiss
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
        // Contacts screen modals
        QueuedModal::ContactsPetname(modal_state) => {
            handle_petname_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::ContactsImport(modal_state) => {
            handle_import_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::ContactsCreate(modal_state) => {
            handle_create_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::ContactsCode(_) => {
            // Code display modal is read-only - just Esc to dismiss
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
        QueuedModal::GuardianSetup(modal_state) => {
            handle_guardian_setup_key_queue(state, commands, key, modal_state);
        }
        // Invitations screen modals (invitations are under Contacts screen)
        QueuedModal::InvitationsCreate(modal_state) => {
            handle_create_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::InvitationsImport(modal_state) => {
            handle_import_invitation_key_queue(state, commands, key, modal_state, Screen::Contacts);
        }
        QueuedModal::InvitationsCode(_) => {
            // Code display modal is read-only - just Esc to dismiss
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
        // Settings screen modals
        QueuedModal::SettingsNickname(modal_state) => {
            handle_settings_nickname_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsThreshold(modal_state) => {
            handle_settings_threshold_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsAddDevice(modal_state) => {
            handle_settings_add_device_key_queue(state, commands, key, modal_state);
        }
        QueuedModal::SettingsRemoveDevice(modal_state) => {
            handle_settings_remove_device_key_queue(state, commands, key, modal_state);
        }
    }
}

/// Handle help modal keys (queue-based)
fn handle_help_modal_key_queue(state: &mut TuiState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.help.scroll = navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Up);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.help.scroll =
                navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Down);
        }
        _ => {}
    }
}

/// Handle confirm modal keys (queue-based)
fn handle_confirm_modal_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    on_confirm: Option<ConfirmAction>,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
            // Execute confirm action if provided
            if let Some(action) = on_confirm {
                match action {
                    ConfirmAction::DeleteChannel { channel_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::DeleteChannel {
                            channel_id,
                        }));
                    }
                    ConfirmAction::RemoveContact { contact_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveContact {
                            contact_id,
                        }));
                    }
                    ConfirmAction::RevokeInvitation { invitation_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RevokeInvitation {
                            invitation_id,
                        }));
                    }
                    ConfirmAction::RemoveDevice { device_id } => {
                        commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                            device_id,
                        }));
                    }
                }
            }
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle guardian select modal keys (queue-based)
fn handle_guardian_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::GuardianSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::GuardianSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if let Some((contact_id, _)) = modal_state.contacts.get(modal_state.selected_index) {
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddGuardian {
                    contact_id: contact_id.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        _ => {}
    }
}

/// Handle contact select modal keys (queue-based)
fn handle_contact_select_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ContactSelectModalState,
) {
    let contact_count = modal_state.contacts.len();
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Up);
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactSelect(ref mut s) = modal {
                    s.selected_index =
                        navigate_list(s.selected_index, s.contacts.len(), NavKey::Down);
                }
            });
        }
        KeyCode::Enter => {
            if contact_count > 0 {
                commands.push(TuiCommand::Dispatch(
                    DispatchCommand::SelectContactByIndex {
                        index: modal_state.selected_index,
                    },
                ));
            }
            // Note: Don't dismiss here - let command handler do it
        }
        _ => {}
    }
}

// ============================================================================
// Queue-Based Modal Handlers (New System)
// ============================================================================

/// Handle block invite modal keys (queue-based)
///
/// Note: Navigation uses `state.block.invite_contact_count` and `state.block.invite_selection`
/// because actual contact data is populated by the shell at render time. The modal state
/// just tracks the title and selection mode.
fn handle_block_invite_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    _modal_state: ContactSelectModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // Use block.invite_selection and block.invite_contact_count (like legacy handler)
            state.block.invite_selection = navigate_list(
                state.block.invite_selection,
                state.block.invite_contact_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // Use block.invite_selection and block.invite_contact_count (like legacy handler)
            state.block.invite_selection = navigate_list(
                state.block.invite_selection,
                state.block.invite_contact_count,
                NavKey::Down,
            );
        }
        KeyCode::Enter => {
            // Shell maps index to contact_id - no need to check for contacts here
            let index = state.block.invite_selection;
            commands.push(TuiCommand::Dispatch(DispatchCommand::InviteToBlock {
                contact_id: format!("__index:{}", index),
            }));
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

/// Handle chat create channel modal keys (queue-based)
fn handle_chat_create_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: CreateChannelModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            // Toggle between name and topic fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatCreate(ref mut s) = modal {
                    s.active_field = (s.active_field + 1) % 2;
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                    name: modal_state.name.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatCreate(ref mut s) = modal {
                    if s.active_field == 0 {
                        s.name.push(c);
                    } else {
                        s.topic.push(c);
                    }
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatCreate(ref mut s) = modal {
                    if s.active_field == 0 {
                        s.name.pop();
                    } else {
                        s.topic.pop();
                    }
                }
            });
        }
        _ => {}
    }
}

/// Handle chat topic edit modal keys (queue-based)
fn handle_chat_topic_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: TopicModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::SetChannelTopic {
                channel_id: modal_state.channel_id.clone(),
                topic: modal_state.value.clone(),
            }));
            state.modal_queue.dismiss();
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatTopic(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ChatTopic(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle petname edit modal keys (queue-based)
fn handle_petname_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: PetnameModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdatePetname {
                    contact_id: modal_state.contact_id.clone(),
                    petname: modal_state.value.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsPetname(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::ContactsPetname(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle import invitation modal keys (queue-based)
fn handle_import_invitation_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ImportInvitationModalState,
    _source_screen: Screen,
) {
    // Demo shortcuts: Ctrl+A / Ctrl+L fill Alice/Carol invite codes.
    //
    // These are handled at the state machine layer so they work consistently
    // across ContactsImport and InvitationsImport modals. In production builds
    // the codes are typically empty unless explicitly provided.
    let is_ctrl_a = (key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('a') | KeyCode::Char('A')))
        // Some terminals report Ctrl+a as the control character (SOH, 0x01) with no modifiers.
        || matches!(key.code, KeyCode::Char('\u{1}'));
    let is_ctrl_l = (key.modifiers.ctrl() && matches!(key.code, KeyCode::Char('l') | KeyCode::Char('L')))
        // Some terminals report Ctrl+l as the control character (FF, 0x0c) with no modifiers.
        || matches!(key.code, KeyCode::Char('\u{c}'));

    if is_ctrl_a {
        let code = if !state.contacts.demo_alice_code.is_empty() {
            state.contacts.demo_alice_code.clone()
        } else {
            state.invitations.demo_alice_code.clone()
        };
        if !code.is_empty() {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => s.code = code.clone(),
                QueuedModal::InvitationsImport(ref mut s) => s.code = code.clone(),
                _ => {}
            });
            return;
        }
    } else if is_ctrl_l {
        let code = if !state.contacts.demo_carol_code.is_empty() {
            state.contacts.demo_carol_code.clone()
        } else {
            state.invitations.demo_carol_code.clone()
        };
        if !code.is_empty() {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => s.code = code.clone(),
                QueuedModal::InvitationsImport(ref mut s) => s.code = code.clone(),
                _ => {}
            });
            return;
        }
    }

    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::ImportInvitation {
                    code: modal_state.code.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => s.code.push(c),
                QueuedModal::InvitationsImport(ref mut s) => s.code.push(c),
                _ => {}
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsImport(ref mut s) => {
                    s.code.pop();
                }
                QueuedModal::InvitationsImport(ref mut s) => {
                    s.code.pop();
                }
                _ => {}
            });
        }
        _ => {}
    }
}

/// Handle create invitation modal keys (queue-based)
fn handle_create_invitation_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: CreateInvitationModalState,
    _source_screen: Screen,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            // Navigate to next step
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsCreate(ref mut s) => s.next_step(),
                QueuedModal::InvitationsCreate(ref mut s) => s.next_step(),
                _ => {}
            });
        }
        KeyCode::BackTab => {
            // Navigate to previous step
            state.modal_queue.update_active(|modal| match modal {
                QueuedModal::ContactsCreate(ref mut s) => s.prev_step(),
                QueuedModal::InvitationsCreate(ref mut s) => s.prev_step(),
                _ => {}
            });
        }
        KeyCode::Enter => {
            // On final step, submit
            if modal_state.step == 2 {
                // Convert type_index to invitation type string
                let invitation_type = match modal_state.type_index {
                    0 => "personal".to_string(),
                    1 => "group".to_string(),
                    2 => "guardian".to_string(),
                    _ => "personal".to_string(),
                };
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                    invitation_type,
                    message: if modal_state.message.is_empty() {
                        None
                    } else {
                        Some(modal_state.message.clone())
                    },
                }));
                state.modal_queue.dismiss();
            } else {
                // Advance to next step
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => s.next_step(),
                    QueuedModal::InvitationsCreate(ref mut s) => s.next_step(),
                    _ => {}
                });
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            // In step 0, cycle type selection
            if modal_state.step == 0 {
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => {
                        s.type_index = s.type_index.saturating_sub(1);
                    }
                    QueuedModal::InvitationsCreate(ref mut s) => {
                        s.type_index = s.type_index.saturating_sub(1);
                    }
                    _ => {}
                });
            } else if modal_state.step == 2 {
                // In step 2, increase TTL
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => {
                        s.ttl_hours = s.ttl_hours.saturating_add(24);
                    }
                    QueuedModal::InvitationsCreate(ref mut s) => {
                        s.ttl_hours = s.ttl_hours.saturating_add(24);
                    }
                    _ => {}
                });
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // In step 0, cycle type selection
            if modal_state.step == 0 {
                state.modal_queue.update_active(|modal| {
                    match modal {
                        QueuedModal::ContactsCreate(ref mut s) => {
                            s.type_index = (s.type_index + 1).min(2); // 3 types max
                        }
                        QueuedModal::InvitationsCreate(ref mut s) => {
                            s.type_index = (s.type_index + 1).min(2);
                        }
                        _ => {}
                    }
                });
            } else if modal_state.step == 2 {
                // In step 2, decrease TTL
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => {
                        s.ttl_hours = s.ttl_hours.saturating_sub(24).max(1);
                    }
                    QueuedModal::InvitationsCreate(ref mut s) => {
                        s.ttl_hours = s.ttl_hours.saturating_sub(24).max(1);
                    }
                    _ => {}
                });
            }
        }
        KeyCode::Char(c) => {
            // In step 1, type message
            if modal_state.step == 1 {
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => s.message.push(c),
                    QueuedModal::InvitationsCreate(ref mut s) => s.message.push(c),
                    _ => {}
                });
            }
        }
        KeyCode::Backspace => {
            if modal_state.step == 1 {
                state.modal_queue.update_active(|modal| match modal {
                    QueuedModal::ContactsCreate(ref mut s) => {
                        s.message.pop();
                    }
                    QueuedModal::InvitationsCreate(ref mut s) => {
                        s.message.pop();
                    }
                    _ => {}
                });
            }
        }
        _ => {}
    }
}

/// Handle guardian setup modal keys (queue-based)
fn handle_guardian_setup_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: GuardianSetupModalState,
) {
    match modal_state.step {
        GuardianSetupStep::SelectContacts => {
            match key.code {
                KeyCode::Esc => {
                    state.modal_queue.dismiss();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            if s.focused_index > 0 {
                                s.focused_index -= 1;
                            }
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            if s.focused_index + 1 < s.contacts.len() {
                                s.focused_index += 1;
                            }
                        }
                    });
                }
                KeyCode::Char(' ') => {
                    // Toggle selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.toggle_selection();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_proceed_to_threshold() {
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::GuardianSetup(ref mut s) = modal {
                                s.step = GuardianSetupStep::ChooseThreshold;
                            }
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => {
            match key.code {
                KeyCode::Esc => {
                    // Go back to contact selection
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.step = GuardianSetupStep::SelectContacts;
                        }
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.increment_k();
                        }
                    });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal_queue.update_active(|modal| {
                        if let QueuedModal::GuardianSetup(ref mut s) = modal {
                            s.decrement_k();
                        }
                    });
                }
                KeyCode::Enter => {
                    if modal_state.can_start_ceremony() {
                        // Dispatch command to start guardian setup ceremony
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::StartGuardianCeremony {
                                contact_ids: modal_state.selected_contact_ids(),
                                threshold_k: modal_state.threshold_k,
                            },
                        ));
                        state.modal_queue.dismiss();
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::CeremonyInProgress => {
            // During ceremony, only allow escape to cancel
            if key.code == KeyCode::Esc {
                state.modal_queue.dismiss();
            }
        }
    }
}

/// Handle settings nickname modal keys (queue-based)
fn handle_settings_nickname_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: NicknameModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateNickname {
                    nickname: modal_state.value.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNickname(ref mut s) = modal {
                    s.value.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsNickname(ref mut s) = modal {
                    s.value.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle settings threshold modal keys (queue-based)
fn handle_settings_threshold_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ThresholdModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Tab => {
            // Toggle between k and n fields
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsThreshold(ref mut s) = modal {
                    s.active_field = (s.active_field + 1) % 2;
                }
            });
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsThreshold(ref mut s) = modal {
                    if s.active_field == 0 {
                        s.increment_k();
                    } else {
                        s.increment_n();
                    }
                }
            });
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsThreshold(ref mut s) = modal {
                    if s.active_field == 0 {
                        s.decrement_k();
                    } else {
                        s.decrement_n();
                    }
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateThreshold {
                    k: modal_state.k,
                    n: modal_state.n,
                }));
                state.modal_queue.dismiss();
            }
        }
        _ => {}
    }
}

/// Handle settings add device modal keys (queue-based)
fn handle_settings_add_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: AddDeviceModalState,
) {
    match key.code {
        KeyCode::Esc => {
            state.modal_queue.dismiss();
        }
        KeyCode::Enter => {
            if modal_state.can_submit() {
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice {
                    name: modal_state.name.clone(),
                }));
                state.modal_queue.dismiss();
            }
        }
        KeyCode::Char(c) => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.name.push(c);
                }
            });
        }
        KeyCode::Backspace => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsAddDevice(ref mut s) = modal {
                    s.name.pop();
                }
            });
        }
        _ => {}
    }
}

/// Handle settings remove device modal keys (queue-based)
fn handle_settings_remove_device_key_queue(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
    modal_state: ConfirmRemoveModalState,
) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            state.modal_queue.dismiss();
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab => {
            state.modal_queue.update_active(|modal| {
                if let QueuedModal::SettingsRemoveDevice(ref mut s) = modal {
                    s.toggle_focus();
                }
            });
        }
        KeyCode::Enter => {
            if modal_state.confirm_focused {
                commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                    device_id: modal_state.device_id.clone(),
                }));
            }
            state.modal_queue.dismiss();
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                device_id: modal_state.device_id.clone(),
            }));
            state.modal_queue.dismiss();
        }
        _ => {}
    }
}

// ============================================================================
// Screen-Specific Key Handlers
// ============================================================================

fn handle_block_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Block invite modal is now handled via queue system
    match key.code {
        KeyCode::Char('i') => {
            state.block.insert_mode = true;
            state.block.insert_mode_entry_char = Some('i');
            state.block.focus = BlockFocus::Input;
        }
        // Left/Right navigation between panels (with wrap-around)
        KeyCode::Left | KeyCode::Char('h') => {
            state.block.focus = match state.block.focus {
                BlockFocus::Residents => BlockFocus::Messages, // Wrap to last
                BlockFocus::Messages => BlockFocus::Residents,
                BlockFocus::Input => BlockFocus::Input, // Don't change in input mode
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.block.focus = match state.block.focus {
                BlockFocus::Residents => BlockFocus::Messages,
                BlockFocus::Messages => BlockFocus::Residents, // Wrap to first
                BlockFocus::Input => BlockFocus::Input,        // Don't change in input mode
            };
        }
        // Up/Down navigation within panels
        KeyCode::Up | KeyCode::Char('k') => match state.block.focus {
            BlockFocus::Residents => {
                state.block.selected_resident = navigate_list(
                    state.block.selected_resident,
                    state.block.resident_count,
                    NavKey::Up,
                );
            }
            BlockFocus::Messages => {
                state.block.message_scroll = navigate_list(
                    state.block.message_scroll,
                    state.block.message_count,
                    NavKey::Up,
                );
            }
            BlockFocus::Input => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match state.block.focus {
            BlockFocus::Residents => {
                state.block.selected_resident = navigate_list(
                    state.block.selected_resident,
                    state.block.resident_count,
                    NavKey::Down,
                );
            }
            BlockFocus::Messages => {
                state.block.message_scroll = navigate_list(
                    state.block.message_scroll,
                    state.block.message_count,
                    NavKey::Down,
                );
            }
            BlockFocus::Input => {}
        },
        KeyCode::Char('v') => {
            // Open invite modal via queue (contacts populated by shell)
            state
                .modal_queue
                .enqueue(QueuedModal::BlockInvite(ContactSelectModalState::single(
                    "Invite to Block",
                    Vec::new(),
                )));
        }
        KeyCode::Char('g') => {
            // Grant steward to selected resident
            commands.push(TuiCommand::Dispatch(DispatchCommand::GrantSteward {
                resident_id: String::new(), // Will be filled by runtime based on selected_resident
            }));
        }
        KeyCode::Char('R') => {
            // Revoke steward from selected resident (uppercase R to not conflict with toggle residents)
            commands.push(TuiCommand::Dispatch(DispatchCommand::RevokeSteward {
                resident_id: String::new(), // Will be filled by runtime based on selected_resident
            }));
        }
        KeyCode::Char('r') => {
            state.block.show_residents = !state.block.show_residents;
        }
        KeyCode::Char('n') => {
            // Navigate to neighborhood
            commands.push(TuiCommand::Dispatch(DispatchCommand::NavigateTo(
                Screen::Neighborhood,
            )));
        }
        _ => {}
    }
}

fn handle_chat_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Char('i') => {
            state.chat.insert_mode = true;
            state.chat.insert_mode_entry_char = Some('i');
            state.chat.focus = ChatFocus::Input;
        }
        // Left/Right navigation between panels (with wrap-around)
        KeyCode::Left | KeyCode::Char('h') => {
            state.chat.focus = match state.chat.focus {
                ChatFocus::Channels => ChatFocus::Messages, // Wrap to last
                ChatFocus::Messages => ChatFocus::Channels,
                ChatFocus::Input => ChatFocus::Input, // Don't change in input mode
            };
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.chat.focus = match state.chat.focus {
                ChatFocus::Channels => ChatFocus::Messages,
                ChatFocus::Messages => ChatFocus::Channels, // Wrap to first
                ChatFocus::Input => ChatFocus::Input,       // Don't change in input mode
            };
        }
        KeyCode::Up | KeyCode::Char('k') => match state.chat.focus {
            ChatFocus::Channels => {
                state.chat.selected_channel = navigate_list(
                    state.chat.selected_channel,
                    state.chat.channel_count,
                    NavKey::Up,
                );
            }
            ChatFocus::Messages => {
                state.chat.message_scroll = navigate_list(
                    state.chat.message_scroll,
                    state.chat.message_count,
                    NavKey::Up,
                );
            }
            _ => {}
        },
        KeyCode::Down | KeyCode::Char('j') => match state.chat.focus {
            ChatFocus::Channels => {
                state.chat.selected_channel = navigate_list(
                    state.chat.selected_channel,
                    state.chat.channel_count,
                    NavKey::Down,
                );
            }
            ChatFocus::Messages => {
                state.chat.message_scroll = navigate_list(
                    state.chat.message_scroll,
                    state.chat.message_count,
                    NavKey::Down,
                );
            }
            _ => {}
        },
        KeyCode::Char('n') => {
            // Open create channel modal via queue
            let mut modal_state = CreateChannelModalState::default();
            modal_state.visible = true;
            state
                .modal_queue
                .enqueue(QueuedModal::ChatCreate(modal_state));
        }
        KeyCode::Char('t') => {
            // Open topic edit modal via queue
            // Channel ID will be set based on currently selected channel
            let mut modal_state = TopicModalState::default();
            modal_state.visible = true;
            state
                .modal_queue
                .enqueue(QueuedModal::ChatTopic(modal_state));
        }
        KeyCode::Char('o') => {
            // Open channel info modal via queue
            let mut modal_state = ChannelInfoModalState::default();
            modal_state.visible = true;
            state
                .modal_queue
                .enqueue(QueuedModal::ChatInfo(modal_state));
        }
        KeyCode::Char('r') => {
            // Retry message (when focused on messages)
            if state.chat.focus == ChatFocus::Messages {
                commands.push(TuiCommand::Dispatch(DispatchCommand::RetryMessage {
                    message_id: String::new(), // Will be filled by runtime based on selection
                }));
            }
        }
        _ => {}
    }
}

fn handle_contacts_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.contacts.selected_index = navigate_list(
                state.contacts.selected_index,
                state.contacts.contact_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.contacts.selected_index = navigate_list(
                state.contacts.selected_index,
                state.contacts.contact_count,
                NavKey::Down,
            );
        }
        KeyCode::Char('e') => {
            // Open petname edit modal via queue
            state
                .modal_queue
                .enqueue(QueuedModal::ContactsPetname(PetnameModalState::default()));
        }
        KeyCode::Char('g') => {
            // Open guardian setup modal via queue (if no pending ceremony)
            if !state.contacts.guardian_setup_modal.has_pending_ceremony {
                state.modal_queue.enqueue(QueuedModal::GuardianSetup(
                    GuardianSetupModalState::default(),
                ));
            } else {
                commands.push(TuiCommand::ShowToast {
                    message: "A guardian ceremony is already in progress".to_string(),
                    level: ToastLevel::Warning,
                });
            }
        }
        KeyCode::Char('c') => {
            // Start chat with selected contact
            commands.push(TuiCommand::Dispatch(DispatchCommand::StartChat {
                contact_id: String::new(), // Will be filled by runtime
            }));
        }
        KeyCode::Char('i') => {
            // Open import invitation modal via queue (accept an invitation code)
            state.modal_queue.enqueue(QueuedModal::ContactsImport(
                ImportInvitationModalState::default(),
            ));
        }
        KeyCode::Char('n') => {
            // Open create invitation modal via queue (send an invitation)
            state.modal_queue.enqueue(QueuedModal::ContactsCreate(
                CreateInvitationModalState::default(),
            ));
        }
        KeyCode::Enter => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::StartChat {
                contact_id: String::new(), // Will be filled by runtime
            }));
        }
        _ => {}
    }
}

fn handle_neighborhood_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.neighborhood.grid.navigate(NavKey::Up);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.neighborhood.grid.navigate(NavKey::Down);
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.neighborhood.grid.navigate(NavKey::Left);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.neighborhood.grid.navigate(NavKey::Right);
        }
        KeyCode::Enter => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::EnterBlock {
                block_id: String::new(), // Will be filled by runtime
            }));
        }
        KeyCode::Char('g') | KeyCode::Char('H') => {
            // Go home
            commands.push(TuiCommand::Dispatch(DispatchCommand::GoHome));
        }
        KeyCode::Char('b') | KeyCode::Esc | KeyCode::Backspace => {
            // Back to street
            commands.push(TuiCommand::Dispatch(DispatchCommand::BackToStreet));
        }
        _ => {}
    }
}

#[allow(dead_code)]
fn handle_invitations_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.invitations.selected_index = navigate_list(
                state.invitations.selected_index,
                state.invitations.invitation_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.invitations.selected_index = navigate_list(
                state.invitations.selected_index,
                state.invitations.invitation_count,
                NavKey::Down,
            );
        }
        KeyCode::Char('f') => {
            state.invitations.filter = state.invitations.filter.next();
        }
        KeyCode::Char('a') | KeyCode::Enter => {
            // Accept invitation
            commands.push(TuiCommand::Dispatch(DispatchCommand::AcceptInvitation {
                invitation_id: String::new(), // Will be filled by runtime
            }));
        }
        KeyCode::Char('d') => {
            commands.push(TuiCommand::Dispatch(DispatchCommand::DeclineInvitation {
                invitation_id: String::new(), // Will be filled by runtime
            }));
        }
        KeyCode::Char('n') => {
            // Open create invitation modal via queue
            state.modal_queue.enqueue(QueuedModal::InvitationsCreate(
                CreateInvitationModalState::default(),
            ));
        }
        KeyCode::Char('i') => {
            // Open import modal via queue
            state.modal_queue.enqueue(QueuedModal::InvitationsImport(
                ImportInvitationModalState::default(),
            ));
        }
        KeyCode::Char('e') => {
            // Export invitation
            commands.push(TuiCommand::Dispatch(DispatchCommand::ExportInvitation {
                invitation_id: String::new(), // Will be filled by runtime
            }));
        }
        _ => {}
    }
}

fn handle_settings_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.settings.section = state.settings.section.prev();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.settings.section = state.settings.section.next();
        }
        KeyCode::Char(' ') => {
            // Space cycles MFA policy when on MFA section
            if state.settings.section == SettingsSection::Mfa {
                state.settings.mfa_policy = state.settings.mfa_policy.next();
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateMfaPolicy {
                    policy: state.settings.mfa_policy,
                }));
            }
        }
        KeyCode::Char('m') => {
            state.settings.mfa_policy = state.settings.mfa_policy.next();
            commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateMfaPolicy {
                policy: state.settings.mfa_policy,
            }));
        }
        KeyCode::Char('e') => {
            if state.settings.section == SettingsSection::Profile {
                // Open nickname edit modal via queue
                state
                    .modal_queue
                    .enqueue(QueuedModal::SettingsNickname(NicknameModalState::default()));
            }
        }
        KeyCode::Enter => {
            match state.settings.section {
                SettingsSection::Profile => {
                    // Open nickname edit modal via queue
                    state
                        .modal_queue
                        .enqueue(QueuedModal::SettingsNickname(NicknameModalState::default()));
                }
                SettingsSection::Threshold => {
                    // Open threshold edit modal via queue
                    state.modal_queue.enqueue(QueuedModal::SettingsThreshold(
                        ThresholdModalState::default(),
                    ));
                }
                _ => {}
            }
        }
        KeyCode::Char('t') => {
            if state.settings.section == SettingsSection::Threshold {
                // Open threshold edit modal via queue
                state.modal_queue.enqueue(QueuedModal::SettingsThreshold(
                    ThresholdModalState::default(),
                ));
            }
        }
        KeyCode::Char('a') => {
            if state.settings.section == SettingsSection::Devices {
                // Open add device modal via queue
                state.modal_queue.enqueue(QueuedModal::SettingsAddDevice(
                    AddDeviceModalState::default(),
                ));
            }
        }
        _ => {}
    }
}

fn handle_recovery_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            state.recovery.tab = state.recovery.tab.prev();
            state.recovery.selected_index = 0;
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.recovery.tab = state.recovery.tab.next();
            state.recovery.selected_index = 0;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.recovery.selected_index = navigate_list(
                state.recovery.selected_index,
                state.recovery.item_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.recovery.selected_index = navigate_list(
                state.recovery.selected_index,
                state.recovery.item_count,
                NavKey::Down,
            );
        }
        KeyCode::Char('a') => {
            if state.recovery.tab == RecoveryTab::Guardians {
                // Show guardian select modal via queue (contacts will be filled by shell)
                state.modal_queue.enqueue(QueuedModal::GuardianSelect(
                    ContactSelectModalState::single("Select Guardian", Vec::new()),
                ));
            }
        }
        KeyCode::Enter => {
            // Enter approves request on Requests tab
            if state.recovery.tab == RecoveryTab::Requests {
                commands.push(TuiCommand::Dispatch(DispatchCommand::ApproveRecovery {
                    request_id: String::new(), // Will be filled by runtime
                }));
            }
        }
        KeyCode::Char('s') | KeyCode::Char('r') => {
            // Start recovery on Recovery tab
            if state.recovery.tab == RecoveryTab::Recovery {
                commands.push(TuiCommand::Dispatch(DispatchCommand::StartRecovery));
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::terminal::events;

    #[test]
    fn test_initial_state() {
        let state = TuiState::new();
        assert_eq!(state.screen(), Screen::Block);
        assert!(!state.has_modal());
        assert!(!state.is_insert_mode());
    }

    #[test]
    fn test_screen_navigation() {
        let state = TuiState::new();

        // Press '2' to go to Chat
        let (new_state, _) = transition(&state, events::char('2'));
        assert_eq!(new_state.screen(), Screen::Chat);

        // Press Tab to go to next screen
        let (new_state, _) = transition(&new_state, events::tab());
        assert_eq!(new_state.screen(), Screen::Contacts);
    }

    #[test]
    fn test_quit() {
        let state = TuiState::new();

        // Press 'q' to quit
        let (new_state, commands) = transition(&state, events::char('q'));
        assert!(new_state.should_exit);
        assert!(commands.iter().any(|c| matches!(c, TuiCommand::Exit)));
    }

    #[test]
    fn test_insert_mode() {
        let state = TuiState::new();

        // Press 'i' to enter insert mode
        let (new_state, _) = transition(&state, events::char('i'));
        assert!(new_state.block.insert_mode);
        assert!(new_state.is_insert_mode());

        // Type some text
        let (new_state, _) = transition(&new_state, events::char('h'));
        let (new_state, _) = transition(&new_state, events::char('i'));
        assert_eq!(new_state.block.input_buffer, "hi");

        // Press Escape to exit insert mode
        let (new_state, _) = transition(&new_state, events::escape());
        assert!(!new_state.block.insert_mode);
        assert!(!new_state.is_insert_mode());
    }

    #[test]
    fn test_help_modal() {
        let state = TuiState::new();

        // Press '?' to open help
        let (new_state, _) = transition(&state, events::char('?'));
        assert!(new_state.has_modal());
        assert_eq!(new_state.current_modal_type(), ModalType::Help);

        // Press Escape to close
        let (new_state, _) = transition(&new_state, events::escape());
        assert!(!new_state.has_modal());
    }

    #[test]
    fn test_send_message_command() {
        let mut state = TuiState::new();
        state.block.insert_mode = true;
        state.block.input_buffer = "hello".to_string();

        // Press Enter to send
        let (new_state, commands) = transition(&state, events::enter());
        assert!(new_state.block.input_buffer.is_empty());
        assert!(commands.iter().any(|c| matches!(
            c,
            TuiCommand::Dispatch(DispatchCommand::SendBlockMessage { content })
            if content == "hello"
        )));
    }

    #[test]
    fn test_resize_event() {
        let state = TuiState::new();

        let (new_state, _) = transition(&state, events::resize(120, 40));
        assert_eq!(new_state.terminal_size, (120, 40));
    }

    #[test]
    fn test_account_setup_modal() {
        let state = TuiState::with_account_setup();

        // Modal should be visible
        assert!(state.has_modal());
        assert_eq!(state.current_modal_type(), ModalType::AccountSetup);

        // Type a name
        let (state, _) = transition(&state, events::char('A'));
        let (state, _) = transition(&state, events::char('l'));
        let (state, _) = transition(&state, events::char('i'));
        let (state, _) = transition(&state, events::char('c'));
        let (state, _) = transition(&state, events::char('e'));
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alice");

        // Submit should dispatch CreateAccount and set creating flag
        let (state, commands) = transition(&state, events::enter());
        assert!(state.account_setup_state().unwrap().creating);
        assert!(commands.iter().any(|c| matches!(
            c,
            TuiCommand::Dispatch(DispatchCommand::CreateAccount { name })
            if name == "Alice"
        )));
    }

    #[test]
    fn test_account_setup_async_feedback() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();
        state.account_setup_state_mut().unwrap().creating = true;

        // Simulate success callback
        state.account_created();
        assert!(state.account_setup_state().unwrap().success);
        assert!(!state.account_setup_state().unwrap().creating);

        // Enter should close modal
        let (state, _) = transition(&state, events::enter());
        assert!(!state.has_modal());
    }

    #[test]
    fn test_account_setup_error_recovery() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();
        state.account_setup_state_mut().unwrap().creating = true;

        // Simulate error callback
        state.account_creation_failed("Network error".to_string());
        assert!(!state.account_setup_state().unwrap().creating);
        assert_eq!(
            state.account_setup_state().unwrap().error,
            Some("Network error".to_string())
        );

        // Enter should reset to input state
        let (state, _) = transition(&state, events::enter());
        assert!(state.account_setup_state().unwrap().error.is_none());
        assert!(!state.account_setup_state().unwrap().success);
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alice"); // Name preserved
    }

    #[test]
    fn test_account_setup_escape() {
        let state = TuiState::with_account_setup();

        // Escape should close modal
        let (state, _) = transition(&state, events::escape());
        assert!(!state.has_modal());
    }

    #[test]
    fn test_account_setup_backspace() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();

        // Backspace should remove character
        let (state, _) = transition(&state, events::backspace());
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alic");
    }
}

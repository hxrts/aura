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

// ============================================================================
// Modal State
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
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

impl AccountSetupModalState {
    /// Whether we can submit the form
    pub fn can_submit(&self) -> bool {
        !self.display_name.trim().is_empty() && !self.creating && !self.success
    }

    /// Start the creating state
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Set success state
    pub fn set_success(&mut self) {
        self.creating = false;
        self.success = true;
    }

    /// Set error state
    pub fn set_error(&mut self, msg: String) {
        self.creating = false;
        self.error = Some(msg);
    }

    /// Reset to input state (for retry after error)
    pub fn reset_to_input(&mut self) {
        self.creating = false;
        self.success = false;
        self.error = None;
    }
}

/// Modal state for any active modal
#[derive(Clone, Debug, Default)]
pub struct ModalState {
    /// Type of modal
    pub modal_type: ModalType,
    /// Selection index within the modal
    pub selection_index: usize,
    /// Number of selectable items (for wrap-around navigation)
    pub selection_count: usize,
    /// Input buffer for text input modals
    pub input_buffer: String,
    /// Secondary input buffer (for modals with multiple inputs)
    pub secondary_input: String,
    /// Current step for multi-step modals
    pub step: usize,
    /// Total steps for multi-step modals
    pub total_steps: usize,
    /// Account setup specific state
    pub account_setup: AccountSetupModalState,
}

impl ModalState {
    /// Create a new modal state
    pub fn new(modal_type: ModalType) -> Self {
        Self {
            modal_type,
            ..Default::default()
        }
    }

    /// Create an account setup modal
    pub fn account_setup() -> Self {
        Self {
            modal_type: ModalType::AccountSetup,
            account_setup: AccountSetupModalState::default(),
            ..Default::default()
        }
    }

    /// Check if a modal is active
    pub fn is_active(&self) -> bool {
        self.modal_type != ModalType::None
    }

    /// Close the modal
    pub fn close(&mut self) {
        self.modal_type = ModalType::None;
        self.selection_index = 0;
        self.input_buffer.clear();
        self.secondary_input.clear();
        self.step = 0;
        self.account_setup = AccountSetupModalState::default();
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
        if self.n < 255 {
            self.n += 1;
        }
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

    /// Active modal (if any)
    pub modal: ModalState,

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

    /// Toast messages (most recent first)
    pub toasts: Vec<Toast>,

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

impl TuiState {
    /// Create a new TUI state with default values
    pub fn new() -> Self {
        Self {
            terminal_size: (80, 24),
            ..Default::default()
        }
    }

    /// Create a TUI state with specific terminal size
    pub fn with_size(width: u16, height: u16) -> Self {
        Self {
            terminal_size: (width, height),
            ..Default::default()
        }
    }

    /// Create a TUI state with the account setup modal visible
    pub fn with_account_setup() -> Self {
        Self {
            modal: ModalState::account_setup(),
            terminal_size: (80, 24),
            ..Default::default()
        }
    }

    /// Get the current screen
    pub fn screen(&self) -> Screen {
        self.router.current()
    }

    /// Check if a modal is active (global or screen-specific)
    pub fn has_modal(&self) -> bool {
        self.modal.is_active() || self.has_screen_modal()
    }

    /// Check if any screen-specific modal is open
    pub fn has_screen_modal(&self) -> bool {
        // Check all screen-specific modals
        self.block.invite_modal_open
            || self.chat.create_modal.visible
            || self.chat.topic_modal.visible
            || self.chat.info_modal.visible
            || self.contacts.petname_modal.visible
            || self.contacts.import_modal.visible
            || self.contacts.create_modal.visible
            || self.contacts.code_modal.visible
            || self.contacts.guardian_setup_modal.visible
            || self.invitations.create_modal.visible
            || self.invitations.import_modal.visible
            || self.invitations.code_modal.visible
            || self.settings.nickname_modal.visible
            || self.settings.threshold_modal.visible
            || self.settings.add_device_modal.visible
            || self.settings.confirm_remove_modal.visible
    }

    /// Check if in insert mode (for text input)
    pub fn is_insert_mode(&self) -> bool {
        match self.screen() {
            Screen::Block => self.block.insert_mode,
            Screen::Chat => self.chat.insert_mode,
            _ => false,
        }
    }

    /// Check if any text input modal is open (where typing goes to input)
    ///
    /// This checks all screen-specific modals that accept text input.
    /// Generic `ModalType::TextInput` was removed to enforce compile-time safety.
    pub fn is_modal_text_input(&self) -> bool {
        self.modal.modal_type == ModalType::AccountSetup
            || self.chat.create_modal.visible
            || self.chat.topic_modal.visible
            || self.contacts.petname_modal.visible
            || self.contacts.import_modal.visible
            || self.invitations.import_modal.visible
            || self.settings.nickname_modal.visible
            || self.settings.add_device_modal.visible
    }

    // ========================================================================
    // External state updates (for async feedback from runtime)
    // ========================================================================

    /// Signal that account creation succeeded
    pub fn account_created(&mut self) {
        if self.modal.modal_type == ModalType::AccountSetup {
            self.modal.account_setup.set_success();
        }
    }

    /// Signal that account creation failed
    pub fn account_creation_failed(&mut self, error: String) {
        if self.modal.modal_type == ModalType::AccountSetup {
            self.modal.account_setup.set_error(error);
        }
    }

    /// Show the account setup modal
    pub fn show_account_setup(&mut self) {
        self.modal = ModalState::account_setup();
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

    // Contacts screen
    UpdatePetname {
        contact_id: String,
        petname: String,
    },
    StartChat {
        contact_id: String,
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

    // Recovery screen
    StartRecovery,
    AddGuardian {
        contact_id: String,
    },
    /// Guardian selection by index (shell maps to contact_id)
    SelectGuardianByIndex {
        index: usize,
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
            // Time-based updates: decrement toast ticks and remove expired toasts
            for toast in &mut new_state.toasts {
                toast.ticks_remaining = toast.ticks_remaining.saturating_sub(1);
            }
            new_state.toasts.retain(|t| t.ticks_remaining > 0);
        }
        _ => {
            // Ignore other events for now (mouse, focus, paste)
        }
    }

    (new_state, commands)
}

/// Handle a key event
fn handle_key_event(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Modal gets priority
    if state.has_modal() {
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

    // Escape - dismiss toasts (when no modal is open)
    // Note: Modal escape handling is in handle_modal_key, so this only fires
    // when there's no modal open
    if key.code == KeyCode::Esc {
        commands.push(TuiCommand::ClearAllToasts);
        return true;
    }

    // Help (?)
    if key.code == KeyCode::Char('?') {
        state.modal = ModalState::new(ModalType::Help);
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

/// Handle modal key events
fn handle_modal_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Check for screen-specific modals first
    if state.has_screen_modal() {
        handle_screen_modal_key(state, commands, key);
        return;
    }

    // Escape closes any global modal
    if key.code == KeyCode::Esc {
        state.modal.close();
        return;
    }

    // Global modal-specific handling
    match state.modal.modal_type {
        ModalType::Help => {
            // Arrow keys for scrolling
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    state.help.scroll =
                        navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Up);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.help.scroll =
                        navigate_list(state.help.scroll, state.help.scroll_max, NavKey::Down);
                }
                _ => {}
            }
        }
        ModalType::AccountSetup => {
            handle_account_setup_key(state, commands, key);
        }
        ModalType::GuardianSelect => {
            // Guardian selection modal
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal.selection_index = navigate_list(
                        state.modal.selection_index,
                        state.modal.selection_count,
                        NavKey::Up,
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal.selection_index = navigate_list(
                        state.modal.selection_index,
                        state.modal.selection_count,
                        NavKey::Down,
                    );
                }
                KeyCode::Enter => {
                    // Dispatch selection by index - shell will map to contact_id
                    // Note: Don't close modal here - let command handler do it after reading contacts
                    let index = state.modal.selection_index;
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::SelectGuardianByIndex { index },
                    ));
                }
                _ => {}
            }
        }
        _ => {
            // Generic modal navigation
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    state.modal.selection_index = navigate_list(
                        state.modal.selection_index,
                        state.modal.selection_count,
                        NavKey::Up,
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.modal.selection_index = navigate_list(
                        state.modal.selection_index,
                        state.modal.selection_count,
                        NavKey::Down,
                    );
                }
                KeyCode::Enter => {
                    // Confirm selection - dispatch command based on modal type
                    // This will be expanded per modal
                }
                _ => {}
            }
        }
    }
}

/// Handle screen-specific modal key events
fn handle_screen_modal_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Block screen modals
    if state.block.invite_modal_open {
        handle_block_invite_modal_key(state, commands, key);
        return;
    }

    // Chat screen modals
    if state.chat.create_modal.visible {
        handle_chat_create_modal_key(state, commands, key);
        return;
    }
    if state.chat.topic_modal.visible {
        handle_chat_topic_modal_key(state, commands, key);
        return;
    }
    if state.chat.info_modal.visible {
        if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
            state.chat.info_modal.hide();
        }
        return;
    }

    // Contacts screen modals
    if state.contacts.petname_modal.visible {
        handle_contacts_petname_modal_key(state, commands, key);
        return;
    }
    if state.contacts.import_modal.visible {
        handle_contacts_import_modal_key(state, commands, key);
        return;
    }
    if state.contacts.create_modal.visible {
        handle_contacts_create_modal_key(state, commands, key);
        return;
    }
    if state.contacts.code_modal.visible {
        if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
            state.contacts.code_modal.hide();
        }
        return;
    }
    if state.contacts.guardian_setup_modal.visible {
        handle_guardian_setup_modal_key(state, commands, key);
        return;
    }

    // Invitations screen modals
    if state.invitations.create_modal.visible {
        handle_invitations_create_modal_key(state, commands, key);
        return;
    }
    if state.invitations.import_modal.visible {
        handle_invitations_import_modal_key(state, commands, key);
        return;
    }
    if state.invitations.code_modal.visible {
        if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
            state.invitations.code_modal.hide();
        }
        return;
    }

    // Settings screen modals
    if state.settings.nickname_modal.visible {
        handle_settings_nickname_modal_key(state, commands, key);
        return;
    }
    if state.settings.threshold_modal.visible {
        handle_settings_threshold_modal_key(state, commands, key);
        return;
    }
    if state.settings.add_device_modal.visible {
        handle_settings_add_device_modal_key(state, commands, key);
        return;
    }
    if state.settings.confirm_remove_modal.visible {
        handle_settings_confirm_remove_modal_key(state, commands, key);
        return;
    }
}

/// Handle block invite modal keys
fn handle_block_invite_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.block.invite_modal_open = false;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.block.invite_selection = navigate_list(
                state.block.invite_selection,
                state.block.invite_contact_count,
                NavKey::Up,
            );
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.block.invite_selection = navigate_list(
                state.block.invite_selection,
                state.block.invite_contact_count,
                NavKey::Down,
            );
        }
        KeyCode::Enter => {
            let index = state.block.invite_selection;
            commands.push(TuiCommand::Dispatch(DispatchCommand::InviteToBlock {
                contact_id: format!("__index:{}", index), // Shell maps index to contact_id
            }));
            state.block.invite_modal_open = false;
        }
        _ => {}
    }
}

/// Handle chat create channel modal keys
fn handle_chat_create_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.chat.create_modal.hide();
        }
        KeyCode::Tab => {
            // Toggle between name and topic fields
            state.chat.create_modal.active_field = (state.chat.create_modal.active_field + 1) % 2;
        }
        KeyCode::Char(c) => {
            if state.chat.create_modal.active_field == 0 {
                state.chat.create_modal.name.push(c);
            } else {
                state.chat.create_modal.topic.push(c);
            }
        }
        KeyCode::Backspace => {
            if state.chat.create_modal.active_field == 0 {
                state.chat.create_modal.name.pop();
            } else {
                state.chat.create_modal.topic.pop();
            }
        }
        KeyCode::Enter => {
            if state.chat.create_modal.can_submit() {
                let name = state.chat.create_modal.name.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateChannel {
                    name,
                }));
                state.chat.create_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle chat topic modal keys
fn handle_chat_topic_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.chat.topic_modal.hide();
        }
        KeyCode::Char(c) => {
            state.chat.topic_modal.value.push(c);
        }
        KeyCode::Backspace => {
            state.chat.topic_modal.value.pop();
        }
        KeyCode::Enter => {
            let channel_id = state.chat.topic_modal.channel_id.clone();
            let topic = state.chat.topic_modal.value.clone();
            commands.push(TuiCommand::Dispatch(DispatchCommand::SetChannelTopic {
                channel_id,
                topic,
            }));
            state.chat.topic_modal.hide();
        }
        _ => {}
    }
}

/// Handle contacts petname modal keys
fn handle_contacts_petname_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.contacts.petname_modal.hide();
        }
        KeyCode::Char(c) => {
            state.contacts.petname_modal.value.push(c);
        }
        KeyCode::Backspace => {
            state.contacts.petname_modal.value.pop();
        }
        KeyCode::Enter => {
            if state.contacts.petname_modal.can_submit() {
                let contact_id = state.contacts.petname_modal.contact_id.clone();
                let petname = state.contacts.petname_modal.value.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdatePetname {
                    contact_id,
                    petname,
                }));
                state.contacts.petname_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle contacts import modal keys (accept invitation code)
fn handle_contacts_import_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    // Check for demo mode shortcuts (Ctrl+a for Alice, Ctrl+l for Carol)
    if key.modifiers.ctrl() {
        match key.code {
            KeyCode::Char('a') => {
                if !state.contacts.demo_alice_code.is_empty() {
                    state.contacts.import_modal.code = state.contacts.demo_alice_code.clone();
                    state.contacts.import_modal.error = None;
                }
                return;
            }
            KeyCode::Char('l') => {
                if !state.contacts.demo_carol_code.is_empty() {
                    state.contacts.import_modal.code = state.contacts.demo_carol_code.clone();
                    state.contacts.import_modal.error = None;
                }
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => {
            state.contacts.import_modal.hide();
        }
        KeyCode::Char(c) => {
            state.contacts.import_modal.code.push(c);
        }
        KeyCode::Backspace => {
            state.contacts.import_modal.code.pop();
        }
        KeyCode::Enter => {
            if state.contacts.import_modal.can_submit() {
                let code = state.contacts.import_modal.code.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::ImportInvitation {
                    code,
                }));
                state.contacts.import_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle contacts create invitation modal keys (send invitation)
fn handle_contacts_create_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    // Invitation types: guardian, friend, contact (3 options)
    const INVITATION_TYPE_COUNT: usize = 3;

    match key.code {
        KeyCode::Esc => {
            state.contacts.create_modal.hide();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.contacts.create_modal.step == 0 {
                state.contacts.create_modal.type_index = navigate_list(
                    state.contacts.create_modal.type_index,
                    INVITATION_TYPE_COUNT,
                    NavKey::Up,
                );
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.contacts.create_modal.step == 0 {
                state.contacts.create_modal.type_index = navigate_list(
                    state.contacts.create_modal.type_index,
                    INVITATION_TYPE_COUNT,
                    NavKey::Down,
                );
            }
        }
        KeyCode::Tab => {
            state.contacts.create_modal.next_step();
        }
        KeyCode::BackTab => {
            state.contacts.create_modal.prev_step();
        }
        KeyCode::Char(c) => {
            if state.contacts.create_modal.step == 1 {
                state.contacts.create_modal.message.push(c);
            }
        }
        KeyCode::Backspace => {
            if state.contacts.create_modal.step == 1 {
                state.contacts.create_modal.message.pop();
            }
        }
        KeyCode::Enter => {
            // Create invitation with selected type
            let type_name = match state.contacts.create_modal.type_index {
                0 => "guardian",
                1 => "friend",
                _ => "contact",
            };
            let message = if state.contacts.create_modal.message.is_empty() {
                None
            } else {
                Some(state.contacts.create_modal.message.clone())
            };
            commands.push(TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                invitation_type: type_name.to_string(),
                message,
            }));
            state.contacts.create_modal.hide();
        }
        _ => {}
    }
}

/// Handle guardian setup modal keys (multi-step wizard)
fn handle_guardian_setup_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    let modal = &mut state.contacts.guardian_setup_modal;

    match modal.step {
        GuardianSetupStep::SelectContacts => {
            match key.code {
                KeyCode::Esc => {
                    modal.hide();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if modal.focused_index > 0 {
                        modal.focused_index -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if modal.focused_index + 1 < modal.contacts.len() {
                        modal.focused_index += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle selection
                    modal.toggle_selection();
                }
                KeyCode::Tab | KeyCode::Enter => {
                    // Proceed to threshold step if enough guardians selected
                    if modal.can_proceed_to_threshold() {
                        modal.step = GuardianSetupStep::ChooseThreshold;
                        // Ensure threshold is valid
                        let n = modal.threshold_n();
                        if modal.threshold_k > n {
                            modal.threshold_k = n;
                        }
                        if modal.threshold_k < 1 && n > 0 {
                            modal.threshold_k = 1;
                        }
                    } else {
                        commands.push(TuiCommand::ShowToast {
                            message: "Select at least 2 contacts to become guardians".to_string(),
                            level: ToastLevel::Warning,
                        });
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::ChooseThreshold => {
            match key.code {
                KeyCode::Esc | KeyCode::BackTab => {
                    // Go back to contact selection
                    modal.step = GuardianSetupStep::SelectContacts;
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    modal.decrement_k();
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    modal.increment_k();
                }
                KeyCode::Enter => {
                    // Start the ceremony
                    if modal.can_start_ceremony() {
                        let contact_ids = modal.selected_contact_ids();
                        let threshold_k = modal.threshold_k;
                        commands.push(TuiCommand::Dispatch(
                            DispatchCommand::StartGuardianCeremony {
                                contact_ids,
                                threshold_k,
                            },
                        ));
                        // Note: The runtime will call modal.start_ceremony() with the actual ceremony_id
                    }
                }
                _ => {}
            }
        }
        GuardianSetupStep::CeremonyInProgress => {
            match key.code {
                KeyCode::Esc => {
                    // Cancel the ceremony
                    commands.push(TuiCommand::Dispatch(
                        DispatchCommand::CancelGuardianCeremony,
                    ));
                    modal.fail_ceremony("Ceremony canceled by user");
                }
                _ => {
                    // Other keys do nothing during ceremony - just wait for responses
                }
            }
        }
    }
}

/// Handle invitations create modal keys
fn handle_invitations_create_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    // Invitation types: guardian, friend, contact (3 options)
    const INVITATION_TYPE_COUNT: usize = 3;

    match key.code {
        KeyCode::Esc => {
            state.invitations.create_modal.hide();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.invitations.create_modal.step == 0 {
                state.invitations.create_modal.type_index = navigate_list(
                    state.invitations.create_modal.type_index,
                    INVITATION_TYPE_COUNT,
                    NavKey::Up,
                );
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.invitations.create_modal.step == 0 {
                state.invitations.create_modal.type_index = navigate_list(
                    state.invitations.create_modal.type_index,
                    INVITATION_TYPE_COUNT,
                    NavKey::Down,
                );
            }
        }
        KeyCode::Tab => {
            state.invitations.create_modal.next_step();
        }
        KeyCode::BackTab => {
            state.invitations.create_modal.prev_step();
        }
        KeyCode::Char(c) => {
            if state.invitations.create_modal.step == 1 {
                state.invitations.create_modal.message.push(c);
            }
        }
        KeyCode::Backspace => {
            if state.invitations.create_modal.step == 1 {
                state.invitations.create_modal.message.pop();
            }
        }
        KeyCode::Enter => {
            // Create invitation with selected type
            let type_name = match state.invitations.create_modal.type_index {
                0 => "guardian",
                1 => "friend",
                _ => "contact",
            };
            let message = if state.invitations.create_modal.message.is_empty() {
                None
            } else {
                Some(state.invitations.create_modal.message.clone())
            };
            commands.push(TuiCommand::Dispatch(DispatchCommand::CreateInvitation {
                invitation_type: type_name.to_string(),
                message,
            }));
            state.invitations.create_modal.hide();
        }
        _ => {}
    }
}

/// Handle invitations import modal keys
fn handle_invitations_import_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    // Check for demo mode shortcuts (Ctrl+a for Alice, Ctrl+l for Carol)
    if key.modifiers.ctrl() {
        match key.code {
            KeyCode::Char('a') => {
                if !state.invitations.demo_alice_code.is_empty() {
                    state.invitations.import_modal.code = state.invitations.demo_alice_code.clone();
                    state.invitations.import_modal.error = None;
                }
                return;
            }
            KeyCode::Char('l') => {
                if !state.invitations.demo_carol_code.is_empty() {
                    state.invitations.import_modal.code = state.invitations.demo_carol_code.clone();
                    state.invitations.import_modal.error = None;
                }
                return;
            }
            _ => {}
        }
    }

    match key.code {
        KeyCode::Esc => {
            state.invitations.import_modal.hide();
        }
        KeyCode::Char(c) => {
            state.invitations.import_modal.code.push(c);
        }
        KeyCode::Backspace => {
            state.invitations.import_modal.code.pop();
        }
        KeyCode::Enter => {
            if state.invitations.import_modal.can_submit() {
                let code = state.invitations.import_modal.code.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::ImportInvitation {
                    code,
                }));
                state.invitations.import_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle settings nickname modal keys
fn handle_settings_nickname_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.settings.nickname_modal.hide();
        }
        KeyCode::Char(c) => {
            state.settings.nickname_modal.value.push(c);
        }
        KeyCode::Backspace => {
            state.settings.nickname_modal.value.pop();
        }
        KeyCode::Enter => {
            if state.settings.nickname_modal.can_submit() {
                let nickname = state.settings.nickname_modal.value.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateNickname {
                    nickname,
                }));
                state.settings.nickname_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle settings threshold modal keys
fn handle_settings_threshold_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.settings.threshold_modal.hide();
        }
        KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
            // Toggle between k and n fields
            state.settings.threshold_modal.active_field =
                (state.settings.threshold_modal.active_field + 1) % 2;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.settings.threshold_modal.active_field == 0 {
                state.settings.threshold_modal.increment_k();
            } else {
                state.settings.threshold_modal.increment_n();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.settings.threshold_modal.active_field == 0 {
                state.settings.threshold_modal.decrement_k();
            } else {
                state.settings.threshold_modal.decrement_n();
            }
        }
        KeyCode::Enter => {
            if state.settings.threshold_modal.can_submit() {
                let k = state.settings.threshold_modal.k;
                let n = state.settings.threshold_modal.n;
                commands.push(TuiCommand::Dispatch(DispatchCommand::UpdateThreshold {
                    k,
                    n,
                }));
                state.settings.threshold_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle settings add device modal keys
fn handle_settings_add_device_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.settings.add_device_modal.hide();
        }
        KeyCode::Char(c) => {
            state.settings.add_device_modal.name.push(c);
        }
        KeyCode::Backspace => {
            state.settings.add_device_modal.name.pop();
        }
        KeyCode::Enter => {
            if state.settings.add_device_modal.can_submit() {
                let name = state.settings.add_device_modal.name.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::AddDevice { name }));
                state.settings.add_device_modal.hide();
            }
        }
        _ => {}
    }
}

/// Handle settings confirm remove modal keys
fn handle_settings_confirm_remove_modal_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            state.settings.confirm_remove_modal.hide();
        }
        KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
            state.settings.confirm_remove_modal.toggle_focus();
        }
        KeyCode::Enter => {
            if state.settings.confirm_remove_modal.confirm_focused {
                let device_id = state.settings.confirm_remove_modal.device_id.clone();
                commands.push(TuiCommand::Dispatch(DispatchCommand::RemoveDevice {
                    device_id,
                }));
            }
            state.settings.confirm_remove_modal.hide();
        }
        _ => {}
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
        KeyCode::Backspace => {
            match screen {
                Screen::Block => {
                    state.block.insert_mode_entry_char = None;
                    state.block.input_buffer.pop();
                }
                Screen::Chat => {
                    state.chat.insert_mode_entry_char = None;
                    state.chat.input_buffer.pop();
                }
                _ => {}
            }
        }
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

/// Handle account setup modal keys
fn handle_account_setup_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    let account_state = &mut state.modal.account_setup;

    // If we're in success state, Enter dismisses
    if account_state.success {
        if key.code == KeyCode::Enter {
            state.modal.close();
        }
        return;
    }

    // If we're in error state, Enter resets to input
    if account_state.error.is_some() {
        if key.code == KeyCode::Enter {
            account_state.reset_to_input();
        }
        return;
    }

    // If we're creating, don't process input
    if account_state.creating {
        return;
    }

    // Normal input handling
    match key.code {
        KeyCode::Char(c) => {
            account_state.display_name.push(c);
        }
        KeyCode::Backspace => {
            account_state.display_name.pop();
        }
        KeyCode::Enter => {
            if account_state.can_submit() {
                let name = account_state.display_name.clone();
                account_state.start_creating();
                commands.push(TuiCommand::Dispatch(DispatchCommand::CreateAccount {
                    name,
                }));
            }
        }
        KeyCode::Esc => {
            state.modal.close();
        }
        _ => {}
    }
}

// ============================================================================
// Screen-Specific Key Handlers
// ============================================================================

fn handle_block_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Handle invite modal if open
    if state.block.invite_modal_open {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                state.block.invite_selection = navigate_list(
                    state.block.invite_selection,
                    state.block.invite_contact_count,
                    NavKey::Up,
                );
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.block.invite_selection = navigate_list(
                    state.block.invite_selection,
                    state.block.invite_contact_count,
                    NavKey::Down,
                );
            }
            KeyCode::Enter => {
                // Confirm invite
                commands.push(TuiCommand::Dispatch(DispatchCommand::InviteToBlock {
                    contact_id: String::new(), // Will be filled by runtime based on invite_selection
                }));
                state.block.invite_modal_open = false;
            }
            KeyCode::Esc => {
                state.block.invite_modal_open = false;
            }
            _ => {}
        }
        return;
    }

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
            // Open invite modal
            state.block.invite_modal_open = true;
            state.block.invite_selection = 0;
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
            // Open create channel modal (screen-specific modal)
            state.chat.create_modal.show();
        }
        KeyCode::Char('t') => {
            // Open topic edit modal (screen-specific modal)
            // Channel ID will be set based on currently selected channel
            state.chat.topic_modal.show("", "");
        }
        KeyCode::Char('o') => {
            // Open channel info modal (screen-specific modal)
            state.chat.info_modal.show("", "", None);
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
            // Open petname edit modal (uses screen-specific modal)
            state.contacts.petname_modal.visible = true;
        }
        KeyCode::Char('g') => {
            // Open guardian setup modal (if no pending ceremony)
            // Note: The modal's show() method will be called by the runtime
            // with the actual contacts list, after this command signals intent.
            // For now, just mark the modal as visible - the runtime will populate it.
            if !state.contacts.guardian_setup_modal.has_pending_ceremony {
                state.contacts.guardian_setup_modal.visible = true;
                state.contacts.guardian_setup_modal.step = GuardianSetupStep::SelectContacts;
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
            // Open import invitation modal (accept an invitation code)
            state.contacts.import_modal.show();
        }
        KeyCode::Char('n') => {
            // Open create invitation modal (send an invitation)
            state.contacts.create_modal.show();
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
            // Open create invitation modal (screen-specific)
            state.invitations.create_modal.visible = true;
        }
        KeyCode::Char('i') => {
            // Open import modal (screen-specific)
            state.invitations.import_modal.visible = true;
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
                // Open nickname edit modal (screen-specific modal)
                state.settings.nickname_modal.visible = true;
            }
        }
        KeyCode::Enter => {
            match state.settings.section {
                SettingsSection::Profile => {
                    // Open nickname edit modal (screen-specific modal)
                    state.settings.nickname_modal.visible = true;
                }
                SettingsSection::Threshold => {
                    state.settings.threshold_modal.visible = true;
                }
                _ => {}
            }
        }
        KeyCode::Char('t') => {
            if state.settings.section == SettingsSection::Threshold {
                state.settings.threshold_modal.visible = true;
            }
        }
        KeyCode::Char('a') => {
            if state.settings.section == SettingsSection::Devices {
                // Open add device modal (screen-specific modal)
                state.settings.add_device_modal.visible = true;
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
                state.modal = ModalState::new(ModalType::GuardianSelect);
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
        assert_eq!(new_state.modal.modal_type, ModalType::Help);

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
        assert_eq!(state.modal.modal_type, ModalType::AccountSetup);

        // Type a name
        let (state, _) = transition(&state, events::char('A'));
        let (state, _) = transition(&state, events::char('l'));
        let (state, _) = transition(&state, events::char('i'));
        let (state, _) = transition(&state, events::char('c'));
        let (state, _) = transition(&state, events::char('e'));
        assert_eq!(state.modal.account_setup.display_name, "Alice");

        // Submit should dispatch CreateAccount and set creating flag
        let (state, commands) = transition(&state, events::enter());
        assert!(state.modal.account_setup.creating);
        assert!(commands.iter().any(|c| matches!(
            c,
            TuiCommand::Dispatch(DispatchCommand::CreateAccount { name })
            if name == "Alice"
        )));
    }

    #[test]
    fn test_account_setup_async_feedback() {
        let mut state = TuiState::with_account_setup();
        state.modal.account_setup.display_name = "Alice".to_string();
        state.modal.account_setup.creating = true;

        // Simulate success callback
        state.account_created();
        assert!(state.modal.account_setup.success);
        assert!(!state.modal.account_setup.creating);

        // Enter should close modal
        let (state, _) = transition(&state, events::enter());
        assert!(!state.has_modal());
    }

    #[test]
    fn test_account_setup_error_recovery() {
        let mut state = TuiState::with_account_setup();
        state.modal.account_setup.display_name = "Alice".to_string();
        state.modal.account_setup.creating = true;

        // Simulate error callback
        state.account_creation_failed("Network error".to_string());
        assert!(!state.modal.account_setup.creating);
        assert_eq!(
            state.modal.account_setup.error,
            Some("Network error".to_string())
        );

        // Enter should reset to input state
        let (state, _) = transition(&state, events::enter());
        assert!(state.modal.account_setup.error.is_none());
        assert!(!state.modal.account_setup.success);
        assert_eq!(state.modal.account_setup.display_name, "Alice"); // Name preserved
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
        state.modal.account_setup.display_name = "Alice".to_string();

        // Backspace should remove character
        let (state, _) = transition(&state, events::backspace());
        assert_eq!(state.modal.account_setup.display_name, "Alic");
    }
}

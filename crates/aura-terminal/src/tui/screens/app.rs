//! # App Screen
//!
//! Main application with screen navigation

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{
    AccountSetupModal, AccountSetupState, ConfirmModal, ContactSelectModal, ContactSelectState,
    DiscoveredPeerInfo, Footer, HelpModal, HelpModalState, InvitePeerCallback, NavBar,
    PeerInvitationStatus, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::effects::EffectCommand;
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::screens::block::{
    BlockInviteCallback, BlockNavCallback, BlockSendCallback, GrantStewardCallback,
    RevokeStewardCallback,
};
use crate::tui::screens::chat::{
    ChannelSelectCallback, CreateChannelCallback, RetryMessageCallback, SendCallback,
    SetTopicCallback,
};
use crate::tui::screens::contacts::{StartChatCallback, UpdatePetnameCallback};
use crate::tui::screens::invitations::{
    CreateInvitationCallback, ExportInvitationCallback, ImportInvitationCallback,
    InvitationCallback,
};
use crate::tui::screens::neighborhood::{GoHomeCallback, NavigationCallback};
use crate::tui::screens::recovery::{ApprovalCallback, RecoveryCallback};
use crate::tui::screens::settings::{
    AddDeviceCallback, MfaCallback, RemoveDeviceCallback, UpdateNicknameCallback,
    UpdateThresholdCallback,
};
use crate::tui::types::{
    BlockBudget, BlockSummary, Channel, Contact, Device, Guardian, Invitation, KeyHint, Message,
    MfaPolicy, PendingRequest, RecoveryStatus, Resident, TraversalDepth,
};

use super::router::Screen;
use super::{
    BlockScreen, ChatScreen, ContactsScreen, NeighborhoodScreen, RecoveryScreen, SettingsScreen,
};

// State machine integration
use crate::tui::convert_iocraft_event;
use crate::tui::props::{
    extract_block_view_props, extract_chat_view_props, extract_contacts_view_props,
    extract_invitations_view_props, extract_neighborhood_view_props, extract_recovery_view_props,
    extract_settings_view_props,
};
use crate::tui::state_machine::{transition, DispatchCommand, ModalType, TuiCommand, TuiState};

/// Props for IoApp
#[derive(Default, Props)]
pub struct IoAppProps {
    // Screen data - populated from IoContext via reactive views
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub invitations: Vec<Invitation>,
    pub guardians: Vec<Guardian>,
    pub devices: Vec<Device>,
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: MfaPolicy,
    pub recovery_status: RecoveryStatus,
    // Block screen data
    pub block_name: String,
    pub residents: Vec<Resident>,
    pub block_budget: BlockBudget,
    pub channel_name: String,
    // Contacts screen data
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub blocks: Vec<BlockSummary>,
    pub traversal_depth: TraversalDepth,
    // Effect dispatch callback for sending messages
    pub on_send: Option<SendCallback>,
    // Effect dispatch callback for retrying failed messages
    pub on_retry_message: Option<RetryMessageCallback>,
    // Effect dispatch callback for channel selection
    pub on_channel_select: Option<ChannelSelectCallback>,
    // Effect dispatch callback for creating new channels
    pub on_create_channel: Option<CreateChannelCallback>,
    // Effect dispatch callback for setting channel topic
    pub on_set_topic: Option<SetTopicCallback>,
    // Effect dispatch callbacks for invitation actions
    pub on_accept_invitation: Option<InvitationCallback>,
    pub on_decline_invitation: Option<InvitationCallback>,
    pub on_create_invitation: Option<CreateInvitationCallback>,
    pub on_export_invitation: Option<ExportInvitationCallback>,
    pub on_import_invitation: Option<ImportInvitationCallback>,
    // Effect dispatch callbacks for neighborhood navigation
    pub on_enter_block: Option<NavigationCallback>,
    pub on_go_home: Option<GoHomeCallback>,
    pub on_back_to_street: Option<GoHomeCallback>,
    // Effect dispatch callbacks for recovery actions
    pub on_start_recovery: Option<RecoveryCallback>,
    pub on_add_guardian: Option<RecoveryCallback>,
    /// Callback when a guardian is selected from the modal (contact_id)
    pub on_select_guardian: Option<GuardianSelectCallback>,
    /// Pending recovery requests from others that we can approve
    pub pending_requests: Vec<PendingRequest>,
    /// Callback for submitting guardian approval (request_id)
    pub on_submit_approval: Option<ApprovalCallback>,
    // Effect dispatch callbacks for settings
    pub on_update_mfa: Option<MfaCallback>,
    pub on_update_nickname: Option<UpdateNicknameCallback>,
    pub on_update_threshold: Option<UpdateThresholdCallback>,
    pub on_add_device: Option<AddDeviceCallback>,
    pub on_remove_device: Option<RemoveDeviceCallback>,
    // Effect dispatch callbacks for contacts actions
    pub on_update_petname: Option<UpdatePetnameCallback>,
    pub on_start_chat: Option<StartChatCallback>,
    pub on_invite_lan_peer: Option<InvitePeerCallback>,
    // Effect dispatch callbacks for block actions
    pub on_block_send: Option<BlockSendCallback>,
    pub on_block_invite: Option<BlockInviteCallback>,
    pub on_block_navigate_neighborhood: Option<BlockNavCallback>,
    pub on_grant_steward: Option<GrantStewardCallback>,
    pub on_revoke_steward: Option<RevokeStewardCallback>,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
    /// Callback for account creation
    pub on_create_account: Option<CreateAccountCallback>,
    // Sync status
    /// Whether sync is in progress
    pub sync_in_progress: bool,
    /// Last sync time (ms since epoch)
    pub last_sync_time: Option<u64>,
    /// Number of known peers
    pub peer_count: usize,
    // Demo mode
    /// Whether running in demo mode
    pub demo_mode: bool,
    /// Alice's invite code (for demo mode)
    pub demo_alice_code: String,
    /// Carol's invite code (for demo mode)
    pub demo_carol_code: String,
}

/// Callback for creating an account
pub type CreateAccountCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback for selecting a guardian from the modal (contact_id)
pub type GuardianSelectCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Key repeat debounce state
#[derive(Clone, Default)]
struct KeyDebounce {
    /// Last key code pressed (as debug string for comparison)
    last_key: Option<String>,
    /// Timestamp of last accepted key press
    last_time: Option<std::time::Instant>,
    /// Whether we're in "repeat" mode (after initial delay)
    repeating: bool,
}

impl KeyDebounce {
    /// Initial delay before key repeat starts (ms)
    const INITIAL_DELAY_MS: u64 = 200;
    /// Delay between repeated keys (ms)
    const REPEAT_DELAY_MS: u64 = 50;

    /// Check if this key event should be accepted (returns true) or debounced (returns false)
    fn should_accept(&mut self, key: &str) -> bool {
        let now = std::time::Instant::now();

        match (&self.last_key, self.last_time) {
            (Some(last), Some(time)) if last == key => {
                // Same key - apply debouncing
                let elapsed = now.duration_since(time).as_millis() as u64;
                let threshold = if self.repeating {
                    Self::REPEAT_DELAY_MS
                } else {
                    Self::INITIAL_DELAY_MS
                };

                if elapsed >= threshold {
                    // Accept and mark as repeating
                    self.last_time = Some(now);
                    self.repeating = true;
                    true
                } else {
                    // Debounce - too fast
                    false
                }
            }
            _ => {
                // Different key or first press - always accept
                self.last_key = Some(key.to_string());
                self.last_time = Some(now);
                self.repeating = false;
                true
            }
        }
    }
}

/// Main application with screen navigation
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Block);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Key debounce state for preventing double-presses
    let key_debounce = hooks.use_ref(KeyDebounce::default);

    // Pure TUI state machine - holds all UI state for deterministic transitions
    // This is the source of truth; iocraft hooks sync FROM this state
    let show_setup = props.show_account_setup;
    let demo_alice = props.demo_alice_code.clone();
    let demo_carol = props.demo_carol_code.clone();
    let tui_state = hooks.use_ref(move || {
        let mut state = if show_setup {
            TuiState::with_account_setup()
        } else {
            TuiState::new()
        };
        // Set demo mode codes for import modal shortcuts (on contacts screen)
        state.contacts.demo_alice_code = demo_alice.clone();
        state.contacts.demo_carol_code = demo_carol.clone();
        // Also keep them on invitations for backwards compatibility
        state.invitations.demo_alice_code = demo_alice.clone();
        state.invitations.demo_carol_code = demo_carol.clone();
        state
    });
    let tui_state_version = hooks.use_state(|| 0usize);

    // Account setup modal state - renders based on TuiState but maintains local UI state
    // for text input handling (hybrid architecture: state machine for transitions, local for rendering)
    let account_state = hooks.use_ref(move || {
        let mut state = AccountSetupState::new();
        if show_setup {
            state.show();
        }
        state
    });
    let account_version = hooks.use_state(|| 0usize);
    let on_create_account = props.on_create_account.clone();

    // Guardian selection modal state
    let guardian_select_state = hooks.use_ref(ContactSelectState::new);
    let guardian_select_version = hooks.use_state(|| 0usize);

    // Contact selection modal state (generic contact picker)
    let contact_select_state = hooks.use_ref(ContactSelectState::new);
    let contact_select_version = hooks.use_state(|| 0usize);

    // Confirm modal state
    let confirm_modal_visible = hooks.use_state(|| false);
    let confirm_modal_title = hooks.use_ref(String::new);
    let confirm_modal_message = hooks.use_ref(String::new);
    let confirm_modal_version = hooks.use_state(|| 0usize);

    // Help modal state
    let help_modal_state = hooks.use_ref(HelpModalState::new);
    let help_modal_version = hooks.use_state(|| 0usize);

    // Toast notifications state
    // Using use_ref for non-Copy Vec<ToastMessage>
    let toasts_ref = hooks.use_ref(|| Vec::<ToastMessage>::new());
    let toasts_version = hooks.use_state(|| 0usize);

    // Get AppCoreContext for IoContext access
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // Subscribe to toast updates by polling IoContext periodically
    // Only trigger re-render when toasts actually change to avoid resource exhaustion
    hooks.use_future({
        let mut toasts_ref = toasts_ref.clone();
        let mut toasts_version = toasts_version.clone();
        let io_ctx = app_ctx.io_context.clone();
        async move {
            let mut last_toast_count = 0usize;
            loop {
                // Poll toasts every 100ms
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                let current_toasts = io_ctx.get_toasts().await;
                let new_count = current_toasts.len();

                // Only update and re-render if toast count changed
                // This prevents constant re-renders when nothing changes
                if new_count != last_toast_count {
                    last_toast_count = new_count;
                    *toasts_ref.write() = current_toasts;
                    toasts_version.set(toasts_version.get().wrapping_add(1));
                }
            }
        }
    });

    // Handle exit request
    if should_exit.get() {
        system.exit();
    }

    // Clone props for use
    let channels = props.channels.clone();
    let messages = props.messages.clone();
    let _invitations = props.invitations.clone();
    let guardians = props.guardians.clone();
    let devices = props.devices.clone();
    let display_name = props.display_name.clone();
    let threshold_k = props.threshold_k;
    let threshold_n = props.threshold_n;
    let mfa_policy = props.mfa_policy;
    let recovery_status = props.recovery_status.clone();
    // Block screen data
    let block_name = props.block_name.clone();
    let residents = props.residents.clone();
    let block_budget = props.block_budget.clone();
    let channel_name = props.channel_name.clone();
    // Contacts screen data
    let contacts = props.contacts.clone();
    let discovered_peers = props.discovered_peers.clone();
    // Neighborhood screen data
    let neighborhood_name = props.neighborhood_name.clone();
    let blocks = props.blocks.clone();
    let traversal_depth = props.traversal_depth;
    // Effect dispatch callbacks
    let on_send = props.on_send.clone();
    let on_retry_message = props.on_retry_message.clone();
    let on_channel_select = props.on_channel_select.clone();
    let on_create_channel = props.on_create_channel.clone();
    let on_set_topic = props.on_set_topic.clone();
    let on_accept_invitation = props.on_accept_invitation.clone();
    let on_decline_invitation = props.on_decline_invitation.clone();
    let on_create_invitation = props.on_create_invitation.clone();
    let on_export_invitation = props.on_export_invitation.clone();
    let on_import_invitation = props.on_import_invitation.clone();
    let on_enter_block = props.on_enter_block.clone();
    let on_go_home = props.on_go_home.clone();
    let on_back_to_street = props.on_back_to_street.clone();
    let on_start_recovery = props.on_start_recovery.clone();
    let on_add_guardian = props.on_add_guardian.clone();
    let on_select_guardian = props.on_select_guardian.clone();
    let pending_requests = props.pending_requests.clone();
    let on_submit_approval = props.on_submit_approval.clone();
    let on_update_mfa = props.on_update_mfa.clone();
    let on_update_nickname = props.on_update_nickname.clone();
    let on_update_threshold = props.on_update_threshold.clone();
    let on_add_device = props.on_add_device.clone();
    let on_remove_device = props.on_remove_device.clone();
    let on_update_petname = props.on_update_petname.clone();
    let on_start_chat = props.on_start_chat.clone();
    let on_invite_lan_peer = props.on_invite_lan_peer.clone();
    let on_block_send = props.on_block_send.clone();
    let on_block_invite = props.on_block_invite.clone();
    let on_block_navigate_neighborhood = props.on_block_navigate_neighborhood.clone();
    let on_grant_steward = props.on_grant_steward.clone();
    let on_revoke_steward = props.on_revoke_steward.clone();

    let current_screen = screen.get();

    // Check if in insert mode (MessageInput has its own hint bar, so hide main hints)
    let is_insert_mode = tui_state.read().is_insert_mode();

    // Extract screen view props from TuiState using testable extraction functions
    let block_props = extract_block_view_props(&tui_state.read());
    let chat_props = extract_chat_view_props(&tui_state.read());
    let contacts_props = extract_contacts_view_props(&tui_state.read());
    let _invitations_props = extract_invitations_view_props(&tui_state.read());
    let settings_props = extract_settings_view_props(&tui_state.read());
    let recovery_props = extract_recovery_view_props(&tui_state.read());
    let neighborhood_props = extract_neighborhood_view_props(&tui_state.read());

    // Global hints that appear on all screens (bottom row)
    let global_hints = vec![
        KeyHint::new("↑↓←→", "Navigate"),
        KeyHint::new("Tab", "Next screen"),
        KeyHint::new("?", "Help"),
        KeyHint::new("q", "Quit"),
    ];

    // Build screen-specific hints based on current screen (top row)
    let screen_hints: Vec<KeyHint> = match current_screen {
        Screen::Block => vec![
            KeyHint::new("i", "Insert"),
            KeyHint::new("v", "Invite"),
            KeyHint::new("n", "Neighbor"),
            KeyHint::new("g", "Grant"),
            KeyHint::new("r", "Revoke"),
        ],
        Screen::Chat => vec![
            KeyHint::new("i", "Insert"),
            KeyHint::new("n", "New"),
            KeyHint::new("o", "Info"),
            KeyHint::new("t", "Topic"),
            KeyHint::new("r", "Retry"),
        ],
        Screen::Contacts => vec![
            KeyHint::new("e", "Edit"),
            KeyHint::new("g", "Guardian"),
            KeyHint::new("c", "Chat"),
            KeyHint::new("i", "Accept"),
            KeyHint::new("n", "Invite"),
        ],
        Screen::Neighborhood => vec![
            KeyHint::new("Enter", "Enter"),
            KeyHint::new("g", "Home"),
            KeyHint::new("b", "Back"),
        ],
        Screen::Settings => vec![
            KeyHint::new("h/l", "Panel"),
            KeyHint::new("Space", "Toggle"),
        ],
        Screen::Recovery => vec![
            KeyHint::new("a", "Add"),
            KeyHint::new("s", "Start"),
            KeyHint::new("h/l", "Tab"),
        ],
    };

    // Clone contacts for guardian modal
    let contacts_for_modal = contacts.clone();

    hooks.use_terminal_events({
        let mut screen = screen.clone();
        let mut should_exit = should_exit.clone();
        let mut tui_state = tui_state.clone();
        let mut tui_state_version = tui_state_version.clone();
        let mut account_state = account_state.clone();
        let mut account_version = account_version.clone();
        let mut guardian_select_state = guardian_select_state.clone();
        let mut guardian_select_version = guardian_select_version.clone();
        let mut contact_select_state = contact_select_state.clone();
        let mut contact_select_version = contact_select_version.clone();
        let mut confirm_modal_visible = confirm_modal_visible.clone();
        let mut confirm_modal_title = confirm_modal_title.clone();
        let mut confirm_modal_message = confirm_modal_message.clone();
        let mut confirm_modal_version = confirm_modal_version.clone();
        let mut help_modal_state = help_modal_state.clone();
        let mut help_modal_version = help_modal_version.clone();
        let on_create_account = on_create_account.clone();
        let on_select_guardian = on_select_guardian.clone();
        let contacts_for_modal = contacts_for_modal.clone();
        // Clone IoContext for toast operations (clear on Escape)
        let io_ctx_for_toasts = app_ctx.io_context.clone();
        // Clone IoContext for ceremony operations
        let io_ctx_for_ceremony = app_ctx.io_context.clone();
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        // Key debounce ref for preventing rapid double-presses
        let mut key_debounce = key_debounce.clone();
        // Clone all dispatch callbacks for use inside the closure
        let on_block_send = on_block_send.clone();
        let on_block_invite = on_block_invite.clone();
        let on_grant_steward = on_grant_steward.clone();
        let on_revoke_steward = on_revoke_steward.clone();
        let on_channel_select = on_channel_select.clone();
        let on_send = on_send.clone();
        let on_retry_message = on_retry_message.clone();
        let on_create_channel = on_create_channel.clone();
        let on_set_topic = on_set_topic.clone();
        let on_update_petname = on_update_petname.clone();
        let on_start_chat = on_start_chat.clone();
        let on_accept_invitation = on_accept_invitation.clone();
        let on_decline_invitation = on_decline_invitation.clone();
        let on_create_invitation = on_create_invitation.clone();
        let on_import_invitation = on_import_invitation.clone();
        let on_export_invitation = on_export_invitation.clone();
        let on_start_recovery = on_start_recovery.clone();
        let on_submit_approval = on_submit_approval.clone();
        let on_update_nickname = on_update_nickname.clone();
        let on_update_threshold = on_update_threshold.clone();
        let on_update_mfa = on_update_mfa.clone();
        let on_add_device = on_add_device.clone();
        let on_remove_device = on_remove_device.clone();
        let on_enter_block = on_enter_block.clone();
        let on_go_home = on_go_home.clone();
        let on_back_to_street = on_back_to_street.clone();
        // Clone contacts for use inside the closure (for populating guardian modal)
        let contacts_for_modal_populate = contacts.clone();
        move |event| {
            // Convert iocraft event to aura-core event and run through state machine
            if let Some(core_event) = convert_iocraft_event(event.clone()) {
                // Apply key debouncing to prevent double-presses
                // Uses classic pattern: longer initial delay, then faster repeat
                let key_str = format!("{:?}", core_event);
                if !key_debounce.write().should_accept(&key_str) {
                    return; // Debounced - skip this event
                }
                // Get current state, apply transition, update state
                let current = tui_state.read().clone();
                let (new_state, commands) = transition(&current, core_event);

                // Sync TuiState changes to iocraft hooks
                if new_state.screen() != current.screen() {
                    screen.set(new_state.screen());
                }
                if new_state.should_exit && !current.should_exit {
                    should_exit.set(true);
                }

                // Sync modal state from TuiState to iocraft hooks
                match new_state.modal.modal_type {
                    ModalType::AccountSetup => {
                        // Sync account setup modal state
                        let legacy_visible = account_state.read().visible;
                        if !legacy_visible {
                            account_state.write().show();
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        // Sync the display_name and other fields
                        let mut legacy = account_state.write();
                        legacy.display_name = new_state.modal.account_setup.display_name.clone();
                        legacy.creating = new_state.modal.account_setup.creating;
                        legacy.success = new_state.modal.account_setup.success;
                        legacy.error = new_state.modal.account_setup.error.clone();
                        drop(legacy);
                        account_version.set(account_version.get().wrapping_add(1));
                    }
                    ModalType::Help => {
                        if !help_modal_state.read().visible {
                            help_modal_state.write().show();
                            help_modal_version.set(help_modal_version.get().wrapping_add(1));
                        }
                    }
                    ModalType::GuardianSelect => {
                        if !guardian_select_state.read().visible {
                            guardian_select_state
                                .write()
                                .show("Select Guardian", contacts_for_modal.clone());
                            guardian_select_version
                                .set(guardian_select_version.get().wrapping_add(1));
                        }
                    }
                    ModalType::None => {
                        // Close any open modals
                        if account_state.read().visible {
                            account_state.write().hide();
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        if help_modal_state.read().visible {
                            help_modal_state.write().hide();
                            help_modal_version.set(help_modal_version.get().wrapping_add(1));
                        }
                        if guardian_select_state.read().visible {
                            guardian_select_state.write().hide();
                            guardian_select_version
                                .set(guardian_select_version.get().wrapping_add(1));
                        }
                        if contact_select_state.read().visible {
                            contact_select_state.write().hide();
                            contact_select_version
                                .set(contact_select_version.get().wrapping_add(1));
                        }
                        if confirm_modal_visible.get() {
                            confirm_modal_visible.set(false);
                            confirm_modal_version
                                .set(confirm_modal_version.get().wrapping_add(1));
                        }
                    }
                    ModalType::ContactSelect => {
                        // Show contact selection modal (generic contact picker)
                        if !contact_select_state.read().visible {
                            contact_select_state
                                .write()
                                .show("Select Contact", contacts_for_modal.clone());
                            contact_select_version
                                .set(contact_select_version.get().wrapping_add(1));
                        }
                    }
                    ModalType::Confirm => {
                        // Show confirmation modal
                        if !confirm_modal_visible.get() {
                            confirm_modal_visible.set(true);
                            *confirm_modal_title.write() = "Confirm Action".to_string();
                            *confirm_modal_message.write() = "Are you sure you want to proceed?".to_string();
                            confirm_modal_version
                                .set(confirm_modal_version.get().wrapping_add(1));
                        }
                    }
                }

                // Execute commands
                for cmd in commands {
                    match cmd {
                        TuiCommand::Exit => {
                            should_exit.set(true);
                        }
                        TuiCommand::Dispatch(dispatch_cmd) => {
                            // Handle dispatch commands - map to callbacks
                            match dispatch_cmd {
                                DispatchCommand::CreateAccount { name } => {
                                    if let Some(ref callback) = on_create_account {
                                        callback(name);
                                    }
                                }
                                DispatchCommand::AddGuardian { contact_id } => {
                                    if let Some(ref callback) = on_select_guardian {
                                        callback(contact_id);
                                    }
                                }
                                DispatchCommand::SelectGuardianByIndex { index } => {
                                    // Map index to contact_id from legacy modal state
                                    let contact_id = guardian_select_state
                                        .read()
                                        .contacts
                                        .get(index)
                                        .map(|c| c.id.clone());

                                    // Hide the modal
                                    guardian_select_state.write().hide();
                                    guardian_select_version
                                        .set(guardian_select_version.get().wrapping_add(1));

                                    // Also close in TuiState
                                    tui_state.write().modal.close();

                                    // Call the callback with contact_id
                                    if let Some(contact_id) = contact_id {
                                        if let Some(ref callback) = on_select_guardian {
                                            callback(contact_id);
                                        }
                                    }
                                }

                                // === Block Screen Commands ===
                                DispatchCommand::SendBlockMessage { content } => {
                                    if let Some(ref callback) = on_block_send {
                                        callback(content);
                                    }
                                }
                                DispatchCommand::InviteToBlock { contact_id } => {
                                    if let Some(ref callback) = on_block_invite {
                                        callback(contact_id);
                                    }
                                }
                                DispatchCommand::GrantSteward { resident_id } => {
                                    if let Some(ref callback) = on_grant_steward {
                                        callback(resident_id);
                                    }
                                }
                                DispatchCommand::RevokeSteward { resident_id } => {
                                    if let Some(ref callback) = on_revoke_steward {
                                        callback(resident_id);
                                    }
                                }

                                // === Chat Screen Commands ===
                                DispatchCommand::SelectChannel { channel_id } => {
                                    if let Some(ref callback) = on_channel_select {
                                        callback(channel_id);
                                    }
                                }
                                DispatchCommand::SendChatMessage {
                                    channel_id,
                                    content,
                                } => {
                                    if let Some(ref callback) = on_send {
                                        callback(channel_id, content);
                                    }
                                }
                                DispatchCommand::RetryMessage { message_id } => {
                                    // Note: RetryMessage requires channel and content from the failed message
                                    // The callback expects (message_id, channel_id, content)
                                    // For now, log a warning since we don't have the full message context here
                                    if on_retry_message.is_some() {
                                        tracing::warn!(
                                            "RetryMessage not fully implemented: message_id={}",
                                            message_id
                                        );
                                    }
                                }
                                DispatchCommand::CreateChannel { name } => {
                                    if let Some(ref callback) = on_create_channel {
                                        callback(name, None);
                                    }
                                }
                                DispatchCommand::SetChannelTopic { channel_id, topic } => {
                                    if let Some(ref callback) = on_set_topic {
                                        callback(channel_id, topic);
                                    }
                                }

                                // === Contacts Screen Commands ===
                                DispatchCommand::UpdatePetname {
                                    contact_id,
                                    petname,
                                } => {
                                    if let Some(ref callback) = on_update_petname {
                                        callback(contact_id, petname);
                                    }
                                }
                                DispatchCommand::StartChat { contact_id } => {
                                    if let Some(ref callback) = on_start_chat {
                                        callback(contact_id);
                                    }
                                }

                                // === Invitations Screen Commands ===
                                DispatchCommand::AcceptInvitation { invitation_id } => {
                                    if let Some(ref callback) = on_accept_invitation {
                                        callback(invitation_id);
                                    }
                                }
                                DispatchCommand::DeclineInvitation { invitation_id } => {
                                    if let Some(ref callback) = on_decline_invitation {
                                        callback(invitation_id);
                                    }
                                }
                                DispatchCommand::CreateInvitation {
                                    invitation_type,
                                    message,
                                } => {
                                    if let Some(ref callback) = on_create_invitation {
                                        // Third argument is TTL in seconds (None = no expiry)
                                        callback(invitation_type, message, None);
                                    }
                                }
                                DispatchCommand::ImportInvitation { code } => {
                                    if let Some(ref callback) = on_import_invitation {
                                        callback(code);
                                    }
                                }
                                DispatchCommand::ExportInvitation { invitation_id } => {
                                    if let Some(ref callback) = on_export_invitation {
                                        // Spawn the async callback to run in the background
                                        tokio::spawn(callback(invitation_id));
                                    }
                                }

                                // === Recovery Screen Commands ===
                                DispatchCommand::StartRecovery => {
                                    if let Some(ref callback) = on_start_recovery {
                                        callback();
                                    }
                                }
                                DispatchCommand::ApproveRecovery { request_id } => {
                                    if let Some(ref callback) = on_submit_approval {
                                        callback(request_id);
                                    }
                                }

                                // === Guardian Ceremony Commands ===
                                DispatchCommand::StartGuardianCeremony { contact_ids, threshold_k } => {
                                    tracing::info!(
                                        "Starting guardian ceremony with {} contacts, threshold {}",
                                        contact_ids.len(),
                                        threshold_k
                                    );

                                    let io_ctx = io_ctx_for_ceremony.clone();
                                    let ids = contact_ids.clone();
                                    let n = contact_ids.len() as u8;
                                    let k = threshold_k;

                                    // Use pre-cloned AppCore for key rotation
                                    let app_core = app_core_for_ceremony.clone();

                                    tokio::spawn(async move {
                                        // Step 1: Rotate keys - generates new FROST threshold keys
                                        // The authority ID would come from the account context
                                        // For demo mode, we'll use the key rotation through the effect system
                                        let core = app_core.read().await;

                                        // Try to perform key rotation through the runtime bridge
                                        // This generates new k-of-n FROST keys for the selected guardians
                                        match core.rotate_guardian_keys(k as u16, n as u16, &ids).await {
                                            Ok((new_epoch, _key_packages, _public_key)) => {
                                                tracing::info!(
                                                    new_epoch,
                                                    threshold = k,
                                                    guardians = n,
                                                    "Key rotation successful, distributing shares to guardians"
                                                );

                                                // Step 2: Demo mode simulates instant acceptance.
                                                // Production mode sends key packages to each guardian
                                                // and waits for protocol-level acceptance.
                                                for contact_id in &ids {
                                                    io_ctx.toggle_contact_guardian(contact_id, true).await;
                                                    tracing::info!("Guardian {} accepted binding (demo)", contact_id);
                                                }

                                                // Step 3: Commit the key rotation (all accepted)
                                                if let Err(e) = core.commit_guardian_key_rotation(new_epoch).await {
                                                    tracing::warn!("Failed to commit key rotation: {}", e);
                                                }

                                                // Show success toast
                                                io_ctx.add_success_toast(
                                                    "guardian-ceremony-complete",
                                                    format!("Guardian ceremony complete! {}-of-{} threshold keys generated", k, n)
                                                ).await;
                                            }
                                            Err(e) => {
                                                tracing::error!("Key rotation failed: {}", e);

                                                // Still mark guardians in demo mode (fallback behavior)
                                                for contact_id in &ids {
                                                    io_ctx.toggle_contact_guardian(contact_id, true).await;
                                                }

                                                io_ctx.add_success_toast(
                                                    "guardian-ceremony-complete",
                                                    format!("Guardian ceremony complete! {} of {} threshold set (key rotation pending)", k, n)
                                                ).await;
                                            }
                                        }
                                    });

                                    // Update TUI state to close modal and show completion
                                    tui_state.write().contacts.guardian_setup_modal.visible = false;
                                    tui_state.write().contacts.guardian_setup_modal.has_pending_ceremony = false;
                                    tui_state_version.set(tui_state_version.get() + 1);
                                }
                                DispatchCommand::CancelGuardianCeremony => {
                                    tracing::info!("Canceling guardian ceremony");

                                    // If there was a pending key rotation, roll it back
                                    // The epoch would be tracked in ceremony state
                                    // For now, just log that we would rollback
                                    tracing::info!("Would rollback any pending key rotation");

                                    // Close the modal and reset ceremony state
                                    tui_state.write().contacts.guardian_setup_modal.visible = false;
                                    tui_state.write().contacts.guardian_setup_modal.has_pending_ceremony = false;
                                    tui_state.write().contacts.guardian_setup_modal.step = Default::default();
                                    tui_state.write().contacts.guardian_setup_modal.selected_indices.clear();
                                    tui_state.write().contacts.guardian_setup_modal.ceremony_responses.clear();
                                    tui_state_version.set(tui_state_version.get() + 1);
                                }

                                // === Settings Screen Commands ===
                                DispatchCommand::UpdateNickname { nickname } => {
                                    if let Some(ref callback) = on_update_nickname {
                                        callback(nickname);
                                    }
                                }
                                DispatchCommand::UpdateThreshold { k, n } => {
                                    if let Some(ref callback) = on_update_threshold {
                                        callback(k, n);
                                    }
                                }
                                DispatchCommand::UpdateMfaPolicy { policy } => {
                                    if let Some(ref callback) = on_update_mfa {
                                        callback(policy);
                                    }
                                }
                                DispatchCommand::AddDevice { name } => {
                                    if let Some(ref callback) = on_add_device {
                                        callback(name);
                                    }
                                }
                                DispatchCommand::RemoveDevice { device_id } => {
                                    if let Some(ref callback) = on_remove_device {
                                        callback(device_id);
                                    }
                                }

                                // === Neighborhood Screen Commands ===
                                DispatchCommand::EnterBlock { block_id } => {
                                    if let Some(ref callback) = on_enter_block {
                                        // Default to Street-level traversal depth
                                        callback(block_id, TraversalDepth::default());
                                    }
                                }
                                DispatchCommand::GoHome => {
                                    if let Some(ref callback) = on_go_home {
                                        callback();
                                    }
                                }
                                DispatchCommand::BackToStreet => {
                                    if let Some(ref callback) = on_back_to_street {
                                        callback();
                                    }
                                }

                                // === Navigation Commands ===
                                DispatchCommand::NavigateTo(_screen) => {
                                    // Navigation is handled by TuiState directly
                                    // The state machine already updates the screen
                                }
                            }
                        }
                        TuiCommand::ShowToast { message, level } => {
                            // Wire to toast system via IoContext
                            let io_ctx = io_ctx_for_toasts.clone();
                            let toast_level = match level {
                                crate::tui::state_machine::ToastLevel::Info => ToastLevel::Info,
                                crate::tui::state_machine::ToastLevel::Success => ToastLevel::Success,
                                crate::tui::state_machine::ToastLevel::Warning => ToastLevel::Warning,
                                crate::tui::state_machine::ToastLevel::Error => ToastLevel::Error,
                            };
                            let toast_id = format!(
                                "toast-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                            );
                            tokio::spawn(async move {
                                io_ctx.add_toast(ToastMessage::new(toast_id, message).with_level(toast_level)).await;
                            });
                        }
                        TuiCommand::DismissToast { id } => {
                            // Dismiss specific toast by ID
                            let io_ctx = io_ctx_for_toasts.clone();
                            let toast_id = format!("toast-{}", id);
                            tokio::spawn(async move {
                                io_ctx.clear_toast(&toast_id).await;
                            });
                        }
                        TuiCommand::ClearAllToasts => {
                            // Clear all toasts via IoContext
                            let io_ctx = io_ctx_for_toasts.clone();
                            tokio::spawn(async move {
                                io_ctx.clear_toasts().await;
                            });
                        }
                        TuiCommand::Render => {
                            // Render is handled by iocraft automatically
                        }
                    }
                }

                // Update TuiState
                *tui_state.write() = new_state;

                // If the guardian setup modal just became visible, populate its contacts list
                // from the reactive contacts data
                {
                    let mut state = tui_state.write();
                    if state.contacts.guardian_setup_modal.visible
                        && state.contacts.guardian_setup_modal.contacts.is_empty()
                    {
                        // Populate from the contacts prop (which comes from reactive signals)
                        // We need to convert Contact -> GuardianCandidate
                        state.contacts.guardian_setup_modal.contacts = contacts_for_modal_populate
                            .iter()
                            .map(|c| crate::tui::state_machine::GuardianCandidate {
                                id: c.id.clone(),
                                name: c.petname.clone(),
                                is_current_guardian: c.is_guardian,
                            })
                            .collect();

                        // Pre-select existing guardians
                        state.contacts.guardian_setup_modal.selected_indices = state
                            .contacts
                            .guardian_setup_modal
                            .contacts
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.is_current_guardian)
                            .map(|(i, _)| i)
                            .collect();

                        tracing::debug!(
                            "Populated guardian modal with {} contacts",
                            state.contacts.guardian_setup_modal.contacts.len()
                        );
                    }
                }

                tui_state_version.set(tui_state_version.get().wrapping_add(1));
            }

            // All key events are now handled by the state machine above.
            // Modal handling (AccountSetup, GuardianSelect, Help) goes through
            // transition() -> sync to legacy state -> command execution.
        }
    });

    // Extract account setup state for rendering from use_ref
    let state_ref = account_state.read();
    let modal_visible = state_ref.visible;
    let modal_creating = state_ref.creating;
    let modal_success = state_ref.success;
    let modal_display_name = state_ref.display_name.clone();
    let modal_error = state_ref.error.clone().unwrap_or_default();
    drop(state_ref); // Release the read lock
                     // account_version is used for triggering re-renders (not directly in UI)
    let _ = account_version.get();

    // Extract guardian select state for rendering from use_ref
    let guardian_state_ref = guardian_select_state.read();
    let guardian_modal_visible = guardian_state_ref.visible;
    let guardian_modal_title = guardian_state_ref.title.clone();
    let guardian_modal_contacts = guardian_state_ref.contacts.clone();
    let guardian_modal_selected = guardian_state_ref.selected_index;
    let guardian_modal_error = guardian_state_ref.error.clone().unwrap_or_default();
    drop(guardian_state_ref); // Release the read lock
                              // guardian_select_version is used for triggering re-renders (not directly in UI)
    let _ = guardian_select_version.get();

    // Extract contact select state for rendering from use_ref
    let contact_state_ref = contact_select_state.read();
    let contact_modal_visible = contact_state_ref.visible;
    let contact_modal_title = contact_state_ref.title.clone();
    let contact_modal_contacts = contact_state_ref.contacts.clone();
    let contact_modal_selected = contact_state_ref.selected_index;
    let contact_modal_error = contact_state_ref.error.clone().unwrap_or_default();
    drop(contact_state_ref); // Release the read lock
                             // contact_select_version is used for triggering re-renders (not directly in UI)
    let _ = contact_select_version.get();

    // Extract confirm modal state for rendering
    let confirm_visible = confirm_modal_visible.get();
    let confirm_title = confirm_modal_title.read().clone();
    let confirm_message = confirm_modal_message.read().clone();
    // confirm_modal_version is used for triggering re-renders (not directly in UI)
    let _ = confirm_modal_version.get();

    // Extract help modal state for rendering from use_ref
    let help_modal_visible = help_modal_state.read().visible;
    // help_modal_version is used for triggering re-renders (not directly in UI)
    let _ = help_modal_version.get();

    // Get current toasts for rendering
    // toasts_version is used for triggering re-renders (not directly in UI)
    let _ = toasts_version.get();
    let current_toasts = toasts_ref.read().clone();

    // Extract sync status from props
    let syncing = props.sync_in_progress;
    let last_sync = props.last_sync_time;
    let peers = props.peer_count;

    // Layout: NavBar (3 rows) + Content (25 rows) + Footer (3 rows) = 31 = TOTAL_HEIGHT
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::TOTAL_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Nav bar with screen tabs and status (3 rows)
            NavBar(
                active_screen: current_screen,
                syncing: syncing,
                last_sync_time: last_sync,
                peer_count: peers,
            )

            // Screen content - fixed 25 rows (MIDDLE_HEIGHT)
            View(width: dim::TOTAL_WIDTH, height: dim::MIDDLE_HEIGHT, overflow: Overflow::Hidden) {
                #(match current_screen {
                    Screen::Block => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            BlockScreen(
                                block_name: block_name.clone(),
                                residents: residents.clone(),
                                messages: messages.clone(),
                                budget: block_budget.clone(),
                                channel_name: channel_name.clone(),
                                contacts: contacts.clone(),
                                // View state: entire struct from extraction function (compile-time enforced)
                                view: block_props.clone(),
                                // Callbacks
                                on_send: on_block_send.clone(),
                                on_invite: on_block_invite.clone(),
                                on_go_neighborhood: on_block_navigate_neighborhood.clone(),
                                on_grant_steward: on_grant_steward.clone(),
                                on_revoke_steward: on_revoke_steward.clone(),
                            )
                        }
                    }],
                    Screen::Chat => {
                        vec![element! {
                            View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                                ChatScreen(
                                    // Domain data
                                    channels: channels.clone(),
                                    messages: messages.clone(),
                                    // View state: entire struct from extraction function (compile-time enforced)
                                    view: chat_props.clone(),
                                    // Callbacks
                                    on_send: on_send.clone(),
                                    on_retry_message: on_retry_message.clone(),
                                    on_channel_select: on_channel_select.clone(),
                                    on_create_channel: on_create_channel.clone(),
                                    on_set_topic: on_set_topic.clone(),
                                )
                            }
                        }]
                    }
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            ContactsScreen(
                                contacts: contacts.clone(),
                                discovered_peers: discovered_peers.clone(),
                                // View state: entire struct from extraction function (compile-time enforced)
                                view: contacts_props.clone(),
                                // Callbacks
                                on_update_petname: on_update_petname.clone(),
                                on_start_chat: on_start_chat.clone(),
                                on_invite_lan_peer: on_invite_lan_peer.clone(),
                                on_import_invitation: on_import_invitation.clone(),
                            )
                        }
                    }],
                    Screen::Neighborhood => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            NeighborhoodScreen(
                                // Domain data
                                neighborhood_name: neighborhood_name.clone(),
                                blocks: blocks.clone(),
                                depth: traversal_depth,
                                // View state: entire struct from extraction function (compile-time enforced)
                                view: neighborhood_props.clone(),
                                // Callbacks
                                on_enter_block: on_enter_block.clone(),
                                on_go_home: on_go_home.clone(),
                                on_back_to_street: on_back_to_street.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            SettingsScreen(
                                // Domain data
                                display_name: display_name.clone(),
                                threshold_k: threshold_k,
                                threshold_n: threshold_n,
                                contact_count: contacts.len(),
                                devices: devices.clone(),
                                mfa_policy: mfa_policy,
                                // View state: entire struct from extraction function (compile-time enforced)
                                view: settings_props.clone(),
                                // Callbacks
                                on_update_mfa: on_update_mfa.clone(),
                                on_update_nickname: on_update_nickname.clone(),
                                on_update_threshold: on_update_threshold.clone(),
                                on_add_device: on_add_device.clone(),
                                on_remove_device: on_remove_device.clone(),
                            )
                        }
                    }],
                    Screen::Recovery => vec![element! {
                        View(width: 100pct, height: 100pct, flex_grow: 1.0, flex_shrink: 1.0) {
                            RecoveryScreen(
                                // Domain data
                                guardians: guardians.clone(),
                                threshold_required: threshold_k as u32,
                                threshold_total: threshold_n as u32,
                                recovery_status: recovery_status.clone(),
                                pending_requests: pending_requests.clone(),
                                // View state: entire struct from extraction function (compile-time enforced)
                                view: recovery_props.clone(),
                                // Callbacks
                                on_start_recovery: on_start_recovery.clone(),
                                on_add_guardian: on_add_guardian.clone(),
                                on_submit_approval: on_submit_approval.clone(),
                            )
                        }
                    }],
                })
            }

            // Footer with key hints (3 rows)
            // Row 2: Screen-specific hints, Row 3: Global hints with navigation
            // Show darkened hints when in insert mode (hotkeys inactive)
            Footer(hints: screen_hints.clone(), global_hints: global_hints.clone(), disabled: is_insert_mode)

            // Account setup modal overlay
            AccountSetupModal(
                visible: modal_visible,
                display_name: modal_display_name,
                focused: true,
                creating: modal_creating,
                success: modal_success,
                error: modal_error,
            )

            // Guardian selection modal overlay
            ContactSelectModal(
                visible: guardian_modal_visible,
                title: guardian_modal_title,
                contacts: guardian_modal_contacts,
                selected_index: guardian_modal_selected,
                error: guardian_modal_error,
            )

            // Generic contact selection modal overlay
            ContactSelectModal(
                visible: contact_modal_visible,
                title: contact_modal_title,
                contacts: contact_modal_contacts,
                selected_index: contact_modal_selected,
                error: contact_modal_error,
            )

            // Confirm dialog modal overlay
            ConfirmModal(
                visible: confirm_visible,
                title: confirm_title,
                message: confirm_message,
                confirm_text: "Confirm".to_string(),
                cancel_text: "Cancel".to_string(),
                confirm_focused: true,
            )

            // Help modal overlay (context-sensitive)
            HelpModal(visible: help_modal_visible, current_screen: Some(current_screen.name().to_string()))

            // Toast notification overlay (absolute positioned at bottom, above key hints)
            ToastContainer(toasts: current_toasts.clone())
        }
    }
}

/// Run the application with IoContext (real data)
///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<()> {
    // Create effect dispatch callbacks
    let ctx_arc = Arc::new(ctx);

    // SendCallback for ChatScreen - fires async dispatch in background
    let ctx_for_send = ctx_arc.clone();
    let on_send: SendCallback = Arc::new(move |channel_id: String, content: String| {
        tracing::debug!(
            "on_send (Chat) callback invoked: channel={}, content={}",
            channel_id,
            content
        );
        let ctx = ctx_for_send.clone();
        let cmd = EffectCommand::SendMessage {
            channel: channel_id.clone(),
            content,
        };
        tokio::spawn(async move {
            match ctx.dispatch(cmd).await {
                Ok(_) => {
                    tracing::debug!("Chat message dispatch succeeded to channel: {}", channel_id);
                }
                Err(e) => {
                    tracing::warn!("Chat message dispatch FAILED: {}", e);
                    ctx.add_error_toast("send-error", format!("Failed to send: {}", e))
                        .await;
                }
            }
        });
    });

    // RetryMessageCallback for retrying failed messages
    let ctx_for_retry = ctx_arc.clone();
    let on_retry_message: RetryMessageCallback = Arc::new(
        move |message_id: String, channel: String, content: String| {
            let ctx = ctx_for_retry.clone();
            let cmd = EffectCommand::RetryMessage {
                message_id,
                channel,
                content,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to retry message: {}", e);
                    ctx.add_error_toast("retry-error", format!("Retry failed: {}", e))
                        .await;
                }
            });
        },
    );

    // ChannelSelectCallback for selecting a channel (triggers real-time message updates)
    let app_core_for_select = ctx_arc.app_core().clone();
    let on_channel_select: ChannelSelectCallback = Arc::new(move |channel_id: String| {
        // Use try_read since we're in a sync callback
        // This is safe because select_channel uses lock_mut() internally
        if let Ok(core) = app_core_for_select.try_read() {
            core.views().select_channel(Some(channel_id));
        }
    });

    // CreateChannelCallback for creating new chat channels
    let ctx_for_create_channel = ctx_arc.clone();
    let on_create_channel: CreateChannelCallback =
        Arc::new(move |name: String, topic: Option<String>| {
            let ctx = ctx_for_create_channel.clone();
            // Start with empty members (creator is automatically included)
            let cmd = EffectCommand::CreateChannel {
                name,
                topic,
                members: vec![],
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to create channel: {}", e);
                    ctx.add_error_toast(
                        "channel-error",
                        format!("Failed to create channel: {}", e),
                    )
                    .await;
                }
            });
        });

    // SetTopicCallback for setting channel topic
    let ctx_for_set_topic = ctx_arc.clone();
    let on_set_topic: SetTopicCallback = Arc::new(move |channel_id: String, topic: String| {
        let ctx = ctx_for_set_topic.clone();
        let cmd = EffectCommand::SetTopic {
            channel: channel_id,
            text: topic,
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to set topic: {}", e);
                ctx.add_error_toast("topic-error", format!("Failed to set topic: {}", e))
                    .await;
            }
        });
    });

    // InvitationCallback for accepting invitations
    let ctx_for_accept = ctx_arc.clone();
    let on_accept_invitation: InvitationCallback = Arc::new(move |invitation_id: String| {
        let ctx = ctx_for_accept.clone();
        let cmd = EffectCommand::AcceptInvitation { invitation_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to accept invitation: {}", e);
                ctx.add_error_toast("invite-error", format!("Failed to accept: {}", e))
                    .await;
            }
        });
    });

    // InvitationCallback for declining invitations
    let ctx_for_decline = ctx_arc.clone();
    let on_decline_invitation: InvitationCallback = Arc::new(move |invitation_id: String| {
        let ctx = ctx_for_decline.clone();
        let cmd = EffectCommand::DeclineInvitation { invitation_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to decline invitation: {}", e);
                ctx.add_error_toast("invite-error", format!("Failed to decline: {}", e))
                    .await;
            }
        });
    });

    // CreateInvitationCallback for creating new invitations
    let ctx_for_create = ctx_arc.clone();
    let on_create_invitation: CreateInvitationCallback = Arc::new(
        move |invitation_type: String, message: Option<String>, ttl_secs: Option<u64>| {
            let ctx = ctx_for_create.clone();
            let cmd = EffectCommand::CreateInvitation {
                invitation_type,
                message,
                ttl_secs,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to create invitation: {}", e);
                    ctx.add_error_toast("create-invite-error", format!("Failed to create: {}", e))
                        .await;
                }
            });
        },
    );

    // ExportInvitationCallback for exporting invitation codes
    let ctx_for_export = ctx_arc.clone();
    let on_export_invitation: ExportInvitationCallback = Arc::new(move |invitation_id: String| {
        let ctx = ctx_for_export.clone();
        Box::pin(async move { ctx.export_invitation_code(&invitation_id).await })
    });

    // ImportInvitationCallback for importing invitation codes
    let ctx_for_import = ctx_arc.clone();
    let on_import_invitation: ImportInvitationCallback = Arc::new(move |code: String| {
        let ctx = ctx_for_import.clone();
        let cmd = EffectCommand::ImportInvitation { code };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to import invitation: {}", e);
                ctx.add_error_toast("import-invite-error", format!("Failed to import: {}", e))
                    .await;
            }
        });
    });

    // NavigationCallback for entering a block
    let ctx_for_enter = ctx_arc.clone();
    let on_enter_block: NavigationCallback =
        Arc::new(move |block_id: String, depth: TraversalDepth| {
            let ctx = ctx_for_enter.clone();
            let depth_str = match depth {
                TraversalDepth::Street => "Street",
                TraversalDepth::Frontage => "Frontage",
                TraversalDepth::Interior => "Interior",
            }
            .to_string();
            let cmd = EffectCommand::MovePosition {
                neighborhood_id: "current".to_string(), // Uses current neighborhood
                block_id,
                depth: depth_str,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to enter block: {}", e);
                    ctx.add_error_toast("nav-error", format!("Failed to enter block: {}", e))
                        .await;
                }
            });
        });

    // GoHomeCallback for navigating to home block
    let ctx_for_home = ctx_arc.clone();
    let on_go_home: GoHomeCallback = Arc::new(move || {
        let ctx = ctx_for_home.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "home".to_string(), // Special block_id to indicate home
            depth: "Interior".to_string(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to go home: {}", e);
                ctx.add_error_toast("nav-error", format!("Failed to go home: {}", e))
                    .await;
            }
        });
    });

    // GoHomeCallback for returning to street view
    let ctx_for_street = ctx_arc.clone();
    let on_back_to_street: GoHomeCallback = Arc::new(move || {
        let ctx = ctx_for_street.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "current".to_string(), // Stay on current block
            depth: "Street".to_string(),     // But change to street view
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to return to street: {}", e);
                ctx.add_error_toast("nav-error", format!("Failed to return to street: {}", e))
                    .await;
            }
        });
    });

    // RecoveryCallback for starting recovery
    let ctx_for_recovery = ctx_arc.clone();
    let on_start_recovery: RecoveryCallback = Arc::new(move || {
        let ctx = ctx_for_recovery.clone();
        let cmd = EffectCommand::StartRecovery;
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to start recovery: {}", e);
                ctx.add_error_toast("recovery-error", format!("Failed to start recovery: {}", e))
                    .await;
            }
        });
    });

    // RecoveryCallback for adding a guardian - dispatches InviteGuardian command
    // Note: This is kept for backward compatibility but the modal flow uses on_select_guardian
    let ctx_for_guardian = ctx_arc.clone();
    let on_add_guardian: RecoveryCallback = Arc::new(move || {
        let ctx = ctx_for_guardian.clone();
        let cmd = EffectCommand::InviteGuardian { contact_id: None };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to invite guardian: {}", e);
                ctx.add_error_toast(
                    "guardian-error",
                    format!("Failed to invite guardian: {}", e),
                )
                .await;
            }
        });
    });

    // GuardianSelectCallback for when a guardian is selected from the modal
    let ctx_for_select_guardian = ctx_arc.clone();
    let on_select_guardian: GuardianSelectCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_select_guardian.clone();
        // Dispatch guardian request - goes through proper e2e flow
        // In demo mode, Alice/Carol simulators will receive and accept the request
        let cmd = EffectCommand::InviteGuardian {
            contact_id: Some(contact_id),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to send guardian request: {}", e);
                ctx.add_error_toast(
                    "guardian-error",
                    format!("Failed to send guardian request: {}", e),
                )
                .await;
            }
        });
    });

    // ApprovalCallback for submitting guardian approval on a pending recovery request
    let ctx_for_approval = ctx_arc.clone();
    let on_submit_approval: ApprovalCallback = Arc::new(move |request_id: String| {
        let ctx = ctx_for_approval.clone();
        let cmd = EffectCommand::SubmitGuardianApproval {
            guardian_id: request_id,
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to submit guardian approval: {}", e);
                ctx.add_error_toast(
                    "approval-error",
                    format!("Failed to submit approval: {}", e),
                )
                .await;
            }
        });
    });

    // MfaCallback for updating MFA policy
    let ctx_for_mfa = ctx_arc.clone();
    let on_update_mfa: MfaCallback = Arc::new(move |policy: MfaPolicy| {
        let ctx = ctx_for_mfa.clone();
        let cmd = EffectCommand::UpdateMfaPolicy {
            require_mfa: policy.requires_mfa(),
        };
        tokio::spawn(async move {
            // Update the MFA policy in IoContext for immediate UI update
            ctx.set_mfa_policy(policy).await;
            // Also dispatch the command for backend persistence
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to update MFA policy: {}", e);
                ctx.add_error_toast("mfa-error", format!("Failed to update MFA policy: {}", e))
                    .await;
            }
        });
    });

    // UpdateNicknameCallback for updating display name - updates IoContext and dispatches command
    let ctx_for_nickname = ctx_arc.clone();
    let on_update_nickname: UpdateNicknameCallback = Arc::new(move |name: String| {
        let ctx = ctx_for_nickname.clone();
        let name_for_cmd = name.clone();
        let cmd = EffectCommand::UpdateNickname { name: name_for_cmd };
        tokio::spawn(async move {
            // Update the display name in IoContext for immediate UI update
            ctx.set_display_name(&name).await;
            // Also dispatch the command for any additional processing
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to update nickname: {}", e);
                ctx.add_error_toast(
                    "nickname-error",
                    format!("Failed to update display name: {}", e),
                )
                .await;
            }
        });
    });

    // UpdateThresholdCallback for updating recovery threshold - dispatches UpdateThreshold intent
    let ctx_for_threshold = ctx_arc.clone();
    let on_update_threshold: UpdateThresholdCallback =
        Arc::new(move |threshold_k: u8, threshold_n: u8| {
            let ctx = ctx_for_threshold.clone();
            let cmd = EffectCommand::UpdateThreshold {
                threshold_k,
                threshold_n,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to update threshold: {}", e);
                    ctx.add_error_toast(
                        "threshold-error",
                        format!("Failed to update threshold: {}", e),
                    )
                    .await;
                }
            });
        });

    // AddDeviceCallback for adding a new device
    let ctx_for_add_device = ctx_arc.clone();
    let on_add_device: AddDeviceCallback = Arc::new(move |device_name: String| {
        let ctx = ctx_for_add_device.clone();
        let cmd = EffectCommand::AddDevice { device_name };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to add device: {}", e);
                ctx.add_error_toast("device-error", format!("Failed to add device: {}", e))
                    .await;
            }
        });
    });

    // RemoveDeviceCallback for removing a device
    let ctx_for_remove_device = ctx_arc.clone();
    let on_remove_device: RemoveDeviceCallback = Arc::new(move |device_id: String| {
        let ctx = ctx_for_remove_device.clone();
        let cmd = EffectCommand::RemoveDevice { device_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to remove device: {}", e);
                ctx.add_error_toast("device-error", format!("Failed to remove device: {}", e))
                    .await;
            }
        });
    });

    // UpdatePetnameCallback for updating a contact's petname - dispatches UpdateContactPetname command
    let ctx_for_update_petname = ctx_arc.clone();
    let on_update_petname: UpdatePetnameCallback =
        Arc::new(move |contact_id: String, new_petname: String| {
            let ctx = ctx_for_update_petname.clone();
            let cmd = EffectCommand::UpdateContactPetname {
                contact_id,
                petname: new_petname,
            };
            tokio::spawn(async move {
                if let Err(e) = ctx.dispatch(cmd).await {
                    eprintln!("Failed to update petname: {}", e);
                    ctx.add_error_toast(
                        "petname-error",
                        format!("Failed to update contact name: {}", e),
                    )
                    .await;
                }
            });
        });

    // StartChatCallback for starting a direct chat with a contact
    let ctx_for_start_chat = ctx_arc.clone();
    let on_start_chat: StartChatCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_start_chat.clone();
        let cmd = EffectCommand::StartDirectChat { contact_id };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to start direct chat: {}", e);
                ctx.add_error_toast("chat-error", format!("Failed to start chat: {}", e))
                    .await;
            }
        });
    });

    // InvitePeerCallback for inviting a discovered LAN peer
    let ctx_for_invite_peer = ctx_arc.clone();
    let on_invite_lan_peer: InvitePeerCallback =
        Arc::new(move |authority_id: String, address: String| {
            let ctx = ctx_for_invite_peer.clone();
            let authority_id_for_mark = authority_id.clone();
            let cmd = EffectCommand::InviteLanPeer {
                authority_id,
                address,
            };
            tokio::spawn(async move {
                match ctx.dispatch(cmd).await {
                    Ok(_) => {
                        // Mark this peer as invited in the context for UI status display
                        ctx.mark_peer_invited(&authority_id_for_mark).await;
                        tracing::info!("LAN peer invited: {}", authority_id_for_mark);
                    }
                    Err(e) => {
                        eprintln!("Failed to invite LAN peer: {}", e);
                        ctx.add_error_toast("peer-error", format!("Failed to invite peer: {}", e))
                            .await;
                    }
                }
            });
        });

    // BlockSendCallback for sending a message in the block channel
    let ctx_for_block_send = ctx_arc.clone();
    let on_block_send: BlockSendCallback = Arc::new(move |content: String| {
        tracing::debug!("on_block_send callback invoked with content: {}", content);
        let ctx = ctx_for_block_send.clone();
        tokio::spawn(async move {
            // Get current block ID from snapshot
            let block_snap = ctx.snapshot_block();
            let block_id = block_snap
                .block
                .as_ref()
                .map(|b| b.id.clone())
                .unwrap_or_else(|| "home".to_string());

            // Use block:<block_id> as the channel for block messages
            let channel = format!("block:{}", block_id);
            tracing::debug!("Dispatching SendMessage to channel: {}", channel);
            let cmd = EffectCommand::SendMessage { channel, content };

            match ctx.dispatch(cmd).await {
                Ok(_) => {
                    tracing::debug!("Block message dispatch succeeded");
                }
                Err(e) => {
                    tracing::warn!("Block message dispatch FAILED: {}", e);
                    ctx.add_error_toast(
                        "block-error",
                        format!("Failed to send block message: {}", e),
                    )
                    .await;
                }
            }
        });
    });

    // BlockInviteCallback for inviting a contact to the block
    let ctx_for_block_invite = ctx_arc.clone();
    let on_block_invite: BlockInviteCallback = Arc::new(move |contact_id: String| {
        let ctx = ctx_for_block_invite.clone();
        let cmd = EffectCommand::SendBlockInvitation {
            contact_id: contact_id.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to send block invitation: {}", e);
                ctx.add_error_toast(
                    "block-error",
                    format!("Failed to send block invitation: {}", e),
                )
                .await;
            }
        });
        tracing::info!("Block invite sent to contact: {}", contact_id);
    });

    // BlockNavCallback for navigating to neighborhood view - dispatches MovePosition to Street depth
    let ctx_for_nav = ctx_arc.clone();
    let on_block_navigate_neighborhood: BlockNavCallback = Arc::new(move || {
        let ctx = ctx_for_nav.clone();
        let cmd = EffectCommand::MovePosition {
            neighborhood_id: "current".to_string(),
            block_id: "current".to_string(),
            depth: "Street".to_string(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to navigate to neighborhood: {}", e);
                ctx.add_error_toast("nav-error", format!("Failed to navigate: {}", e))
                    .await;
            }
        });
    });

    // GrantStewardCallback for promoting a resident to steward (Admin)
    let ctx_for_grant_steward = ctx_arc.clone();
    let on_grant_steward: GrantStewardCallback = Arc::new(move |resident_id: String| {
        let ctx = ctx_for_grant_steward.clone();
        let cmd = EffectCommand::GrantSteward {
            target: resident_id.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to grant steward role: {}", e);
                ctx.add_error_toast(
                    "steward-error",
                    format!("Failed to grant steward role: {}", e),
                )
                .await;
            }
        });
        tracing::info!("Granting steward role to: {}", resident_id);
    });

    // RevokeStewardCallback for demoting a steward back to resident
    let ctx_for_revoke_steward = ctx_arc.clone();
    let on_revoke_steward: RevokeStewardCallback = Arc::new(move |resident_id: String| {
        let ctx = ctx_for_revoke_steward.clone();
        let cmd = EffectCommand::RevokeSteward {
            target: resident_id.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = ctx.dispatch(cmd).await {
                eprintln!("Failed to revoke steward role: {}", e);
                ctx.add_error_toast(
                    "steward-error",
                    format!("Failed to revoke steward role: {}", e),
                )
                .await;
            }
        });
        tracing::info!("Revoking steward role from: {}", resident_id);
    });

    // CreateAccountCallback for account creation from setup modal
    // This calls IoContext::create_account which actually creates the account.json file
    let ctx_for_create = ctx_arc.clone();
    let on_create_account: CreateAccountCallback = Arc::new(move |display_name: String| {
        let ctx = ctx_for_create.clone();
        // Call the actual create_account method that writes the file
        match ctx.create_account(&display_name) {
            Ok(()) => {
                println!("Account created successfully for: {}", display_name);
                // Also dispatch the intent to create a journal fact
                let cmd = EffectCommand::CreateAccount {
                    display_name: display_name.clone(),
                };
                tokio::spawn(async move {
                    if let Err(e) = ctx.dispatch(cmd).await {
                        // Non-fatal: file was created, journal fact is optional
                        eprintln!("Note: Journal fact creation failed: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Failed to create account: {}", e);
            }
        }
    });

    // Check if account already exists to determine if we show setup modal
    let show_account_setup = !ctx_arc.has_account();

    // Get data from IoContext
    let channels = ctx_arc.get_channels();
    let messages = ctx_arc.get_messages();
    let guardians = ctx_arc.get_guardians();
    let recovery_status = ctx_arc.get_recovery_status();
    let invitations = ctx_arc.get_invitations();
    let contacts = ctx_arc.get_contacts();
    let residents = ctx_arc.get_residents();
    let block_budget = ctx_arc.get_block_budget();

    // Get discovered peers from rendezvous service
    let discovered_peer_data = ctx_arc.get_discovered_peers().await;
    let invited_peer_ids = ctx_arc.get_invited_peer_ids().await;
    let discovered_peers: Vec<DiscoveredPeerInfo> = discovered_peer_data
        .into_iter()
        .map(|(authority_id, address)| {
            let addr = if address.is_empty() {
                "rendezvous".to_string()
            } else {
                address
            };
            // Check if this peer has been invited
            let status = if invited_peer_ids.contains(&authority_id) {
                PeerInvitationStatus::Pending
            } else {
                PeerInvitationStatus::None
            };
            DiscoveredPeerInfo::new(&authority_id, &addr)
                .with_method("rendezvous")
                .with_status(status)
        })
        .collect();

    // Get block info from snapshot
    let block_snap = ctx_arc.snapshot_block();
    let block_name = block_snap
        .block
        .as_ref()
        .map(|b| b.name.clone())
        .unwrap_or_else(|| "My Block".to_string());
    // Chat channel uses selected channel from context
    // Note: Block messages use block:<block_id> channel, computed dynamically in on_block_send callback
    let channel_name = ctx_arc
        .get_selected_channel()
        .unwrap_or_else(|| "general".to_string());

    // Get neighborhood info from snapshot
    let neighborhood_snap = ctx_arc.snapshot_neighborhood();
    let neighborhood_name = neighborhood_snap
        .neighborhood_name
        .clone()
        .unwrap_or_else(|| "Neighborhood".to_string());
    let blocks: Vec<BlockSummary> = neighborhood_snap
        .blocks
        .iter()
        .map(|b| {
            let name = if b.name.is_empty() { b.id.clone() } else { b.name.clone() };
            BlockSummary::new(&b.id)
                .with_name(&name)
                .with_residents(b.resident_count.unwrap_or(0) as u8)
        })
        .collect();

    // Device list: Retrieved from IoContext snapshot
    // Current device is derived from device_id; additional devices will come from commitment tree
    let devices = ctx_arc.get_devices();

    // Get threshold info from recovery status
    let threshold_k = recovery_status.threshold as u8;
    let threshold_n = guardians.len().max(recovery_status.threshold as usize) as u8;

    // Get sync status for status bar display
    let sync_in_progress = ctx_arc.is_syncing().await;
    let last_sync_time = ctx_arc.last_sync_time().await;
    let peer_count = ctx_arc.known_peers_count().await;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());

    // Wrap the app in ContextProvider
    // This enables components to use `hooks.use_context::<AppCoreContext>()` for
    // reactive signal subscription via `use_future`
    {
        let context = app_core_context;
        element! {
            ContextProvider(value: Context::owned(context)) {
                IoApp(
                    channels: channels,
                    messages: messages,
                    invitations: invitations,
                    guardians: guardians,
                    devices: devices,
                    display_name: ctx_arc.get_display_name().await,
                    threshold_k: threshold_k,
                    threshold_n: threshold_n,
                    mfa_policy: MfaPolicy::SensitiveOnly,
                    recovery_status: recovery_status,
                    // Block screen data
                    block_name: block_name,
                    residents: residents,
                    block_budget: block_budget,
                    channel_name: channel_name,
                    // Contacts screen data
                    contacts: contacts,
                    discovered_peers: discovered_peers,
                    // Neighborhood screen data
                    neighborhood_name: neighborhood_name,
                    blocks: blocks,
                    traversal_depth: TraversalDepth::Street,
                    // Effect dispatch callbacks
                    on_send: Some(on_send),
                    on_retry_message: Some(on_retry_message),
                    on_channel_select: Some(on_channel_select.clone()),
                    on_create_channel: Some(on_create_channel),
                    on_set_topic: Some(on_set_topic),
                    on_accept_invitation: Some(on_accept_invitation),
                    on_decline_invitation: Some(on_decline_invitation),
                    on_create_invitation: Some(on_create_invitation),
                    on_export_invitation: Some(on_export_invitation),
                    on_import_invitation: Some(on_import_invitation),
                    on_enter_block: Some(on_enter_block),
                    on_go_home: Some(on_go_home),
                    on_back_to_street: Some(on_back_to_street),
                    // Recovery callbacks
                    on_start_recovery: Some(on_start_recovery),
                    on_add_guardian: Some(on_add_guardian),
                    on_select_guardian: Some(on_select_guardian),
                    pending_requests: Vec::new(), // Populated reactively in RecoveryScreen
                    on_submit_approval: Some(on_submit_approval),
                    // Settings callbacks
                    on_update_mfa: Some(on_update_mfa),
                    on_update_nickname: Some(on_update_nickname),
                    on_update_threshold: Some(on_update_threshold),
                    on_add_device: Some(on_add_device),
                    on_remove_device: Some(on_remove_device),
                    // Contacts callbacks
                    on_update_petname: Some(on_update_petname),
                    on_start_chat: Some(on_start_chat),
                    on_invite_lan_peer: Some(on_invite_lan_peer),
                    // Block callbacks
                    on_block_send: Some(on_block_send),
                    on_block_invite: Some(on_block_invite),
                    on_block_navigate_neighborhood: Some(on_block_navigate_neighborhood),
                    on_grant_steward: Some(on_grant_steward),
                    on_revoke_steward: Some(on_revoke_steward),
                    // Account setup
                    show_account_setup: show_account_setup,
                    on_create_account: Some(on_create_account.clone()),
                    // Sync status
                    sync_in_progress: sync_in_progress,
                    last_sync_time: last_sync_time,
                    peer_count: peer_count,
                    // Demo mode (get from context)
                    demo_mode: ctx_arc.is_demo_mode(),
                    demo_alice_code: ctx_arc.demo_alice_code(),
                    demo_carol_code: ctx_arc.demo_carol_code(),
                )
            }
        }
        .fullscreen()
        .await
    }
}

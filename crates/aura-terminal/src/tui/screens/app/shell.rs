//! # App Shell
//!
//! Main application shell with screen navigation and modal management.
//!
//! This is the root TUI component that coordinates all screens, handles
//! events, manages the state machine, and renders modals.

use super::account_setup_modal::{AccountSetupModal, AccountSetupState};
use super::modal_overlays::{
    render_add_device_modal, render_block_invite_modal, render_channel_info_modal,
    render_chat_create_modal, render_contacts_create_modal, render_contacts_import_modal,
    render_guardian_setup_modal, render_invitation_code_modal, render_invitations_create_modal,
    render_invitations_import_modal, render_nickname_modal, render_petname_modal,
    render_remove_device_modal, render_threshold_modal, render_topic_modal,
};

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    ContactSelectModal, ContactSelectState, DiscoveredPeerInfo, Footer, HelpModal, HelpModalState,
    ModalFrame, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::layout::dim;
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    BlockScreen, ChatScreen, ContactsScreen, NeighborhoodScreen, RecoveryScreen, SettingsScreen,
};
use crate::tui::types::{
    BlockBudget, BlockSummary, Channel, Contact, Device, Guardian, Invitation, KeyHint, Message,
    MfaPolicy, PendingRequest, RecoveryStatus, Resident, TraversalDepth,
};

// State machine integration
use crate::tui::convert_iocraft_event;
use crate::tui::props::{
    extract_block_view_props, extract_chat_view_props, extract_contacts_view_props,
    extract_invitations_view_props, extract_neighborhood_view_props, extract_recovery_view_props,
    extract_settings_view_props,
};
use crate::tui::state_machine::{transition, DispatchCommand, QueuedModal, TuiCommand, TuiState};
use crate::tui::updates::{ui_update_channel, UiUpdate, UiUpdateReceiver, UiUpdateSender};
use std::sync::Mutex;

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
    /// Pending recovery requests from others that we can approve
    pub pending_requests: Vec<PendingRequest>,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
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
    // Reactive update channel - receiver wrapped in Arc<Mutex<Option>> for take-once semantics
    /// UI update receiver for reactive updates from callbacks
    pub update_rx: Option<Arc<Mutex<Option<UiUpdateReceiver>>>>,
    /// UI update sender for sending updates from event handlers
    pub update_tx: Option<UiUpdateSender>,
    /// Callback registry for all domain actions
    pub callbacks: Option<CallbackRegistry>,
}

/// Main application with screen navigation
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Block);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

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

    // =========================================================================
    // UI Update Channel - Single reactive channel for all async callback results
    //
    // Callbacks in run_app_with_context send their results through this channel.
    // The update processor (use_future below) awaits on this channel and updates
    // State<T> values, which automatically trigger re-renders via iocraft's waker.
    //
    // The receiver is passed via props.update_rx from run_app_with_context.
    // This replaces polling loops and detached tokio::spawn patterns.
    // =========================================================================
    let update_rx_holder = props.update_rx.clone();
    let _update_tx_holder = props.update_tx.clone();

    // Account setup modal state - LEGACY hook-based system for backwards compatibility
    // NOTE: When show_setup is true, the queue-based system (TuiState.modal_queue) handles
    // the account setup modal. The legacy state is NOT initialized to avoid dual-state issues.
    // The queue-based system is the source of truth for account setup modals.
    let account_state = hooks.use_ref(AccountSetupState::new);
    let account_version = hooks.use_state(|| 0usize);

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

    // Display name state - State<T> automatically triggers re-renders on .set()
    let display_name_state = hooks.use_state({
        let initial = props.display_name.clone();
        move || initial
    });

    // Get AppCoreContext for IoContext access
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // NOTE: Toast polling loop removed - toasts now flow through UiUpdate channel
    // All toast operations (ShowToast, DismissToast, ClearAllToasts) send UiUpdate variants
    // which are processed by the UI Update Processor below

    // =========================================================================
    // UI Update Processor - Central handler for all async callback results

    // This is the single point where all async updates flow through.
    // Callbacks send UiUpdate variants, this processor matches and updates
    // the appropriate State<T> values, triggering automatic re-renders.
    // Only runs if update_rx was provided via props.
    // =========================================================================
    if let Some(rx_holder) = update_rx_holder {
        hooks.use_future({
            let mut display_name_state = display_name_state.clone();
            let mut account_state = account_state.clone();
            let mut account_version = account_version.clone();
            // Toast queue migration: use TuiState.toast_queue instead of toasts_state
            let mut tui_state = tui_state.clone();
            let mut tui_state_version = tui_state_version.clone();
            async move {
                // Helper macro-like function to add a toast to the queue
                // (Inline to avoid borrow checker issues with closures)
                macro_rules! enqueue_toast {
                    ($msg:expr, $level:expr) => {{
                        let mut state = tui_state.write();
                        let toast_id = state.next_toast_id;
                        state.next_toast_id += 1;
                        let toast =
                            crate::tui::state_machine::QueuedToast::new(toast_id, $msg, $level);
                        state.toast_queue.enqueue(toast);
                        drop(state);
                        tui_state_version.set(tui_state_version.get().wrapping_add(1));
                    }};
                }

                // Take the receiver from the holder (only happens once)
                #[allow(clippy::expect_used)]
                // TUI initialization - panic is appropriate if channel setup failed
                let mut rx = {
                    let mut guard = rx_holder.lock().expect("Failed to lock update_rx");
                    guard.take().expect("UI update receiver already taken")
                };

                // Process updates as they arrive
                while let Some(update) = rx.recv().await {
                    match update {
                        // Settings updates - State<T>.set() triggers re-render automatically
                        UiUpdate::DisplayNameChanged(name) => {
                            display_name_state.set(name);
                        }

                        // Toast notifications - now use queue system
                        UiUpdate::ToastAdded(toast) => {
                            // Convert ToastMessage to QueuedToast and enqueue
                            let level = match toast.level {
                                ToastLevel::Info => crate::tui::state_machine::ToastLevel::Info,
                                ToastLevel::Success => {
                                    crate::tui::state_machine::ToastLevel::Success
                                }
                                ToastLevel::Warning => {
                                    crate::tui::state_machine::ToastLevel::Warning
                                }
                                ToastLevel::Error | ToastLevel::Conflict => {
                                    crate::tui::state_machine::ToastLevel::Error
                                }
                            };
                            enqueue_toast!(toast.message, level);
                        }
                        UiUpdate::ToastDismissed { toast_id: _ } => {
                            // Dismiss from queue (FIFO, ignores ID)
                            tui_state.write().toast_queue.dismiss();
                            tui_state_version.set(tui_state_version.get().wrapping_add(1));
                        }
                        UiUpdate::ToastsCleared => {
                            tui_state.write().toast_queue.clear();
                            tui_state_version.set(tui_state_version.get().wrapping_add(1));
                        }

                        // Error handling - show in modal or as toast depending on operation
                        UiUpdate::OperationFailed { operation, error } => {
                            // For account creation, show error in the modal instead of toast
                            if operation == "CreateAccount" {
                                // Update both queue-based state and legacy hook state
                                tui_state.write().modal_queue.update_active(|modal| {
                                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                                        s.set_error(error.clone());
                                    }
                                });
                                account_state.write().set_error(&error);
                                account_version.set(account_version.get().wrapping_add(1));
                                tui_state_version.set(tui_state_version.get().wrapping_add(1));
                            } else {
                                // For other operations, show as toast via queue
                                enqueue_toast!(
                                    format!("{} failed: {}", operation, error),
                                    crate::tui::state_machine::ToastLevel::Error
                                );
                            }
                        }

                        // Success notifications - show informational toasts via queue
                        UiUpdate::MessageSent { channel, .. } => {
                            enqueue_toast!(
                                format!("Message sent to {}", channel),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }

                        UiUpdate::InvitationAccepted { invitation_id: _ } => {
                            enqueue_toast!(
                                "Invitation accepted".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        UiUpdate::InvitationDeclined { invitation_id: _ } => {
                            enqueue_toast!(
                                "Invitation declined".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }

                        UiUpdate::InvitationCreated { invitation_code: _ } => {
                            enqueue_toast!(
                                "Invitation created".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        UiUpdate::SyncCompleted => {
                            enqueue_toast!(
                                "Sync completed".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        UiUpdate::SyncFailed { error } => {
                            enqueue_toast!(
                                format!("Sync failed: {}", error),
                                crate::tui::state_machine::ToastLevel::Error
                            );
                        }

                        UiUpdate::AccountCreated => {
                            // Update the account setup modal to show success screen
                            // Uses only the queue-based state (legacy state is deprecated)
                            tui_state.write().account_created_queued();
                            tui_state_version.set(tui_state_version.get().wrapping_add(1));
                        }

                        UiUpdate::RecoveryStarted => {
                            enqueue_toast!(
                                "Recovery process started".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }

                        // Navigation and state changes - no toast needed, handled by navigation system
                        UiUpdate::ChannelSelected(_)
                        | UiUpdate::ChannelCreated(_)
                        | UiUpdate::BlockEntered { .. }
                        | UiUpdate::NavigatedHome
                        | UiUpdate::NavigatedToStreet
                        | UiUpdate::NavigatedToNeighborhood => {
                            // Navigation handled elsewhere - no additional UI update needed
                        }

                        // Other updates - log in debug mode only
                        _ => {
                            // Intentionally no stdout/stderr logging here: writing to the terminal
                            // while iocraft is in fullscreen mode can scroll the buffer and create
                            // visual artifacts (e.g., duplicated nav bar).
                        }
                    }
                }
            }
        });
    }

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
    // Use reactively updated display_name from UiUpdate channel - State<T> triggers re-renders
    let display_name = display_name_state.read().clone();
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
    // Pending recovery requests
    let pending_requests = props.pending_requests.clone();
    // Callbacks registry and individual callback extraction for screen props
    let callbacks = props.callbacks.clone();

    // Extract individual callbacks from registry for screen component props
    // (Screen components still use individual callback props for now)
    let on_block_send = callbacks.as_ref().map(|cb| cb.block.on_send.clone());
    let on_block_invite = callbacks.as_ref().map(|cb| cb.block.on_invite.clone());
    let on_block_navigate_neighborhood = callbacks
        .as_ref()
        .map(|cb| cb.block.on_navigate_neighborhood.clone());
    let on_grant_steward = callbacks
        .as_ref()
        .map(|cb| cb.block.on_grant_steward.clone());
    let on_revoke_steward = callbacks
        .as_ref()
        .map(|cb| cb.block.on_revoke_steward.clone());

    let on_send = callbacks.as_ref().map(|cb| cb.chat.on_send.clone());
    let on_retry_message = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_retry_message.clone());
    let on_channel_select = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_channel_select.clone());
    let on_create_channel = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_create_channel.clone());
    let on_set_topic = callbacks.as_ref().map(|cb| cb.chat.on_set_topic.clone());

    let on_update_petname = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_update_petname.clone());
    let on_start_chat = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_start_chat.clone());
    let on_invite_lan_peer = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_invite_lan_peer.clone());
    let on_import_invitation = callbacks
        .as_ref()
        .map(|cb| cb.invitations.on_import.clone());

    let on_enter_block = callbacks
        .as_ref()
        .map(|cb| cb.neighborhood.on_enter_block.clone());
    let on_go_home = callbacks
        .as_ref()
        .map(|cb| cb.neighborhood.on_go_home.clone());
    let on_back_to_street = callbacks
        .as_ref()
        .map(|cb| cb.neighborhood.on_back_to_street.clone());

    let on_update_mfa = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_mfa.clone());
    let on_update_nickname = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_nickname.clone());
    let on_update_threshold = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_threshold.clone());
    let on_add_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_add_device.clone());
    let on_remove_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_remove_device.clone());

    let on_start_recovery = callbacks
        .as_ref()
        .map(|cb| cb.recovery.on_start_recovery.clone());
    let on_add_guardian = callbacks
        .as_ref()
        .map(|cb| cb.recovery.on_add_guardian.clone());
    let on_submit_approval = callbacks
        .as_ref()
        .map(|cb| cb.recovery.on_submit_approval.clone());

    let current_screen = screen.get();

    // Check if in insert mode (MessageInput has its own hint bar, so hide main hints)
    let is_insert_mode = tui_state.read().is_insert_mode();

    // Extract screen view props from TuiState using testable extraction functions
    let block_props = extract_block_view_props(&tui_state.read());
    let chat_props = extract_chat_view_props(&tui_state.read());
    let contacts_props = extract_contacts_view_props(&tui_state.read());
    let invitations_props = extract_invitations_view_props(&tui_state.read());
    let settings_props = extract_settings_view_props(&tui_state.read());
    let recovery_props = extract_recovery_view_props(&tui_state.read());
    let neighborhood_props = extract_neighborhood_view_props(&tui_state.read());

    // =========================================================================
    // NEW: Extract modal state from queue (type-enforced single modal at a time)
    // This will replace the legacy hook-based modal state extraction above.
    // =========================================================================
    let queued_modal = tui_state.read().modal_queue.current().cloned();
    let queue_account_setup = match &queued_modal {
        Some(QueuedModal::AccountSetup(state)) => Some(state.clone()),
        _ => None,
    };
    let queue_help = match &queued_modal {
        Some(QueuedModal::Help { current_screen }) => current_screen.clone(),
        _ => None,
    };
    let queue_guardian_select = match &queued_modal {
        Some(QueuedModal::GuardianSelect(state)) => Some(state.clone()),
        _ => None,
    };
    let _queue_contact_select = match &queued_modal {
        Some(QueuedModal::ContactSelect(state)) => Some(state.clone()),
        _ => None,
    };
    let _queue_confirm = match &queued_modal {
        Some(QueuedModal::Confirm {
            title,
            message,
            on_confirm: _,
        }) => Some((title.clone(), message.clone())),
        _ => None,
    };

    // Pre-compute account setup modal props for cleaner rendering
    let account_setup_props = queue_account_setup.as_ref().map(|state| {
        (
            state.display_name.clone(),
            state.creating,
            state.should_show_spinner(),
            state.success,
            state.error.clone().unwrap_or_default(),
        )
    });

    // Extract toast state from queue (type-enforced single toast at a time)
    let queued_toast = tui_state.read().toast_queue.current().cloned();

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
        // Clone IoContext for ceremony operations
        let io_ctx_for_ceremony = app_ctx.io_context.clone();
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        // Clone callbacks registry for command dispatch
        let callbacks = callbacks.clone();
        // Clone contacts for use inside the closure (for populating guardian modal)
        let contacts_for_modal_populate = contacts.clone();
        move |event| {
            // Convert iocraft event to aura-core event and run through state machine
            if let Some(core_event) = convert_iocraft_event(event.clone()) {
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

                // Sync modal queue state from TuiState to iocraft hooks
                match new_state.modal_queue.current() {
                    Some(QueuedModal::AccountSetup(state)) => {
                        // Sync account setup modal state
                        let legacy_visible = account_state.read().visible;
                        if !legacy_visible {
                            account_state.write().show();
                            account_version.set(account_version.get().wrapping_add(1));
                        }
                        // Sync the display_name and other fields
                        let mut legacy = account_state.write();
                        legacy.display_name = state.display_name.clone();
                        legacy.creating = state.creating;
                        legacy.success = state.success;
                        legacy.error = state.error.clone();
                        drop(legacy);
                        account_version.set(account_version.get().wrapping_add(1));
                    }
                    Some(QueuedModal::Help { .. }) => {
                        if !help_modal_state.read().visible {
                            help_modal_state.write().show();
                            help_modal_version.set(help_modal_version.get().wrapping_add(1));
                        }
                    }
                    Some(QueuedModal::GuardianSelect(state)) => {
                        if !guardian_select_state.read().visible {
                            // Convert contacts to the format expected by the modal
                            let contacts: Vec<Contact> = state
                                .contacts
                                .iter()
                                .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                                .collect();
                            guardian_select_state
                                .write()
                                .show("Select Guardian", contacts);
                            guardian_select_version
                                .set(guardian_select_version.get().wrapping_add(1));
                        }
                    }
                    Some(QueuedModal::ContactSelect(state)) => {
                        if !contact_select_state.read().visible {
                            let contacts: Vec<Contact> = state
                                .contacts
                                .iter()
                                .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                                .collect();
                            contact_select_state
                                .write()
                                .show(&state.title, contacts);
                            contact_select_version
                                .set(contact_select_version.get().wrapping_add(1));
                        }
                    }
                    Some(QueuedModal::Confirm { title, message, .. }) => {
                        if !confirm_modal_visible.get() {
                            confirm_modal_visible.set(true);
                            *confirm_modal_title.write() = title.clone();
                            *confirm_modal_message.write() = message.clone();
                            confirm_modal_version
                                .set(confirm_modal_version.get().wrapping_add(1));
                        }
                    }
                    None => {
                        // Close any open modal hooks when no queued modal
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
                    // Other queued modal types don't need iocraft hook sync
                    _ => {}
                }

                // Execute commands using callbacks registry
                if let Some(ref cb) = callbacks {
                    for cmd in commands {
                        match cmd {
                            TuiCommand::Exit => {
                                should_exit.set(true);
                            }
                            TuiCommand::Dispatch(dispatch_cmd) => {
                                // Handle dispatch commands via CallbackRegistry
                                match dispatch_cmd {
                                    DispatchCommand::CreateAccount { name } => {
                                        (cb.app.on_create_account)(name);
                                    }
                                    DispatchCommand::AddGuardian { contact_id } => {
                                        (cb.recovery.on_select_guardian)(contact_id);
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

                                        // Also dismiss the modal from the queue
                                        tui_state.write().modal_queue.dismiss();

                                        // Call the callback with contact_id
                                        if let Some(contact_id) = contact_id {
                                            (cb.recovery.on_select_guardian)(contact_id);
                                        }
                                    }

                                    // === Block Screen Commands ===
                                    DispatchCommand::SendBlockMessage { content } => {
                                        (cb.block.on_send)(content);
                                    }
                                    DispatchCommand::InviteToBlock { contact_id } => {
                                        (cb.block.on_invite)(contact_id);
                                    }
                                    DispatchCommand::GrantSteward { resident_id } => {
                                        (cb.block.on_grant_steward)(resident_id);
                                    }
                                    DispatchCommand::RevokeSteward { resident_id } => {
                                        (cb.block.on_revoke_steward)(resident_id);
                                    }

                                    // === Chat Screen Commands ===
                                    DispatchCommand::SelectChannel { channel_id } => {
                                        (cb.chat.on_channel_select)(channel_id);
                                    }
                                    DispatchCommand::SendChatMessage {
                                        channel_id,
                                        content,
                                    } => {
                                        (cb.chat.on_send)(channel_id, content);
                                    }
                                    DispatchCommand::RetryMessage { message_id } => {
                                        // Note: RetryMessage requires channel and content from the failed message
                                        // For now, log a warning since we don't have the full message context here
                                        tracing::warn!(
                                            "RetryMessage not fully implemented: message_id={}",
                                            message_id
                                        );
                                    }
                                    DispatchCommand::CreateChannel { name } => {
                                        (cb.chat.on_create_channel)(name, None);
                                    }
                                    DispatchCommand::SetChannelTopic { channel_id, topic } => {
                                        (cb.chat.on_set_topic)(channel_id, topic);
                                    }
                                    DispatchCommand::DeleteChannel { channel_id } => {
                                        // TODO: Implement channel deletion callback
                                        tracing::info!("Delete channel requested: {}", channel_id);
                                    }

                                    // === Contacts Screen Commands ===
                                    DispatchCommand::UpdatePetname {
                                        contact_id,
                                        petname,
                                    } => {
                                        (cb.contacts.on_update_petname)(contact_id, petname);
                                    }
                                    DispatchCommand::StartChat { contact_id } => {
                                        (cb.contacts.on_start_chat)(contact_id);
                                    }
                                    DispatchCommand::RemoveContact { contact_id } => {
                                        // TODO: Implement contact removal callback
                                        tracing::info!("Remove contact requested: {}", contact_id);
                                    }
                                    DispatchCommand::SelectContactByIndex { index } => {
                                        // Generic contact selection by index
                                        // This is used by ContactSelect modal - map index to contact_id
                                        tracing::info!("Contact selected by index: {}", index);
                                        // Dismiss the modal after selection
                                        tui_state.write().modal_queue.dismiss();
                                        tui_state_version.set(tui_state_version.get() + 1);
                                    }

                                    // === Invitations Screen Commands ===
                                    DispatchCommand::AcceptInvitation { invitation_id } => {
                                        (cb.invitations.on_accept)(invitation_id);
                                    }
                                    DispatchCommand::DeclineInvitation { invitation_id } => {
                                        (cb.invitations.on_decline)(invitation_id);
                                    }
                                    DispatchCommand::CreateInvitation {
                                        invitation_type,
                                        message,
                                    } => {
                                        // Third argument is TTL in seconds (None = no expiry)
                                        (cb.invitations.on_create)(invitation_type, message, None);
                                    }
                                    DispatchCommand::ImportInvitation { code } => {
                                        (cb.invitations.on_import)(code);
                                    }
                                    DispatchCommand::ExportInvitation { invitation_id } => {
                                        (cb.invitations.on_export)(invitation_id);
                                    }
                                    DispatchCommand::RevokeInvitation { invitation_id } => {
                                        // TODO: Implement invitation revocation callback
                                        tracing::info!("Revoke invitation requested: {}", invitation_id);
                                    }

                                    // === Recovery Screen Commands ===
                                    DispatchCommand::StartRecovery => {
                                        (cb.recovery.on_start_recovery)();
                                    }
                                    DispatchCommand::ApproveRecovery { request_id } => {
                                        (cb.recovery.on_submit_approval)(request_id);
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

                                            // Initiate guardian ceremony through the real protocol.
                                            // This sends guardian invitations to each guardian through
                                            // their full Aura runtimes (not mock acceptance).
                                            match core.initiate_guardian_ceremony(k as u16, n as u16, &ids).await {
                                                Ok(ceremony_id) => {
                                                    tracing::info!(
                                                        ceremony_id = ?ceremony_id,
                                                        threshold = k,
                                                        guardians = n,
                                                        "Guardian ceremony initiated, waiting for guardian responses"
                                                    );

                                                    // The ceremony will proceed through the actual protocol:
                                                    // 1. Key packages are sent to each guardian
                                                    // 2. Guardians process invitations through their runtimes
                                                    // 3. They respond with AcceptGuardianBinding or decline
                                                    // 4. GuardianBinding facts are committed to journal
                                                    // 5. Views update reactively from journal facts
                                                    //
                                                    // No mock acceptance here - full protocol fidelity!

                                                    // Show ceremony started toast
                                                    io_ctx.add_info_toast(
                                                        "guardian-ceremony-started",
                                                        format!("Guardian ceremony started! Waiting for {}-of-{} guardians to respond", k, n)
                                                    ).await;

                                                    // Spawn a task to monitor ceremony progress and show completion
                                                    let app_core_monitor = app_core.clone();
                                                    let io_ctx_monitor = io_ctx.clone();
                                                    tokio::spawn(async move {
                                                        // Poll for ceremony completion (max 30 seconds)
                                                        for _ in 0..60 {
                                                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                                                            let core = app_core_monitor.read().await;
                                                            if let Ok(status) = core.get_ceremony_status(&ceremony_id).await {
                                                                if status.is_complete {
                                                                    tracing::info!("Guardian ceremony completed successfully");
                                                                    io_ctx_monitor.add_success_toast(
                                                                        "guardian-ceremony-complete",
                                                                        format!("Guardian ceremony complete! {}-of-{} threshold achieved", k, n)
                                                                    ).await;
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!("Failed to initiate guardian ceremony: {}", e);

                                                    io_ctx.add_error_toast(
                                                        "guardian-ceremony-error",
                                                        format!("Failed to initiate guardian ceremony: {}", e)
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
                                        (cb.settings.on_update_nickname)(nickname);
                                    }
                                    DispatchCommand::UpdateThreshold { k, n } => {
                                        (cb.settings.on_update_threshold)(k, n);
                                    }
                                    DispatchCommand::UpdateMfaPolicy { policy } => {
                                        (cb.settings.on_update_mfa)(policy);
                                    }
                                    DispatchCommand::AddDevice { name } => {
                                        (cb.settings.on_add_device)(name);
                                    }
                                    DispatchCommand::RemoveDevice { device_id } => {
                                        (cb.settings.on_remove_device)(device_id);
                                    }

                                    // === Neighborhood Screen Commands ===
                                    DispatchCommand::EnterBlock { block_id } => {
                                        // Default to Street-level traversal depth
                                        (cb.neighborhood.on_enter_block)(block_id, TraversalDepth::default());
                                    }
                                    DispatchCommand::GoHome => {
                                        (cb.neighborhood.on_go_home)();
                                    }
                                    DispatchCommand::BackToStreet => {
                                        (cb.neighborhood.on_back_to_street)();
                                    }

                                    // === Navigation Commands ===
                                    DispatchCommand::NavigateTo(_screen) => {
                                        // Navigation is handled by TuiState directly
                                        // The state machine already updates the screen
                                    }
                                }
                            }
                            TuiCommand::ShowToast { message, level } => {
                                // Add toast to queue (type-enforced single toast at a time)
                                let mut state = tui_state.write();
                                let toast_id = state.next_toast_id;
                                state.next_toast_id += 1;
                                let toast = crate::tui::state_machine::QueuedToast::new(
                                    toast_id,
                                    message,
                                    level,
                                );
                                state.toast_queue.enqueue(toast);
                                drop(state);
                                tui_state_version.set(tui_state_version.get().wrapping_add(1));
                            }
                            TuiCommand::DismissToast { id: _ } => {
                                // Dismiss current toast from queue (ignores ID - FIFO semantics)
                                tui_state.write().toast_queue.dismiss();
                                tui_state_version.set(tui_state_version.get().wrapping_add(1));
                            }
                            TuiCommand::ClearAllToasts => {
                                // Clear all toasts from queue
                                tui_state.write().toast_queue.clear();
                                tui_state_version.set(tui_state_version.get().wrapping_add(1));
                            }
                            TuiCommand::Render => {
                                // Render is handled by iocraft automatically
                            }
                        }
                    }
                }

                // Update TuiState
                *tui_state.write() = new_state;

                // If the guardian setup modal just became visible, populate its contacts list
                // from the reactive contacts data
                {
                    let mut state = tui_state.write();
                    // Check queue for GuardianSetup modal and populate contacts if empty
                    let needs_population = matches!(
                        state.modal_queue.current(),
                        Some(QueuedModal::GuardianSetup(s)) if s.contacts.is_empty()
                    );

                    if needs_population {
                        // Populate from the contacts prop (which comes from reactive signals)
                        // We need to convert Contact -> GuardianCandidate
                        let candidates: Vec<_> = contacts_for_modal_populate
                            .iter()
                            .map(|c| crate::tui::state_machine::GuardianCandidate {
                                id: c.id.clone(),
                                name: c.petname.clone(),
                                is_current_guardian: c.is_guardian,
                            })
                            .collect();

                        // Pre-select existing guardians
                        let selected: Vec<_> = candidates
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.is_current_guardian)
                            .map(|(i, _)| i)
                            .collect();

                        let count = candidates.len();

                        // Update the modal in the queue
                        state.modal_queue.update_active(|modal| {
                            if let QueuedModal::GuardianSetup(ref mut s) = modal {
                                s.contacts = candidates.clone();
                                s.selected_indices = selected.clone();
                            }
                        });

                        tracing::debug!("Populated guardian modal with {} contacts", count);
                    }
                }

                tui_state_version.set(tui_state_version.get().wrapping_add(1));
            }

            // All key events are handled by the state machine above.
            // Modal handling (AccountSetup, GuardianSelect, Help) goes through
            // transition() -> sync to legacy state -> command execution.
        }
    });

    // Legacy account_state is kept for potential future use but not actively used.
    // The queue-based system (TuiState.modal_queue) is the source of truth.
    // Trigger version read to allow hooks system to track changes if needed later.
    let _ = account_version.get();
    let _ = account_state.read(); // Keep ref active

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
    let _contact_modal_visible = contact_state_ref.visible;
    let _contact_modal_title = contact_state_ref.title.clone();
    let _contact_modal_contacts = contact_state_ref.contacts.clone();
    let _contact_modal_selected = contact_state_ref.selected_index;
    let _contact_modal_error = contact_state_ref.error.clone().unwrap_or_default();
    drop(contact_state_ref); // Release the read lock
                             // contact_select_version is used for triggering re-renders (not directly in UI)
    let _ = contact_select_version.get();

    // Extract confirm modal state for rendering
    let _confirm_visible = confirm_modal_visible.get();
    let _confirm_title = confirm_modal_title.read().clone();
    let _confirm_message = confirm_modal_message.read().clone();
    // confirm_modal_version is used for triggering re-renders (not directly in UI)
    let _ = confirm_modal_version.get();

    // Extract help modal state for rendering from use_ref
    let help_modal_visible = help_modal_state.read().visible;
    // help_modal_version is used for triggering re-renders (not directly in UI)
    let _ = help_modal_version.get();

    // Extract sync status from props
    let syncing = props.sync_in_progress;
    let last_sync = props.last_sync_time;
    let peers = props.peer_count;

    // Layout: NavBar (3 rows) + Content (25 rows) + Footer (3 rows) = 31 = TOTAL_HEIGHT
    //
    // Content always renders. Modals overlay via ModalFrame (Position::Absolute).
    // ModalFrame positions at top: NAV_HEIGHT to overlay the content area.

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::TOTAL_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Nav bar area (3 rows) - always visible
            NavBar(
                active_screen: current_screen,
                syncing: syncing,
                last_sync_time: last_sync,
                peer_count: peers,
            )

            // Middle content area (25 rows) - always renders screen content
            // Modals overlay via ModalFrame (absolute positioning)
            View(
                width: dim::TOTAL_WIDTH,
                height: dim::MIDDLE_HEIGHT,
                overflow: Overflow::Hidden,
            ) {
                #(match current_screen {
                    Screen::Block => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            BlockScreen(
                                block_name: block_name.clone(),
                                residents: residents.clone(),
                                messages: messages.clone(),
                                budget: block_budget.clone(),
                                channel_name: channel_name.clone(),
                                contacts: contacts.clone(),
                                view: block_props.clone(),
                                on_send: on_block_send.clone(),
                                on_invite: on_block_invite.clone(),
                                on_go_neighborhood: on_block_navigate_neighborhood.clone(),
                                on_grant_steward: on_grant_steward.clone(),
                                on_revoke_steward: on_revoke_steward.clone(),
                            )
                        }
                    }],
                    Screen::Chat => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ChatScreen(
                                channels: channels.clone(),
                                messages: messages.clone(),
                                view: chat_props.clone(),
                                on_send: on_send.clone(),
                                on_retry_message: on_retry_message.clone(),
                                on_channel_select: on_channel_select.clone(),
                                on_create_channel: on_create_channel.clone(),
                                on_set_topic: on_set_topic.clone(),
                            )
                        }
                    }],
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ContactsScreen(
                                contacts: contacts.clone(),
                                discovered_peers: discovered_peers.clone(),
                                view: contacts_props.clone(),
                                on_update_petname: on_update_petname.clone(),
                                on_start_chat: on_start_chat.clone(),
                                on_invite_lan_peer: on_invite_lan_peer.clone(),
                                on_import_invitation: on_import_invitation.clone(),
                            )
                        }
                    }],
                    Screen::Neighborhood => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            NeighborhoodScreen(
                                neighborhood_name: neighborhood_name.clone(),
                                blocks: blocks.clone(),
                                depth: traversal_depth,
                                view: neighborhood_props.clone(),
                                on_enter_block: on_enter_block.clone(),
                                on_go_home: on_go_home.clone(),
                                on_back_to_street: on_back_to_street.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            SettingsScreen(
                                display_name: display_name.clone(),
                                threshold_k: threshold_k,
                                threshold_n: threshold_n,
                                contact_count: contacts.len(),
                                devices: devices.clone(),
                                mfa_policy: mfa_policy,
                                view: settings_props.clone(),
                                on_update_mfa: on_update_mfa.clone(),
                                on_update_nickname: on_update_nickname.clone(),
                                on_update_threshold: on_update_threshold.clone(),
                                on_add_device: on_add_device.clone(),
                                on_remove_device: on_remove_device.clone(),
                            )
                        }
                    }],
                    Screen::Recovery => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            RecoveryScreen(
                                guardians: guardians.clone(),
                                threshold_required: threshold_k as u32,
                                threshold_total: threshold_n as u32,
                                recovery_status: recovery_status.clone(),
                                pending_requests: pending_requests.clone(),
                                view: recovery_props.clone(),
                                on_start_recovery: on_start_recovery.clone(),
                                on_add_guardian: on_add_guardian.clone(),
                                on_submit_approval: on_submit_approval.clone(),
                            )
                        }
                    }],
                })
            }

            // Footer with key hints (3 rows)
            Footer(hints: screen_hints.clone(), global_hints: global_hints.clone(), disabled: is_insert_mode)

            // === GLOBAL MODALS (using ModalFrame overlay) ===
            // Account setup modal (queue-based only - legacy state deprecated)
            #(if let Some((display_name_prop, creating_prop, show_spinner_prop, success_prop, error_prop)) = account_setup_props.clone() {
                Some(element! {
                    ModalFrame {
                        AccountSetupModal(
                            visible: true,
                            display_name: display_name_prop,
                            focused: true,
                            creating: creating_prop,
                            show_spinner: show_spinner_prop,
                            success: success_prop,
                            error: error_prop,
                        )
                    }
                }.into_any())
            } else {
                None
            })

            // Guardian selection modal (queue-based)
            #(if let Some(ref state) = queue_guardian_select {
                let contacts_as_objects: Vec<Contact> = state.contacts.iter()
                    .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                    .collect();
                Some(element! {
                    ModalFrame {
                        ContactSelectModal(
                            visible: true,
                            title: state.title.clone(),
                            contacts: contacts_as_objects,
                            selected_index: state.selected_index,
                            error: String::new(),
                        )
                    }
                }.into_any())
            } else if guardian_modal_visible {
                // Guardian selection modal (legacy)
                Some(element! {
                    ModalFrame {
                        ContactSelectModal(
                            visible: true,
                            title: guardian_modal_title.clone(),
                            contacts: guardian_modal_contacts.clone(),
                            selected_index: guardian_modal_selected,
                            error: guardian_modal_error.clone(),
                        )
                    }
                }.into_any())
            } else {
                None
            })

            // Help modal
            #(if let Some(ref screen_name) = queue_help {
                Some(element! {
                    ModalFrame {
                        HelpModal(visible: true, current_screen: Some(screen_name.name().to_string()))
                    }
                }.into_any())
            } else if help_modal_visible {
                Some(element! {
                    ModalFrame {
                        HelpModal(visible: true, current_screen: Some(current_screen.name().to_string()))
                    }
                }.into_any())
            } else {
                None
            })

            // === SCREEN-SPECIFIC MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_petname_modal(&contacts_props))
            #(render_contacts_import_modal(&contacts_props))
            #(render_contacts_create_modal(&contacts_props))
            #(render_guardian_setup_modal(&contacts_props))

            // === CHAT SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_chat_create_modal(&chat_props))
            #(render_topic_modal(&chat_props))
            #(render_channel_info_modal(&chat_props))

            // === SETTINGS SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_nickname_modal(&settings_props))
            #(render_threshold_modal(&settings_props, threshold_k))
            #(render_add_device_modal(&settings_props))
            #(render_remove_device_modal(&settings_props))

            // === BLOCK SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_block_invite_modal(&block_props, &contacts))

            // === INVITATIONS SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_invitations_create_modal(&invitations_props))
            #(render_invitation_code_modal(&invitations_props))
            #(render_invitations_import_modal(&invitations_props, props.demo_mode))

            // === TOAST OVERLAY ===
            // Toast notifications overlay the footer when active
            // All toasts now go through the queue system (type-enforced single toast at a time)
            #(if let Some(ref toast) = queued_toast {
                Some(element! {
                    ToastContainer(toasts: vec![ToastMessage {
                        id: toast.id.to_string(),
                        message: toast.message.clone(),
                        level: match toast.level {
                            crate::tui::state_machine::ToastLevel::Info => ToastLevel::Info,
                            crate::tui::state_machine::ToastLevel::Success => ToastLevel::Success,
                            crate::tui::state_machine::ToastLevel::Warning => ToastLevel::Warning,
                            crate::tui::state_machine::ToastLevel::Error => ToastLevel::Error,
                        },
                    }])
                })
            } else {
                None
            })
        }
    }
}

/// Run the application with IoContext (real data)
///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<()> {
    // Create the UI update channel for reactive updates
    let (update_tx, update_rx) = ui_update_channel();
    let update_rx_holder = Arc::new(Mutex::new(Some(update_rx)));

    // Create effect dispatch callbacks using CallbackRegistry
    let ctx_arc = Arc::new(ctx);
    let app_core = ctx_arc.app_core().clone();
    let callbacks = CallbackRegistry::new(ctx_arc.clone(), update_tx.clone(), app_core);

    // Create CallbackContext for providing callbacks to components via iocraft context
    let callback_context = CallbackContext::new(callbacks.clone());

    // Check if account already exists to determine if we show setup modal
    let show_account_setup = !ctx_arc.has_account();

    // ========================================================================
    // Reactive Pattern: All data is provided via signals, not polling
    // ========================================================================
    // Screens subscribe to their respective signals and update reactively:
    // - ChatScreen subscribes to CHAT_SIGNAL
    // - RecoveryScreen subscribes to RECOVERY_SIGNAL
    // - InvitationsScreen subscribes to INVITATIONS_SIGNAL
    // - ContactsScreen subscribes to CONTACTS_SIGNAL + DISCOVERED_PEERS_SIGNAL
    // - BlockScreen subscribes to BLOCK_SIGNAL
    // - NeighborhoodScreen subscribes to NEIGHBORHOOD_SIGNAL
    // - SettingsScreen subscribes to SETTINGS_SIGNAL
    //
    // Props passed below are ONLY used as empty/default initial values.
    // Screens ignore these and use signal data immediately on mount.

    let channels = Vec::new();
    let messages = Vec::new();
    let guardians = Vec::new();
    let recovery_status = RecoveryStatus::default();
    let invitations = Vec::new();
    let contacts = Vec::new();
    let residents = Vec::new();
    let block_budget = BlockBudget::default();
    let discovered_peers: Vec<DiscoveredPeerInfo> = Vec::new();

    // Block and neighborhood data - reactively updated via signals
    let block_name = String::from("My Block");
    let channel_name = String::from("general");
    let neighborhood_name = String::from("Neighborhood");
    let blocks: Vec<BlockSummary> = Vec::new();

    // Settings data - reactively updated via SETTINGS_SIGNAL
    let devices = Vec::new();
    let display_name = String::new();
    let threshold_k = 0;
    let threshold_n = 0;

    // Get sync status for status bar display
    let sync_in_progress = ctx_arc.is_syncing().await;
    let last_sync_time = ctx_arc.last_sync_time().await;
    let peer_count = ctx_arc.known_peers_count().await;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());

    // Wrap the app in nested ContextProviders
    // This enables components to use:
    // - `hooks.use_context::<AppCoreContext>()` for reactive signal subscription
    // - `hooks.use_context::<CallbackContext>()` for accessing domain callbacks
    {
        // Prevent any stray stdout/stderr writes while iocraft is in fullscreen.
        // This avoids terminal scroll artifacts (e.g., "duplicated" nav bar).
        let _stdio_guard = if std::env::var_os("AURA_TUI_ALLOW_STDIO").is_some() {
            None
        } else {
            Some(crate::tui::fullscreen_stdio::FullscreenStdioGuard::redirect_stderr_to_null()?)
        };

        let app_context = app_core_context;
        let cb_context = callback_context;
        element! {
            ContextProvider(value: Context::owned(app_context)) {
                ContextProvider(value: Context::owned(cb_context)) {
                    IoApp(
                    // Chat screen data
                    channels: channels,
                    messages: messages,
                    // Recovery screen data
                    invitations: invitations,
                    guardians: guardians,
                    pending_requests: Vec::new(), // Populated reactively in RecoveryScreen
                    recovery_status: recovery_status,
                    // Settings screen data
                    devices: devices,
                    display_name: display_name,
                    threshold_k: threshold_k,
                    threshold_n: threshold_n,
                    mfa_policy: MfaPolicy::SensitiveOnly,
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
                    // Account setup
                    show_account_setup: show_account_setup,
                    // Sync status
                    sync_in_progress: sync_in_progress,
                    last_sync_time: last_sync_time,
                    peer_count: peer_count,
                    // Demo mode (get from context)
                    demo_mode: ctx_arc.is_demo_mode(),
                    demo_alice_code: ctx_arc.demo_alice_code(),
                    demo_carol_code: ctx_arc.demo_carol_code(),
                    // Reactive update channel
                    update_rx: Some(update_rx_holder),
                    update_tx: Some(update_tx.clone()),
                    // Callbacks registry
                    callbacks: Some(callbacks),
                )
                }
            }
        }
        .fullscreen()
        .await
    }
}

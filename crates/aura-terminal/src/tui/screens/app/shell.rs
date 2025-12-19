//! # App Shell
//!
//! Main application shell with screen navigation and modal management.
//!
//! This is the root TUI component that coordinates all screens, handles
//! events, manages the state machine, and renders modals.

use super::modal_overlays::{
    render_account_setup_modal, render_add_device_modal, render_block_invite_modal,
    render_channel_info_modal, render_chat_create_modal, render_confirm_modal,
    render_contact_modal, render_contacts_create_modal, render_contacts_import_modal,
    render_display_name_modal, render_guardian_modal, render_guardian_setup_modal,
    render_help_modal, render_invitation_code_modal, render_invitations_create_modal,
    render_invitations_import_modal, render_nickname_modal, render_remove_device_modal,
    render_threshold_modal, render_topic_modal, GlobalModalProps,
};

use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::signal_defs::SETTINGS_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::layout::dim;
use crate::tui::screens::app::subscriptions::{use_contacts_subscription, use_nav_status_signals};
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
    #[cfg(feature = "development")]
    pub demo_mode: bool,
    /// Alice's invite code (for demo mode)
    #[cfg(feature = "development")]
    pub demo_alice_code: String,
    /// Carol's invite code (for demo mode)
    #[cfg(feature = "development")]
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
    #[cfg(feature = "development")]
    let demo_alice = props.demo_alice_code.clone();
    #[cfg(feature = "development")]
    let demo_carol = props.demo_carol_code.clone();
    let tui_state = hooks.use_ref(move || {
        #[cfg(feature = "development")]
        {
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
        }

        #[cfg(not(feature = "development"))]
        {
            if show_setup {
                TuiState::with_account_setup()
            } else {
                TuiState::new()
            }
        }
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

    // Display name state - State<T> automatically triggers re-renders on .set()
    let display_name_state = hooks.use_state({
        let initial = props.display_name.clone();
        move || initial
    });

    // Get AppCoreContext for IoContext access
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // =========================================================================
    // NavBar status: derive from reactive signals (no blocking awaits at startup).
    // =========================================================================
    let nav_signals = use_nav_status_signals(
        &mut hooks,
        &app_ctx,
        props.sync_in_progress,
        props.peer_count,
        props.last_sync_time,
    );

    // =========================================================================
    // Contacts subscription: SharedContacts for dispatch handlers to read
    // =========================================================================
    // Unlike props.contacts (which is empty), this Arc is kept up-to-date
    // by a reactive subscription. Dispatch handler closures capture the Arc,
    // not the data, so they always read current contacts.
    let shared_contacts = use_contacts_subscription(&mut hooks, &app_ctx);

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
                                // Update queue-based state
                                tui_state.write().modal_queue.update_active(|modal| {
                                    if let QueuedModal::AccountSetup(ref mut s) = modal {
                                        s.set_error(error.clone());
                                    }
                                });
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

    let on_update_nickname = callbacks
        .as_ref()
        .map(|cb| cb.contacts.on_update_nickname.clone());
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
    let on_update_display_name = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_display_name.clone());
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

    #[cfg(feature = "development")]
    let demo_mode = props.demo_mode;
    #[cfg(not(feature = "development"))]
    let demo_mode = false;

    // =========================================================================
    // Global modal overlays
    // =========================================================================
    let mut global_modals = GlobalModalProps::default();
    global_modals.current_screen_name = current_screen.name().to_string();

    if let Some(modal) = tui_state.read().modal_queue.current() {
        match modal {
            QueuedModal::AccountSetup(state) => {
                global_modals.account_setup_visible = true;
                global_modals.account_setup_display_name = state.display_name.clone();
                global_modals.account_setup_creating = state.creating;
                global_modals.account_setup_show_spinner = state.should_show_spinner();
                global_modals.account_setup_success = state.success;
                global_modals.account_setup_error = state.error.clone();
            }
            QueuedModal::GuardianSelect(state) => {
                global_modals.guardian_modal_visible = true;
                global_modals.guardian_modal_title = state.title.clone();
                global_modals.guardian_modal_contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                    .collect();
                global_modals.guardian_modal_selected = state.selected_index;
            }
            QueuedModal::ContactSelect(state) => {
                global_modals.contact_modal_visible = true;
                global_modals.contact_modal_title = state.title.clone();
                global_modals.contact_modal_contacts = state
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                    .collect();
                global_modals.contact_modal_selected = state.selected_index;
            }
            QueuedModal::Confirm {
                title,
                message,
                on_confirm: _,
            } => {
                global_modals.confirm_visible = true;
                global_modals.confirm_title = title.clone();
                global_modals.confirm_message = message.clone();
            }
            QueuedModal::Help { current_screen } => {
                global_modals.help_modal_visible = true;
                if let Some(help_screen) = current_screen {
                    global_modals.current_screen_name = help_screen.name().to_string();
                }
            }
            _ => {}
        }
    }

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
        // Clone AppCoreContext for ceremony operations
        let app_ctx_for_ceremony = app_ctx.clone();
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        // Clone callbacks registry for command dispatch
        let callbacks = callbacks.clone();
        // Clone shared contacts Arc for guardian setup dispatch
        // This Arc is updated by a reactive subscription, so reading from it
        // always gets current contacts (not stale props)
        let shared_contacts_for_dispatch = shared_contacts.clone();
        move |event| {
            // Convert iocraft event to aura-core event and run through state machine
            if let Some(core_event) = convert_iocraft_event(event.clone()) {
                // Get current state, apply transition, update state
                let current = tui_state.read().clone();
                let (mut new_state, commands) = transition(&current, core_event);

                // Sync TuiState changes to iocraft hooks
                if new_state.screen() != current.screen() {
                    screen.set(new_state.screen());
                }
                if new_state.should_exit && !current.should_exit {
                    should_exit.set(true);
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
                                    DispatchCommand::UpdateNickname {
                                        contact_id,
                                        nickname,
                                    } => {
                                        (cb.contacts.on_update_nickname)(contact_id, nickname);
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

                                    // === Guardian Setup Modal ===
                                    DispatchCommand::OpenGuardianSetup => {
                                        // Read current contacts from reactive subscription
                                        // This reads from SharedContacts Arc which is kept up-to-date
                                        // by a separate reactive subscription (not stale props)
                                        let current_contacts = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        // Populate candidates from current contacts
                                        let candidates: Vec<crate::tui::state_machine::GuardianCandidate> = current_contacts
                                            .iter()
                                            .map(|c| crate::tui::state_machine::GuardianCandidate {
                                                id: c.id.clone(),
                                                name: c.nickname.clone(),
                                                is_current_guardian: c.is_guardian,
                                            })
                                            .collect();

                                        // Pre-select existing guardians
                                        let selected: Vec<usize> = candidates
                                            .iter()
                                            .enumerate()
                                            .filter(|(_, c)| c.is_current_guardian)
                                            .map(|(i, _)| i)
                                            .collect();

                                        // Create populated modal state
                                        let mut modal_state = crate::tui::state_machine::GuardianSetupModalState::default();
                                        modal_state.contacts = candidates;
                                        modal_state.selected_indices = selected;

                                        // Enqueue the modal to new_state (not tui_state, which gets overwritten)
                                        new_state.modal_queue.enqueue(crate::tui::state_machine::QueuedModal::GuardianSetup(modal_state));
                                    }

                                    // === Guardian Ceremony Commands ===
                                    DispatchCommand::StartGuardianCeremony { contact_ids, threshold_k } => {
                                        tracing::info!(
                                            "Starting guardian ceremony with {} contacts, threshold {}",
                                            contact_ids.len(),
                                            threshold_k
                                        );

                                        let app_ctx = app_ctx_for_ceremony.clone();
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
                                                    app_ctx.add_info_toast(
                                                        "guardian-ceremony-started",
                                                        format!("Guardian ceremony started! Waiting for {}-of-{} guardians to respond", k, n)
                                                    ).await;

                                                    // Spawn a task to monitor ceremony progress and show completion
                                                    let app_core_monitor = app_core.clone();
                                                    let app_ctx_monitor = app_ctx.clone();
                                                    tokio::spawn(async move {
                                                        // Poll for ceremony completion (max 30 seconds)
                                                        for _ in 0..60 {
                                                            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                                                            let core = app_core_monitor.read().await;
                                                            if let Ok(status) = core.get_ceremony_status(&ceremony_id).await {
                                                                if status.is_complete {
                                                                    tracing::info!("Guardian ceremony completed successfully");
                                                                    app_ctx_monitor.add_success_toast(
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

                                                    app_ctx.add_error_toast(
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
                                    DispatchCommand::UpdateDisplayName { display_name } => {
                                        (cb.settings.on_update_display_name)(display_name);
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

                tui_state_version.set(tui_state_version.get().wrapping_add(1));
            }

            // All key events are handled by the state machine above.
            // Modal handling goes through transition() -> command execution.
        }
    });

    // Nav bar status is updated reactively from signals.
    let syncing = nav_signals.syncing.get();
    let last_sync = nav_signals.last_sync_time.get();
    let peers = nav_signals.peer_count.get();

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
                                on_update_nickname: on_update_nickname.clone(),
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
                                on_update_display_name: on_update_display_name.clone(),
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

            // === GLOBAL MODALS ===
            #(render_account_setup_modal(&global_modals))
            #(render_guardian_modal(&global_modals))
            #(render_contact_modal(&global_modals))
            #(render_confirm_modal(&global_modals))
            #(render_help_modal(&global_modals))

            // === SCREEN-SPECIFIC MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_nickname_modal(&contacts_props))
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
            #(render_display_name_modal(&settings_props))
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
            #(render_invitations_import_modal(&invitations_props, demo_mode))

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
    let display_name = {
        let core = ctx_arc.app_core().read().await;
        core.read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .display_name
    };
    let threshold_k = 0;
    let threshold_n = 0;

    // Status bar values are updated reactively after mount.
    // Avoid blocking before entering fullscreen (important for demo mode).
    let sync_in_progress = false;
    let last_sync_time: Option<u64> = None;
    let peer_count: usize = 0;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());

    // Wrap the app in nested ContextProviders
    // This enables components to use:
    // - `hooks.use_context::<AppCoreContext>()` for reactive signal subscription
    // - `hooks.use_context::<CallbackContext>()` for accessing domain callbacks
    {
        let app_context = app_core_context;
        let cb_context = callback_context;
        #[cfg(feature = "development")]
        let mut app = element! {
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
        };

        #[cfg(not(feature = "development"))]
        let mut app = element! {
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
                        // Reactive update channel
                        update_rx: Some(update_rx_holder),
                        update_tx: Some(update_tx.clone()),
                        // Callbacks registry
                        callbacks: Some(callbacks),
                    )
                }
            }
        };

        app.fullscreen().await
    }
}

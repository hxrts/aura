//! # App Shell
//!
//! Main application shell with screen navigation and modal management.
//!
//! This is the root TUI component that coordinates all screens, handles
//! events, manages the state machine, and renders modals.

// Allow field reassignment for large structs with many conditional fields
#![allow(clippy::field_reassign_with_default)]
// Allow manual map patterns in element! macro contexts for clarity
#![allow(clippy::manual_map)]

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

use aura_app::signal_defs::{ERROR_SIGNAL, SETTINGS_SIGNAL};
use aura_app::AppError;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::layout::dim;
use crate::tui::screens::app::subscriptions::{
    use_channels_subscription, use_contacts_subscription, use_invitations_subscription,
    use_messages_subscription, use_nav_status_signals, use_neighborhood_blocks_subscription,
    use_pending_requests_subscription, use_residents_subscription,
};
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    BlockScreen, ChatScreen, ContactsScreen, NeighborhoodScreen, RecoveryScreen, SettingsScreen,
};
use crate::tui::types::{
    BlockBudget, BlockSummary, Channel, Contact, Device, Guardian, Invitation, KeyHint, Message,
    MfaPolicy, PendingRequest, RecoveryStatus, Resident, TraversalDepth,
};

// State machine integration
use crate::tui::iocraft_adapter::convert_iocraft_event;
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

#[derive(Clone)]
struct TuiStateHandle {
    state: Ref<TuiState>,
    version: State<usize>,
}

impl TuiStateHandle {
    fn new(state: Ref<TuiState>, version: State<usize>) -> Self {
        Self { state, version }
    }

    fn bump(&mut self) {
        self.version.set(self.version.get().wrapping_add(1));
    }

    fn read_clone(&self) -> TuiState {
        self.state.read().clone()
    }

    fn with_mut<R>(&mut self, f: impl FnOnce(&mut TuiState) -> R) -> R {
        let mut guard = self.state.write();
        let out = f(&mut guard);
        drop(guard);
        self.bump();
        out
    }

    fn replace(&mut self, new_state: TuiState) {
        self.with_mut(|state| *state = new_state);
    }
}

#[allow(clippy::field_reassign_with_default)] // Large struct with many conditional fields
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
    let tui = TuiStateHandle::new(tui_state.clone(), tui_state_version.clone());

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

    // =========================================================================
    // Messages subscription: SharedMessages for dispatch handlers to read
    // =========================================================================
    // Used to look up failed messages by ID for retry operations.
    // The Arc is kept up-to-date by a reactive subscription to CHAT_SIGNAL.
    let shared_messages = use_messages_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Residents subscription: SharedResidents for dispatch handlers to read
    // =========================================================================
    let shared_residents = use_residents_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Channels subscription: SharedChannels for dispatch handlers to read
    // =========================================================================
    let shared_channels = use_channels_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Invitations subscription: SharedInvitations for dispatch handlers to read
    // =========================================================================
    let shared_invitations = use_invitations_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Neighborhood blocks subscription: SharedNeighborhoodBlocks for dispatch handlers to read
    // =========================================================================
    let shared_neighborhood_blocks = use_neighborhood_blocks_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Pending requests subscription: SharedPendingRequests for dispatch handlers to read
    // =========================================================================
    let shared_pending_requests = use_pending_requests_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // ERROR_SIGNAL subscription: central domain error surfacing
    // =========================================================================
    // Rule: AppCore/dispatch failures emit ERROR_SIGNAL (Option<AppError>) and are
    // rendered here (toast queue), so screens/callbacks do not need their own
    // per-operation error toasts.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut tui = tui.clone();
        async move {
            let format_error = |err: &AppError| {
                format!("{}: {}", err.code(), err)
            };

            // Initial read.
            {
                let core = app_core.raw().read().await;
                if let Ok(Some(err)) = core.read(&*ERROR_SIGNAL).await {
                    let msg = format_error(&err);
                    tui.with_mut(|state| {
                        // Prefer routing errors into the account setup modal when it is active.
                        let routed = matches!(
                            state.modal_queue.current(),
                            Some(QueuedModal::AccountSetup(_))
                        );
                        if routed {
                            state.modal_queue.update_active(|modal| {
                                if let QueuedModal::AccountSetup(ref mut s) = modal {
                                    s.set_error(msg.clone());
                                }
                            });
                        }

                        if !routed {
                            let toast_id = state.next_toast_id;
                            state.next_toast_id += 1;
                            let toast = crate::tui::state_machine::QueuedToast::new(
                                toast_id,
                                msg,
                                crate::tui::state_machine::ToastLevel::Error,
                            );
                            state.toast_queue.enqueue(toast);
                        }
                    });
                }
            }

            // Subscribe for updates.
            // IMPORTANT: never permanently stop listening. If the subscription stream
            // errors (e.g., closed/lost), retry with backoff instead of silently ending.
            let mut backoff = std::time::Duration::from_millis(50);
            loop {
                let mut stream = {
                    let core = app_core.raw().read().await;
                    core.subscribe(&*ERROR_SIGNAL)
                };

                while let Ok(err_opt) = stream.recv().await {
                    let Some(err) = err_opt else { continue };
                    let msg = format_error(&err);
                    tui.with_mut(|state| {
                        // Prefer routing errors into the account setup modal when it is active.
                        let routed = matches!(
                            state.modal_queue.current(),
                            Some(QueuedModal::AccountSetup(_))
                        );
                        if routed {
                            state.modal_queue.update_active(|modal| {
                                if let QueuedModal::AccountSetup(ref mut s) = modal {
                                    s.set_error(msg.clone());
                                }
                            });
                        }

                        if !routed {
                            let toast_id = state.next_toast_id;
                            state.next_toast_id += 1;
                            let toast = crate::tui::state_machine::QueuedToast::new(
                                toast_id,
                                msg,
                                crate::tui::state_machine::ToastLevel::Error,
                            );
                            state.toast_queue.enqueue(toast);
                        }
                    });

                    backoff = std::time::Duration::from_millis(50);
                }

                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(std::time::Duration::from_secs(2));
            }
        }
    });

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
            // Toast queue migration: mutate TuiState via TuiStateHandle (always bumps render version)
            let mut tui = tui.clone();
            async move {
                // Helper macro-like function to add a toast to the queue
                // (Inline to avoid borrow checker issues with closures)
                macro_rules! enqueue_toast {
                    ($msg:expr, $level:expr) => {{
                        tui.with_mut(|state| {
                            let toast_id = state.next_toast_id;
                            state.next_toast_id += 1;
                            let toast = crate::tui::state_machine::QueuedToast::new(
                                toast_id,
                                $msg,
                                $level,
                            );
                            state.toast_queue.enqueue(toast);
                        });
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
                    // IMPORTANT: This match is intentionally exhaustive (no `_ => {}`).
                    // Adding a new UiUpdate variant must cause a compile-time error here,
                    // so the shell cannot silently drop UI updates.
                    match update {
                        // =========================================================================
                        // Settings updates
                        // =========================================================================
                        UiUpdate::DisplayNameChanged(name) => {
                            display_name_state.set(name);
                        }
                        UiUpdate::MfaPolicyChanged(_policy) => {
                            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
                        }
                        UiUpdate::ThresholdChanged { k: _, n: _ } => {
                            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
                        }
                        UiUpdate::DeviceAdded(_device) => {
                            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
                        }
                        UiUpdate::DeviceRemoved { device_id: _ } => {
                            // Settings screen renders from SETTINGS_SIGNAL; no local state update.
                        }

                        // =========================================================================
                        // Toast notifications
                        // =========================================================================
                        UiUpdate::ToastAdded(toast) => {
                            // Convert ToastMessage to QueuedToast and enqueue.
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
                            // Dismiss from queue (FIFO, ignores ID).
                            tui.with_mut(|state| {
                                state.toast_queue.dismiss();
                            });
                        }
                        UiUpdate::ToastsCleared => {
                            tui.with_mut(|state| {
                                state.toast_queue.clear();
                            });
                        }

                        // =========================================================================
                        // Chat / messaging
                        // =========================================================================
                        UiUpdate::MessageSent { channel, .. } => {
                            enqueue_toast!(
                                format!("Message sent to {}", channel),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }
                        UiUpdate::MessageRetried { message_id: _ } => {
                            enqueue_toast!(
                                "Retrying message…".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }
                        UiUpdate::ChannelSelected(_) => {
                            // Navigation/state machine owns selected channel.
                        }
                        UiUpdate::ChannelCreated(_) => {
                            // CHAT_SIGNAL should reflect the new channel; no extra work.
                        }
                        UiUpdate::TopicSet {
                            channel: _,
                            topic: _,
                        } => {
                            // CHAT_SIGNAL should reflect updated topic; no extra work.
                        }

                        // =========================================================================
                        // Invitations
                        // =========================================================================
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
                        UiUpdate::InvitationExported { code: _ } => {
                            // The code is surfaced via a dedicated modal/clipboard path.
                        }
                        UiUpdate::InvitationImported { invitation_code: _ } => {
                            enqueue_toast!(
                                "Invitation imported".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        // =========================================================================
                        // Navigation
                        // =========================================================================
                        UiUpdate::BlockEntered { block_id: _ } => {
                            // Navigation/state machine owns the current block selection.
                        }
                        UiUpdate::NavigatedHome => {
                            // Navigation/state machine handles this.
                        }
                        UiUpdate::NavigatedToStreet => {
                            // Navigation/state machine handles this.
                        }
                        UiUpdate::NavigatedToNeighborhood => {
                            // Navigation/state machine handles this.
                        }

                        // =========================================================================
                        // Recovery
                        // =========================================================================
                        UiUpdate::RecoveryStarted => {
                            enqueue_toast!(
                                "Recovery process started".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }
                        UiUpdate::GuardianAdded { contact_id: _ } => {
                            // RECOVERY_SIGNAL owns guardian state; no local state update.
                        }
                        UiUpdate::GuardianSelected { contact_id: _ } => {
                            // RECOVERY_SIGNAL owns guardian state; no local state update.
                        }
                        UiUpdate::ApprovalSubmitted { request_id: _ } => {
                            enqueue_toast!(
                                "Approval submitted".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::GuardianCeremonyProgress { step: _ } => {
                            // Deprecated in favor of `GuardianCeremonyStatus`.
                        }
                        UiUpdate::GuardianCeremonyStatus {
                            ceremony_id,
                            accepted_guardians,
                            total_count,
                            threshold,
                            is_complete,
                            has_failed,
                            error_message,
                            pending_epoch,
                        } => {
                            let mut toast: Option<(String, crate::tui::state_machine::ToastLevel)> =
                                None;

                            tui.with_mut(|state| {
                                let mut dismiss_modal = false;

                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state_machine::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step,
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            if s.ceremony_id.is_none() {
                                                s.set_ceremony_id(ceremony_id.clone());
                                            }

                                            if let Some(epoch) = pending_epoch {
                                                s.pending_epoch = Some(epoch);
                                            }

                                            for (id, _name, response) in &mut s.ceremony_responses {
                                                if accepted_guardians.iter().any(|g| g == id) {
                                                    *response = crate::tui::state_machine::GuardianCeremonyResponse::Accepted;
                                                } else if matches!(
                                                    response,
                                                    crate::tui::state_machine::GuardianCeremonyResponse::Accepted
                                                ) {
                                                    *response = crate::tui::state_machine::GuardianCeremonyResponse::Pending;
                                                }
                                            }

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Guardian ceremony failed".to_string());
                                                s.error = Some(msg.clone());

                                                // Return to threshold selection so the user can retry.
                                                s.step = crate::tui::state_machine::GuardianSetupStep::ChooseThreshold;
                                                s.ceremony_id = None;
                                                s.pending_epoch = None;
                                                s.ceremony_responses.clear();

                                                toast = Some((
                                                    msg,
                                                    crate::tui::state_machine::ToastLevel::Error,
                                                ));
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Guardian ceremony complete! {}-of-{} committed",
                                                        threshold, total_count
                                                    ),
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                            }
                                        }
                                    }
                                });

                                if dismiss_modal {
                                    state.modal_queue.dismiss();
                                }
                            });

                            if let Some((msg, level)) = toast {
                                enqueue_toast!(msg, level);
                            }
                        }

                        // =========================================================================
                        // Contacts
                        // =========================================================================
                        UiUpdate::NicknameUpdated {
                            contact_id: _,
                            nickname: _,
                        } => {
                            // CONTACTS_SIGNAL owns contact data; no local state update.
                        }
                        UiUpdate::ChatStarted { contact_id: _ } => {
                            // Navigation/state machine handles screen changes.
                        }
                        UiUpdate::LanPeerInvited { peer_id: _ } => {
                            enqueue_toast!(
                                "LAN peer invited".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        // =========================================================================
                        // Block operations
                        // =========================================================================
                        UiUpdate::BlockMessageSent {
                            block_id: _,
                            content: _,
                        } => {
                            enqueue_toast!(
                                "Block message sent".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::BlockInviteSent { contact_id: _ } => {
                            enqueue_toast!(
                                "Invite sent".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::StewardGranted { contact_id: _ } => {
                            enqueue_toast!(
                                "Steward granted".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::StewardRevoked { contact_id: _ } => {
                            enqueue_toast!(
                                "Steward revoked".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }

                        // =========================================================================
                        // Account
                        // =========================================================================
                        UiUpdate::AccountCreated => {
                            // Update the account setup modal to show success screen.
                            tui.with_mut(|state| {
                                state.account_created_queued();
                            });
                        }

                        // =========================================================================
                        // Sync
                        // =========================================================================
                        UiUpdate::SyncStarted => {
                            enqueue_toast!(
                                "Syncing…".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
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

                        // =========================================================================
                        // UI-only errors (domain/runtime errors use ERROR_SIGNAL)
                        // =========================================================================
                        UiUpdate::OperationFailed { operation, error } => {
                            // For account creation, show error in the modal instead of toast.
                            if operation == "CreateAccount" {
                                tui.with_mut(|state| {
                                    state.modal_queue.update_active(|modal| {
                                        if let QueuedModal::AccountSetup(ref mut s) = modal {
                                            s.set_error(error.clone());
                                        }
                                    });
                                });
                            } else {
                                enqueue_toast!(
                                    format!("{} failed: {}", operation, error),
                                    crate::tui::state_machine::ToastLevel::Error
                                );
                            }
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
    let block_invite_contacts: Vec<Contact> = match tui_state.read().modal_queue.current() {
        Some(QueuedModal::BlockInvite(state)) => state
            .contacts
            .iter()
            .map(|(id, name)| Contact::new(id.clone(), name.clone()))
            .collect(),
        _ => Vec::new(),
    };
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
        KeyHint::new("↑↓←→", "Nav"),
        KeyHint::new("Tab", "Next"),
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
        let mut tui = tui.clone();
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        // Clone update channel sender for ceremony UI updates
        let update_tx_for_ceremony = props.update_tx.clone();
        // Clone callbacks registry for command dispatch
        let callbacks = callbacks.clone();
        // Clone shared contacts Arc for guardian setup dispatch
        let shared_channels_for_dispatch = shared_channels.clone();
        let shared_invitations_for_dispatch = shared_invitations.clone();
        let shared_neighborhood_blocks_for_dispatch = shared_neighborhood_blocks.clone();
        let shared_pending_requests_for_dispatch = shared_pending_requests.clone();
        // This Arc is updated by a reactive subscription, so reading from it
        // always gets current contacts (not stale props)
        let shared_contacts_for_dispatch = shared_contacts.clone();
        // Clone shared messages Arc for message retry dispatch
        // Clone shared residents Arc for block moderation dispatch
        // Used to map selected resident index -> resident ID without placeholders
        let shared_residents_for_dispatch = shared_residents.clone();
        // Used to look up failed messages by ID to get channel and content for retry
        let shared_messages_for_dispatch = shared_messages.clone();
        move |event| {
            // Convert iocraft event to aura-core event and run through state machine
            if let Some(core_event) = convert_iocraft_event(event.clone()) {
                // Get current state, apply transition, update state
                let current = tui.read_clone();
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
                                    DispatchCommand::GrantStewardSelected => {
                                        let idx = new_state.block.selected_resident;
                                        if let Ok(guard) = shared_residents_for_dispatch.read() {
                                            if let Some(resident) = guard.get(idx) {
                                                (cb.block.on_grant_steward)(resident.id.clone());
                                            } else {
                                                new_state.toast_error("No resident selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read residents");
                                        }
                                    }
                                    DispatchCommand::RevokeStewardSelected => {
                                        let idx = new_state.block.selected_resident;
                                        if let Ok(guard) = shared_residents_for_dispatch.read() {
                                            if let Some(resident) = guard.get(idx) {
                                                (cb.block.on_revoke_steward)(resident.id.clone());
                                            } else {
                                                new_state.toast_error("No resident selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read residents");
                                        }
                                    }

                                    // === Chat Screen Commands ===
                                    DispatchCommand::SelectChannel { channel_id } => {
                                        (cb.chat.on_channel_select)(channel_id);
                                    }
                                    DispatchCommand::SendChatMessage { content } => {
                                        let idx = new_state.chat.selected_channel;
                                        if let Ok(guard) = shared_channels_for_dispatch.read() {
                                            if let Some(channel) = guard.get(idx) {
                                                (cb.chat.on_send)(channel.id.clone(), content);
                                            } else {
                                                new_state.toast_error("No channel selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read channels");
                                        }
                                    }
                                    DispatchCommand::RetryMessage => {
                                        let idx = new_state.chat.message_scroll;
                                        if let Ok(guard) = shared_messages_for_dispatch.read() {
                                            if let Some(msg) = guard.get(idx) {
                                                (cb.chat.on_retry_message)(
                                                    msg.id.clone(),
                                                    msg.channel_id.clone(),
                                                    msg.content.clone(),
                                                );
                                            } else {
                                                new_state.toast_error("No message selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read messages");
                                        }
                                    }
                                    DispatchCommand::OpenChatTopicModal => {
                                        let idx = new_state.chat.selected_channel;
                                        if let Ok(guard) = shared_channels_for_dispatch.read() {
                                            if let Some(channel) = guard.get(idx) {
                                                let modal_state = crate::tui::state_machine::TopicModalState::for_channel(
                                                    &channel.id,
                                                    channel.topic.as_deref().unwrap_or(""),
                                                );
                                                new_state
                                                    .modal_queue
                                                    .enqueue(crate::tui::state_machine::QueuedModal::ChatTopic(
                                                        modal_state,
                                                    ));
                                            } else {
                                                new_state.toast_error("No channel selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read channels");
                                        }
                                    }
                                    DispatchCommand::OpenChatInfoModal => {
                                        let idx = new_state.chat.selected_channel;
                                        if let Ok(guard) = shared_channels_for_dispatch.read() {
                                            if let Some(channel) = guard.get(idx) {
                                                let modal_state = crate::tui::state_machine::ChannelInfoModalState::for_channel(
                                                    &channel.id,
                                                    &channel.name,
                                                    channel.topic.as_deref(),
                                                );
                                                new_state
                                                    .modal_queue
                                                    .enqueue(crate::tui::state_machine::QueuedModal::ChatInfo(
                                                        modal_state,
                                                    ));
                                            } else {
                                                new_state.toast_error("No channel selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read channels");
                                        }
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
                                    DispatchCommand::StartChat => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            if let Some(contact) = guard.get(idx) {
                                                (cb.contacts.on_start_chat)(contact.id.clone());
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
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
                                        new_state.modal_queue.dismiss();
                                    }

                                    // === Invitations Screen Commands ===
                                    DispatchCommand::AcceptInvitation => {
                                        let idx = new_state.invitations.selected_index;
                                        let filter = new_state.invitations.filter;

                                        if let Ok(guard) = shared_invitations_for_dispatch.read() {
                                            let mut selected: Option<String> = None;
                                            let mut seen = 0usize;
                                            for inv in guard.iter() {
                                                let include = match filter {
                                                    crate::tui::types::InvitationFilter::All => true,
                                                    crate::tui::types::InvitationFilter::Sent => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Outbound
                                                    }
                                                    crate::tui::types::InvitationFilter::Received => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Inbound
                                                    }
                                                };
                                                if !include {
                                                    continue;
                                                }
                                                if seen == idx {
                                                    selected = Some(inv.id.clone());
                                                    break;
                                                }
                                                seen += 1;
                                            }

                                            if let Some(inv_id) = selected {
                                                (cb.invitations.on_accept)(inv_id);
                                            } else {
                                                new_state.toast_error("No invitation selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read invitations");
                                        }
                                    }
                                    DispatchCommand::DeclineInvitation => {
                                        let idx = new_state.invitations.selected_index;
                                        let filter = new_state.invitations.filter;

                                        if let Ok(guard) = shared_invitations_for_dispatch.read() {
                                            let mut selected: Option<String> = None;
                                            let mut seen = 0usize;
                                            for inv in guard.iter() {
                                                let include = match filter {
                                                    crate::tui::types::InvitationFilter::All => true,
                                                    crate::tui::types::InvitationFilter::Sent => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Outbound
                                                    }
                                                    crate::tui::types::InvitationFilter::Received => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Inbound
                                                    }
                                                };
                                                if !include {
                                                    continue;
                                                }
                                                if seen == idx {
                                                    selected = Some(inv.id.clone());
                                                    break;
                                                }
                                                seen += 1;
                                            }

                                            if let Some(inv_id) = selected {
                                                (cb.invitations.on_decline)(inv_id);
                                            } else {
                                                new_state.toast_error("No invitation selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read invitations");
                                        }
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
                                    DispatchCommand::ExportInvitation => {
                                        let idx = new_state.invitations.selected_index;
                                        let filter = new_state.invitations.filter;

                                        if let Ok(guard) = shared_invitations_for_dispatch.read() {
                                            let mut selected: Option<String> = None;
                                            let mut seen = 0usize;
                                            for inv in guard.iter() {
                                                let include = match filter {
                                                    crate::tui::types::InvitationFilter::All => true,
                                                    crate::tui::types::InvitationFilter::Sent => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Outbound
                                                    }
                                                    crate::tui::types::InvitationFilter::Received => {
                                                        inv.direction == crate::tui::types::InvitationDirection::Inbound
                                                    }
                                                };
                                                if !include {
                                                    continue;
                                                }
                                                if seen == idx {
                                                    selected = Some(inv.id.clone());
                                                    break;
                                                }
                                                seen += 1;
                                            }

                                            if let Some(inv_id) = selected {
                                                (cb.invitations.on_export)(inv_id);
                                            } else {
                                                new_state.toast_error("No invitation selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read invitations");
                                        }
                                    }
                                    DispatchCommand::RevokeInvitation { invitation_id } => {
                                        // TODO: Implement invitation revocation callback
                                        tracing::info!("Revoke invitation requested: {}", invitation_id);
                                    }

                                    // === Recovery Screen Commands ===
                                    DispatchCommand::StartRecovery => {
                                        (cb.recovery.on_start_recovery)();
                                    }
                                    DispatchCommand::ApproveRecovery => {
                                        let idx = new_state.recovery.selected_index;
                                        if let Ok(guard) = shared_pending_requests_for_dispatch.read() {
                                            if let Some(req) = guard.get(idx) {
                                                (cb.recovery.on_submit_approval)(req.id.clone());
                                            } else {
                                                new_state.toast_error("No request selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read requests");
                                        }
                                    }

                                    // === Block Invite Modal ===
                                    DispatchCommand::OpenBlockInvite => {
                                        let current_contacts = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        let contacts: Vec<(String, String)> = current_contacts
                                            .iter()
                                            .map(|c| (c.id.clone(), c.nickname.clone()))
                                            .collect();

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::BlockInvite(
                                                crate::tui::state_machine::ContactSelectModalState::single(
                                                    "Invite to Block",
                                                    contacts,
                                                ),
                                            ),
                                        );
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

                                        let ids = contact_ids.clone();
                                        let n = contact_ids.len() as u16;
                                        let k = threshold_k as u16;

                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();

                                        tokio::spawn(async move {
                                            let core = app_core.raw().read().await;

                                            match core
                                                .initiate_guardian_ceremony(k, n, &ids)
                                                .await
                                            {
                                                Ok(ceremony_id) => {
                                                    tracing::info!(
                                                        ceremony_id = ?ceremony_id,
                                                        threshold = k,
                                                        guardians = n,
                                                        "Guardian ceremony initiated, waiting for guardian responses"
                                                    );

                                                    if let Some(tx) = update_tx.clone() {
                                                        let _ = tx.send(UiUpdate::ToastAdded(ToastMessage::info(
                                                            "guardian-ceremony-started",
                                                            format!(
                                                                "Guardian ceremony started! Waiting for {}-of-{} guardians to respond",
                                                                k, n
                                                            ),
                                                        )));

                                                        // Prime the modal with an initial status update so `ceremony_id` is
                                                        // available immediately for UI cancel.
                                                        let _ = tx.send(UiUpdate::GuardianCeremonyStatus {
                                                            ceremony_id: ceremony_id.clone(),
                                                            accepted_guardians: Vec::new(),
                                                            total_count: n,
                                                            threshold: k,
                                                            is_complete: false,
                                                            has_failed: false,
                                                            error_message: None,
                                                            pending_epoch: None,
                                                        });
                                                    }

                                                    // Spawn a task to monitor ceremony progress.
                                                    let app_core_monitor = app_core.clone();
                                                    let update_tx_monitor = update_tx.clone();
                                                    tokio::spawn(async move {
                                                        for _ in 0..60 {
                                                            tokio::time::sleep(tokio::time::Duration::from_millis(500))
                                                                .await;

                                                            let core = app_core_monitor.raw().read().await;
                                                            if let Ok(status) =
                                                                core.get_ceremony_status(&ceremony_id).await
                                                            {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.send(UiUpdate::GuardianCeremonyStatus {
                                                                        ceremony_id: status.ceremony_id.clone(),
                                                                        accepted_guardians: status.accepted_guardians.clone(),
                                                                        total_count: status.total_count,
                                                                        threshold: status.threshold,
                                                                        is_complete: status.is_complete,
                                                                        has_failed: status.has_failed,
                                                                        error_message: status.error_message.clone(),
                                                                        pending_epoch: status.pending_epoch,
                                                                    });
                                                                }

                                                                if status.has_failed {
                                                                    // Roll back the pending key rotation if epoch was created.
                                                                    if let Some(pending_epoch) = status.pending_epoch {
                                                                        tracing::info!(
                                                                            pending_epoch,
                                                                            "Rolling back failed key rotation"
                                                                        );
                                                                        if let Err(e) = core
                                                                            .rollback_guardian_key_rotation(pending_epoch)
                                                                            .await
                                                                        {
                                                                            tracing::error!(
                                                                                "Failed to rollback key rotation: {}",
                                                                                e
                                                                            );
                                                                        }
                                                                    }
                                                                    break;
                                                                }

                                                                if status.is_complete {
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Failed to initiate guardian ceremony: {}",
                                                        e
                                                    );

                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.send(UiUpdate::operation_failed(
                                                            "Guardian ceremony",
                                                            e.to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    DispatchCommand::CancelGuardianCeremony { ceremony_id } => {
                                        tracing::info!(ceremony_id = %ceremony_id, "Canceling guardian ceremony");

                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();

                                        tokio::spawn(async move {
                                            let core = app_core.raw().read().await;

                                            // Get ceremony status to retrieve the pending epoch.
                                            if let Ok(status) = core.get_ceremony_status(&ceremony_id).await {
                                                if let Some(pending_epoch) = status.pending_epoch {
                                                    tracing::info!(
                                                        pending_epoch,
                                                        "Rolling back canceled key rotation"
                                                    );

                                                    if let Err(e) =
                                                        core.rollback_guardian_key_rotation(pending_epoch).await
                                                    {
                                                        tracing::error!(
                                                            "Failed to rollback key rotation: {}",
                                                            e
                                                        );

                                                        if let Some(tx) = update_tx.clone() {
                                                            let _ = tx.send(UiUpdate::operation_failed(
                                                                "Cancel guardian ceremony",
                                                                e.to_string(),
                                                            ));
                                                        }
                                                        return;
                                                    }
                                                }
                                            }

                                            if let Some(tx) = update_tx {
                                                let _ = tx.send(UiUpdate::ToastAdded(ToastMessage::info(
                                                    "guardian-ceremony-canceled",
                                                    "Guardian ceremony canceled",
                                                )));
                                            }
                                        });
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
                                    DispatchCommand::EnterBlock => {
                                        let idx = new_state.neighborhood.grid.current();
                                        if let Ok(guard) = shared_neighborhood_blocks_for_dispatch.read() {
                                            if let Some(block_id) = guard.get(idx) {
                                                // Default to Street-level traversal depth
                                                (cb.neighborhood.on_enter_block)(
                                                    block_id.clone(),
                                                    TraversalDepth::default(),
                                                );
                                            } else {
                                                new_state.toast_error("No block selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read neighborhood blocks");
                                        }
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
                                // Apply UI-only effects to the next state (which is what we persist).
                                let toast_id = new_state.next_toast_id;
                                new_state.next_toast_id += 1;
                                let toast = crate::tui::state_machine::QueuedToast::new(
                                    toast_id,
                                    message,
                                    level,
                                );
                                new_state.toast_queue.enqueue(toast);
                            }
                            TuiCommand::DismissToast { id: _ } => {
                                // Dismiss current toast from queue (ignores ID - FIFO semantics)
                                new_state.toast_queue.dismiss();
                            }
                            TuiCommand::ClearAllToasts => {
                                // Clear all toasts from queue
                                new_state.toast_queue.clear();
                            }
                            TuiCommand::Render => {
                                // Render is handled by iocraft automatically
                            }
                        }
                    }
                }

                // Update TuiState (and always bump render version)
                tui.replace(new_state);
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
            // Nav bar area (2 rows) - tabs + border
            NavBar(
                active_screen: current_screen,
            )

            // Middle content area (26 rows) - always renders screen content
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

            // Footer with key hints and status (3 rows)
            Footer(
                hints: screen_hints.clone(),
                global_hints: global_hints.clone(),
                disabled: is_insert_mode,
                syncing: syncing,
                last_sync_time: last_sync,
                peer_count: peers,
            )

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
            #(render_block_invite_modal(&block_props, &block_invite_contacts))

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
    let app_core = ctx_arc.app_core_raw().clone();
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
        let core = ctx_arc.app_core_raw().read().await;
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

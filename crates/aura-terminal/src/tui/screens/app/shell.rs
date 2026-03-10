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
    render_access_override_modal, render_account_setup_modal, render_add_device_modal,
    render_capability_config_modal, render_channel_info_modal, render_chat_create_modal,
    render_confirm_modal, render_contact_modal, render_contacts_code_modal,
    render_contacts_create_modal, render_contacts_import_modal, render_device_enrollment_modal,
    render_device_import_modal, render_device_select_modal, render_guardian_modal,
    render_guardian_setup_modal, render_help_modal, render_home_create_modal,
    render_mfa_setup_modal, render_moderator_assignment_modal, render_nickname_modal,
    render_nickname_suggestion_modal, render_remove_device_modal, render_topic_modal,
    GlobalModalProps,
};

use crate::tui::components::copy_to_clipboard;
use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::ceremonies::{
    ChannelError, GuardianSetupError, MfaSetupError, RecoveryError, MIN_CHANNEL_PARTICIPANTS,
    MIN_MFA_DEVICES,
};
use aura_app::ui::contract::OperationState;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{NetworkStatus, ERROR_SIGNAL, SETTINGS_SIGNAL};
use aura_app::ui::workflows::access as access_workflows;
use aura_app::ui::workflows::ceremonies::{
    cancel_key_rotation_ceremony, monitor_key_rotation_ceremony, start_device_threshold_ceremony,
    start_guardian_ceremony,
};
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::settings::refresh_settings_from_runtime;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::CeremonyId;
use aura_core::types::FrostThreshold;

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::harness_state::maybe_export_ui_snapshot;
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::keymap::{global_footer_hints, screen_footer_hints};
use crate::tui::layout::dim;
use crate::tui::navigation::clamp_list_index;
use crate::tui::screens::app::subscriptions::{
    use_authority_id_subscription, use_channels_subscription, use_contacts_subscription,
    use_devices_subscription, use_discovered_peers_subscription, use_invitations_subscription,
    use_messages_subscription, use_nav_status_signals, use_neighborhood_home_meta_subscription,
    use_neighborhood_homes_subscription, use_notifications_subscription,
    use_pending_requests_subscription, use_threshold_subscription, SharedNeighborhoodHomeMeta,
};
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    ChatScreen, ContactsScreen, NeighborhoodScreen, NotificationsScreen, SettingsScreen,
};
use crate::tui::types::{
    AccessLevel, Channel, Contact, Device, Guardian, HomeSummary, Invitation, Message, MfaPolicy,
};

// State machine integration
use crate::tui::iocraft_adapter::convert_iocraft_event;
use crate::tui::props::{
    extract_chat_view_props, extract_contacts_view_props, extract_neighborhood_view_props,
    extract_notifications_view_props, extract_settings_view_props,
};
use crate::tui::state_machine::{transition, DispatchCommand, QueuedModal, TuiCommand, TuiState};
use crate::tui::updates::{ui_update_channel, UiUpdate, UiUpdateReceiver, UiUpdateSender};
use std::sync::Mutex;

mod events;
mod input;
mod render;
mod state;
use events::handle_channel_selection_change;
use input::transition_from_terminal_event;
use render::{build_global_modals, state_indicator_label};
use state::{sync_neighborhood_navigation_state, TuiStateHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
enum NotificationSelection {
    ReceivedInvitation(String),
    SentInvitation(String),
    RecoveryRequest(String),
}

fn read_selected_notification(
    selected_index: usize,
    invitations: &std::sync::Arc<std::sync::RwLock<Vec<Invitation>>>,
    pending_requests: &std::sync::Arc<std::sync::RwLock<Vec<crate::tui::types::PendingRequest>>>,
) -> Option<NotificationSelection> {
    let invitation_items = invitations
        .read()
        .ok()
        .map(|guard| {
            guard
                .iter()
                .filter_map(|invitation| {
                    let selection = match (invitation.direction, invitation.status) {
                        (
                            crate::tui::types::InvitationDirection::Inbound,
                            crate::tui::types::InvitationStatus::Pending,
                        ) => Some(NotificationSelection::ReceivedInvitation(
                            invitation.id.clone(),
                        )),
                        (
                            crate::tui::types::InvitationDirection::Outbound,
                            crate::tui::types::InvitationStatus::Pending,
                        ) => Some(NotificationSelection::SentInvitation(invitation.id.clone())),
                        _ => None,
                    }?;
                    Some((invitation.created_at, selection))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let recovery_items = pending_requests
        .read()
        .ok()
        .map(|guard| {
            guard
                .iter()
                .map(|request| {
                    (
                        request.initiated_at,
                        NotificationSelection::RecoveryRequest(request.id.clone()),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut notifications = invitation_items;
    notifications.extend(recovery_items);
    notifications.sort_by(|left, right| right.0.cmp(&left.0));

    notifications
        .get(selected_index)
        .map(|(_, selection)| selection.clone())
}

/// Props for IoApp
///
/// These values are initial seeds only. Screens subscribe to `aura_app` signals
/// for live data and will overwrite these props immediately on mount.
#[derive(Default, Props)]
pub struct IoAppProps {
    // Screen data - initial seeds only (live data comes from signal subscriptions)
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub invitations: Vec<Invitation>,
    pub guardians: Vec<Guardian>,
    pub devices: Vec<Device>,
    pub nickname_suggestion: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub mfa_policy: MfaPolicy,
    // Contacts screen data
    pub contacts: Vec<Contact>,
    /// Discovered LAN peers
    pub discovered_peers: Vec<DiscoveredPeerInfo>,
    // Neighborhood screen data
    pub neighborhood_name: String,
    pub homes: Vec<HomeSummary>,
    pub access_level: AccessLevel,
    // Account setup
    /// Whether to show account setup modal on start
    pub show_account_setup: bool,
    // Network status
    /// Unified network status (disconnected, no peers, syncing, synced)
    pub network_status: NetworkStatus,
    /// Transport-level peers (active network connections)
    pub transport_peers: usize,
    /// Online contacts (people you know who are currently online)
    pub known_online: usize,
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
    /// Mobile device id (for demo MFA shortcuts)
    #[cfg(feature = "development")]
    pub demo_mobile_device_id: String,
    /// Mobile authority id (for demo device enrollment)
    #[cfg(feature = "development")]
    pub demo_mobile_authority_id: String,
    // Reactive update channel - receiver wrapped in Arc<Mutex<Option>> for take-once semantics
    /// UI update receiver for reactive updates from callbacks
    pub update_rx: Option<Arc<Mutex<Option<UiUpdateReceiver>>>>,
    /// UI update sender for sending updates from event handlers
    pub update_tx: Option<UiUpdateSender>,
    /// Callback registry for all domain actions
    pub callbacks: Option<CallbackRegistry>,
    /// Cached runtime bridge for harness-mode semantic actions that should not
    /// contend on AppCore state locks during startup convergence.
    pub runtime_bridge: Option<Arc<dyn aura_app::runtime_bridge::RuntimeBridge>>,
}

/// Main application with screen navigation
#[allow(clippy::field_reassign_with_default)] // Large struct with many conditional fields
#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Neighborhood);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Pure TUI state machine - holds all UI state for deterministic transitions
    // This is the source of truth; iocraft hooks sync FROM this state
    let show_setup = props.show_account_setup;
    #[cfg(feature = "development")]
    let demo_alice = props.demo_alice_code.clone();
    #[cfg(feature = "development")]
    let demo_carol = props.demo_carol_code.clone();
    #[cfg(feature = "development")]
    let demo_mobile_device_id = props.demo_mobile_device_id.clone();
    #[cfg(feature = "development")]
    let demo_mobile_authority_id = props.demo_mobile_authority_id.clone();
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
            state.settings.demo_mobile_device_id = demo_mobile_device_id.clone();
            state.settings.demo_mobile_authority_id = demo_mobile_authority_id.clone();
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
    let update_tx_holder = props.update_tx.clone();

    // Nickname suggestion state - State<T> automatically triggers re-renders on .set()
    let nickname_suggestion_state = hooks.use_state({
        let initial = props.nickname_suggestion.clone();
        move || initial
    });

    // Get AppCoreContext for IoContext access
    let app_ctx = hooks.use_context::<AppCoreContext>();
    let tasks = app_ctx.tasks();

    // =========================================================================
    // NavBar status: derive from reactive signals (no blocking awaits at startup).
    // =========================================================================
    let nav_signals = use_nav_status_signals(
        &mut hooks,
        &app_ctx,
        props.network_status.clone(),
        props.known_online,
        props.transport_peers,
    );

    // =========================================================================
    // Contacts subscription: SharedContacts for dispatch handlers to read
    // =========================================================================
    // Unlike props.contacts (which is empty), this Arc is kept up-to-date
    // by a reactive subscription. Dispatch handler closures capture the Arc,
    // not the data, so they always read current contacts.
    // Also sends ContactCountChanged updates to keep TuiState in sync for navigation.
    let shared_contacts = use_contacts_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());
    let shared_discovered_peers =
        use_discovered_peers_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Authority subscription: current authority id for dispatch handlers
    // =========================================================================
    let shared_authority_id = use_authority_id_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Channels subscription: SharedChannels for dispatch handlers to read
    // =========================================================================
    // Must be created before messages subscription since messages depend on channels
    let shared_channels = use_channels_subscription(
        &mut hooks,
        &app_ctx,
        shared_authority_id.clone(),
        update_tx_holder.clone(),
    );

    // =========================================================================
    // Shared selection state for messages subscription
    // =========================================================================
    // The TUI state's selected_channel index, shared so the messages subscription
    // can read which channel's messages to fetch.
    let tui_selected_ref = hooks.use_ref(|| std::sync::Arc::new(std::sync::RwLock::new(0_usize)));
    let tui_selected: std::sync::Arc<std::sync::RwLock<usize>> = tui_selected_ref.read().clone();
    let selected_channel_id_ref =
        hooks.use_ref(|| std::sync::Arc::new(std::sync::RwLock::new(None::<String>)));
    let selected_channel_id: std::sync::Arc<std::sync::RwLock<Option<String>>> =
        selected_channel_id_ref.read().clone();

    // =========================================================================
    // Messages subscription: SharedMessages for dispatch handlers to read
    // =========================================================================
    // Used to look up failed messages by ID for retry operations.
    // The Arc is kept up-to-date by a reactive subscription to CHAT_SIGNAL.
    let shared_messages = use_messages_subscription(
        &mut hooks,
        &app_ctx,
        shared_channels.clone(),
        tui_selected.clone(),
        selected_channel_id.clone(),
    );

    // Clone for ChatScreen to compute per-channel message counts
    let tui_selected_for_chat_screen = tui_selected.clone();

    // =========================================================================
    // Devices subscription: SharedDevices for dispatch handlers to read
    // =========================================================================
    let shared_devices = use_devices_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Invitations subscription: SharedInvitations for notification action dispatch
    // =========================================================================
    let shared_invitations = use_invitations_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Neighborhood homes subscription: SharedNeighborhoodHomes for dispatch handlers to read
    // =========================================================================
    let shared_neighborhood_homes = use_neighborhood_homes_subscription(&mut hooks, &app_ctx);
    let shared_neighborhood_home_meta =
        use_neighborhood_home_meta_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Pending requests subscription: SharedPendingRequests for dispatch handlers to read
    // =========================================================================
    let shared_pending_requests = use_pending_requests_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Notifications subscription: keep notification count in sync for navigation
    // =========================================================================
    use_notifications_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Threshold subscription: SharedThreshold for dispatch handlers to read
    // =========================================================================
    // Threshold values from settings - used for recovery eligibility checks
    let shared_threshold = use_threshold_subscription(&mut hooks, &app_ctx);
    let shared_threshold_for_dispatch = shared_threshold;

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
            let format_error = |err: &AppError| format!("{}: {}", err.code(), err);

            // Initial read.
            {
                let reactive = {
                    let core = app_core.raw().read().await;
                    core.reactive().clone()
                };
                if let Ok(Some(err)) = reactive.read(&*ERROR_SIGNAL).await {
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

    // =========================================================================
    // Toast Auto-Dismiss Timer
    //
    // Runs every 100ms to tick the toast queue, enabling auto-dismiss for
    // non-error toasts (5 second timeout). Error toasts never auto-dismiss.
    // Only triggers re-render when a toast is actually dismissed.
    // =========================================================================
    hooks.use_future({
        let mut tui = tui.clone();
        async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
            loop {
                interval.tick().await;
                // Only tick auto-dismissing toasts. Keep error toasts static and avoid
                // forcing a full re-render unless dismissal actually occurred.
                let should_tick = tui
                    .read_clone()
                    .toast_queue
                    .current()
                    .is_some_and(|toast| toast.auto_dismisses());
                if should_tick {
                    tui.tick_active_toast_timer();
                }
            }
        }
    });

    // =========================================================================
    // Discovered Peers Auto-Refresh
    //
    // Keep LAN/rendezvous peer discovery fresh in the background so the
    // Contacts screen can stay purely reactive.
    // =========================================================================
    hooks.use_future({
        let app_core = app_ctx.app_core.raw().clone();
        async move {
            loop {
                let _ = network_workflows::refresh_discovered_peers(&app_core).await;
                tokio::time::sleep(network_workflows::DISCOVERED_PEERS_REFRESH_INTERVAL).await;
            }
        }
    });

    // =========================================================================
    // UI Update Processor - Central handler for all async callback results

    // This is the single point where all async updates flow through.
    // Callbacks send UiUpdate variants, this processor matches and updates
    // the appropriate State<T> values, triggering automatic re-renders.
    // Only runs if update_rx was provided via props.
    // =========================================================================
    let tasks_for_updates = tasks.clone();
    if let Some(rx_holder) = update_rx_holder {
        hooks.use_future({
            let mut nickname_suggestion_state = nickname_suggestion_state.clone();
            let app_core = app_ctx.app_core.clone();
            // Toast queue migration: mutate TuiState via TuiStateHandle (always bumps render version)
            let mut tui = tui.clone();
            let shared_contacts_for_updates = shared_contacts.clone();
            let shared_channels_for_updates = shared_channels.clone();
            // Shared selection state for messages subscription synchronization
            let tui_selected_for_updates = tui_selected;
            let selected_channel_id_for_updates = selected_channel_id.clone();
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
                        UiUpdate::NicknameSuggestionChanged(name) => {
                            nickname_suggestion_state.set(name);
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
                        UiUpdate::DeviceEnrollmentStarted {
                            ceremony_id,
                            nickname_suggestion,
                            enrollment_code,
                            pending_epoch: _,
                            device_id: _,
                        } => {
                            tui.with_mut(|state| {
                                state.settings.last_device_enrollment_code =
                                    enrollment_code.clone();
                                if state.settings.pending_mobile_enrollment_autofill {
                                    state.settings.pending_mobile_enrollment_autofill = false;
                                    state.modal_queue.update_active(|modal| {
                                        if let crate::tui::state_machine::QueuedModal::SettingsDeviceImport(ref mut s) = modal {
                                            s.code = enrollment_code.clone();
                                        }
                                    });
                                } else {
                                    state.modal_queue.enqueue(
                                        crate::tui::state_machine::QueuedModal::SettingsDeviceEnrollment(
                                            crate::tui::state_machine::DeviceEnrollmentCeremonyModalState::started(
                                                ceremony_id,
                                                nickname_suggestion,
                                                enrollment_code,
                                            ),
                                        ),
                                    );
                                }
                            });
                        }
                        UiUpdate::KeyRotationCeremonyStatus {
                            ceremony_id,
                            kind,
                            accepted_count,
                            total_count,
                            threshold,
                            is_complete,
                            has_failed,
                            accepted_participants,
                            error_message,
                            pending_epoch,
                            agreement_mode,
                            reversion_risk,
                        } => {
                            let mut toast: Option<(String, crate::tui::state_machine::ToastLevel)> =
                                None;
                            let mut dismiss_ceremony_started_toast = false;
                            let mut handled_device_enrollment_modal = false;

                            tui.with_mut(|state| {
                                let mut dismiss_modal = false;

                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state_machine::QueuedModal::SettingsDeviceEnrollment(ref mut s) = modal {
                                        handled_device_enrollment_modal = true;
                                        if s.ceremony.ceremony_id.as_deref() == Some(ceremony_id.as_str()) {
                                            s.update_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            if has_failed {
                                                toast = Some((
                                                    error_message
                                                        .clone()
                                                        .unwrap_or_else(|| "Device enrollment failed".to_string()),
                                                    crate::tui::state_machine::ToastLevel::Error,
                                                ));
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    "Device enrollment complete".to_string(),
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                                let app_core = app_core.raw().clone();
                                                let tasks = tasks_for_updates.clone();
                                                tasks.spawn(async move {
                                                    // Small delay to allow commitment tree update to propagate
                                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                                    let _ = refresh_settings_from_runtime(&app_core).await;
                                                });
                                            }
                                        }
                                    } else if let crate::tui::state_machine::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            // Ensure ceremony id is set for cancel UX.
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            // Update per-guardian responses based on accepted participants.
                                            use aura_core::threshold::ParticipantIdentity;
                                            let accepted_guardians: Vec<String> = accepted_participants
                                                .iter()
                                                .filter_map(|p| match p {
                                                    ParticipantIdentity::Guardian(id) => Some(id.to_string()),
                                                    _ => None,
                                                })
                                                .collect();

                                            s.update_responses_from_accepted(&accepted_guardians);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Guardian ceremony failed".to_string());
                                                // Return to threshold selection so the user can retry.
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((msg, crate::tui::state_machine::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    match kind {
                                                        aura_app::ui::types::CeremonyKind::GuardianRotation => format!(
                                                            "Guardian ceremony complete! {threshold}-of-{total_count} committed"
                                                        ),
                                                        aura_app::ui::types::CeremonyKind::DeviceEnrollment => {
                                                            "Device enrollment complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::DeviceRemoval => {
                                                            "Device removal complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::DeviceRotation => {
                                                            format!(
                                                                "Device threshold ceremony complete ({threshold}-of-{total_count})"
                                                            )
                                                        }
                                                        aura_app::ui::types::CeremonyKind::Recovery => {
                                                            "Recovery ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::Invitation => {
                                                            "Invitation ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::RendezvousSecureChannel => {
                                                            "Rendezvous ceremony complete".to_string()
                                                        }
                                                        aura_app::ui::types::CeremonyKind::OtaActivation => {
                                                            "OTA activation ceremony complete".to_string()
                                                        }
                                                    },
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                                if matches!(
                                                    kind,
                                                    aura_app::ui::types::CeremonyKind::DeviceEnrollment
                                                        | aura_app::ui::types::CeremonyKind::DeviceRemoval
                                                        | aura_app::ui::types::CeremonyKind::DeviceRotation
                                                ) {
                                                    let app_core = app_core.raw().clone();
                                                    let tasks = tasks_for_updates.clone();
                                                    tasks.spawn(async move {
                                                        let _ = refresh_settings_from_runtime(&app_core).await;
                                                    });
                                                }
                                            }
                                        }
                                    } else if let crate::tui::state_machine::QueuedModal::MfaSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_count,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            use aura_core::threshold::ParticipantIdentity;
                                            let accepted_devices: Vec<String> = accepted_participants
                                                .iter()
                                                .filter_map(|p| match p {
                                                    ParticipantIdentity::Device(id) => Some(id.to_string()),
                                                    _ => None,
                                                })
                                                .collect();

                                            s.update_responses_from_accepted(&accepted_devices);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Multifactor ceremony failed".to_string());
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((msg, crate::tui::state_machine::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Multifactor ceremony complete! {threshold}-of-{total_count} committed"
                                                    ),
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            }
                                        }
                                    }
                                });

                                if dismiss_modal {
                                    state.modal_queue.dismiss();
                                }
                                if dismiss_ceremony_started_toast {
                                    state.toast_queue.dismiss();
                                }
                            });

                            if !handled_device_enrollment_modal
                                && matches!(kind, aura_app::ui::types::CeremonyKind::DeviceEnrollment)
                                && (is_complete || has_failed)
                            {
                                let app_core = app_core.raw().clone();
                                let tasks = tasks_for_updates.clone();
                                tasks.spawn(async move {
                                    // Small delay to allow commitment tree update to propagate
                                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                                    let _ = refresh_settings_from_runtime(&app_core).await;
                                });
                                if is_complete {
                                    toast = Some((
                                        "Device enrollment complete".to_string(),
                                        crate::tui::state_machine::ToastLevel::Success,
                                    ));
                                } else if has_failed {
                                    toast = Some((
                                        error_message
                                            .clone()
                                            .unwrap_or_else(|| "Device enrollment failed".to_string()),
                                        crate::tui::state_machine::ToastLevel::Error,
                                    ));
                                }
                            }

                            if let Some((msg, level)) = toast {
                                enqueue_toast!(msg, level);
                            }
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
                        UiUpdate::MessageSent { .. } => {
                            // Auto-scroll to bottom (show latest messages including the one just sent)
                            tui.with_mut(|state| {
                                state.chat.message_scroll = 0;
                            });
                        }
                        UiUpdate::MessageRetried { message_id: _ } => {
                            enqueue_toast!(
                                "Retrying message…".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }
                        UiUpdate::ChannelSelected(channel_id) => {
                            // Navigation/state machine owns selected index; cache selected ID
                            // so dispatch can still send when scoped channel snapshots lag.
                            if let Ok(mut selected_id) = selected_channel_id_for_updates.write() {
                                *selected_id = Some(channel_id.clone());
                            }
                            let selected_idx = shared_channels_for_updates
                                .read()
                                .ok()
                                .and_then(|channels| {
                                    channels
                                        .iter()
                                        .position(|channel| channel.id == channel_id)
                                });
                            if let Some(idx) = selected_idx {
                                tui.with_mut(|state| {
                                    state.chat.selected_channel = idx;
                                    state.chat.message_scroll = 0;
                                });
                            }
                        }
                        UiUpdate::ChannelCreated(name) => {
                            enqueue_toast!(
                                format!("Created '{name}'."),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::ChatStateUpdated {
                            channel_count,
                            message_count,
                            selected_index,
                        } => {
                            tui.with_mut(|state| {
                                let prev_message_count = state.chat.message_count;
                                let was_at_bottom = state.chat.message_scroll == 0;

                                state.chat.channel_count = channel_count;
                                state.chat.message_count = message_count;

                                if channel_count == 0 {
                                    state.chat.selected_channel = 0;
                                    state.chat.message_scroll = 0;
                                    return;
                                }

                                // Only update selected_channel from app layer when the
                                // current selection is invalid. This preserves user-driven
                                // channel focus across reactive updates.
                                let current_selection_invalid =
                                    state.chat.selected_channel >= channel_count;

                                if current_selection_invalid {
                                    let idx = clamp_list_index(selected_index.unwrap_or(0), channel_count);
                                    state.chat.selected_channel = idx;
                                    // Reset scroll when switching channels
                                    state.chat.message_scroll = 0;
                                }

                                let selected_channel_id = selected_channel_id_for_updates
                                    .read()
                                    .ok()
                                    .and_then(|guard| guard.clone());
                                if let Some(selected_channel_id) = selected_channel_id {
                                    if let Ok(channels) = shared_channels_for_updates.read() {
                                        if let Some(idx) = channels
                                            .iter()
                                            .position(|channel| channel.id == selected_channel_id)
                                        {
                                            if state.chat.selected_channel != idx {
                                                state.chat.selected_channel = idx;
                                                state.chat.message_scroll = 0;
                                            }
                                        }
                                    }
                                }

                                // Sync the shared selection state so message subscription
                                // knows which channel's messages to fetch
                                if let Ok(mut guard) = tui_selected_for_updates.write() {
                                    *guard = state.chat.selected_channel;
                                }
                                if let Ok(channels) = shared_channels_for_updates.read() {
                                    if let Some(selected) = channels
                                        .get(state.chat.selected_channel)
                                        .map(|channel| channel.id.clone())
                                    {
                                        if let Ok(mut selected_id) =
                                            selected_channel_id_for_updates.write()
                                        {
                                            *selected_id = Some(selected);
                                        }
                                    }
                                }

                                // Auto-scroll to bottom when new messages arrive, but only if
                                // user was already at the bottom (hasn't scrolled up to read history)
                                let new_messages_arrived = message_count > prev_message_count;
                                if new_messages_arrived && was_at_bottom {
                                    state.chat.message_scroll = 0;
                                }

                                // Clamp scroll to valid range
                                let max_scroll = message_count.saturating_sub(18);
                                if state.chat.message_scroll > max_scroll {
                                    state.chat.message_scroll = max_scroll;
                                }
                            });
                        }
                        UiUpdate::TopicSet {
                            channel: _,
                            topic: _,
                        } => {
                            // CHAT_SIGNAL should reflect updated topic; no extra work.
                        }
                        UiUpdate::NeighborhoodStateUpdated { message_count } => {
                            tui.with_mut(|state| {
                                let prev_message_count = state.neighborhood.message_count;
                                let was_at_bottom = state.neighborhood.message_scroll == 0;

                                state.neighborhood.message_count = message_count;

                                // Auto-scroll to bottom when new messages arrive, but only if
                                // user was already at the bottom (hasn't scrolled up to read history)
                                let new_messages_arrived = message_count > prev_message_count;
                                if new_messages_arrived && was_at_bottom {
                                    state.neighborhood.message_scroll = 0;
                                }

                                // Clamp scroll to valid range
                                let max_scroll = message_count.saturating_sub(18);
                                if state.neighborhood.message_scroll > max_scroll {
                                    state.neighborhood.message_scroll = max_scroll;
                                }
                            });
                        }
                        UiUpdate::ChannelInfoParticipants {
                            channel_id,
                            participants,
                        } => {
                            let mapped_participants = if let Ok(contacts) =
                                shared_contacts_for_updates.read()
                            {
                                participants
                                    .iter()
                                    .map(|entry| {
                                        if entry == "You" {
                                            return entry.clone();
                                        }
                                        if let Some(contact) =
                                            contacts.iter().find(|c| c.id == *entry)
                                        {
                                            if !contact.nickname.is_empty() {
                                                return contact.nickname.clone();
                                            }
                                            if let Some(name) = &contact.nickname_suggestion {
                                                return name.clone();
                                            }
                                        }
                                        entry.clone()
                                    })
                                    .collect::<Vec<_>>()
                            } else {
                                participants.clone()
                            };
                            tui.with_mut(|state| {
                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state_machine::QueuedModal::ChatInfo(ref mut info) = modal {
                                        if info.channel_id == channel_id
                                            && (mapped_participants.len() > 1 || info.participants.len() <= 1) {
                                                info.participants = mapped_participants.clone();
                                            }
                                    }
                                });
                            });
                        }

                        // =========================================================================
                        // Invitations
                        // =========================================================================
                        UiUpdate::InvitationAccepted { invitation_id: _ } => {
                            tui.with_mut(|state| {
                                state.set_operation_state(
                                    OperationId::invitation_accept(),
                                    OperationState::Succeeded,
                                );
                            });
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
                        UiUpdate::InvitationExported { code } => {
                            tui.with_mut(|state| {
                                state.set_operation_state(
                                    OperationId::invitation_create(),
                                    OperationState::Succeeded,
                                );
                                state.last_exported_invitation_code = Some(code.clone());
                                let copied = copy_to_clipboard(&code).is_ok();
                                state
                                    .modal_queue
                                    .enqueue(crate::tui::state_machine::QueuedModal::ContactsCode(
                                        {
                                            let mut modal =
                                                crate::tui::state_machine::InvitationCodeModalState::for_code(code);
                                            if copied {
                                                modal.set_copied();
                                            }
                                            modal
                                        },
                                    ));
                            });
                        }
                        UiUpdate::InvitationImported { invitation_code: _ } => {
                            tui.with_mut(|state| {
                                state.set_operation_state(
                                    OperationId::invitation_accept(),
                                    OperationState::Succeeded,
                                );
                            });
                            enqueue_toast!(
                                "Invitation imported".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }

                        // =========================================================================
                        // Navigation
                        // =========================================================================
                        UiUpdate::HomeEntered { home_id: _ } => {
                            // Navigation/state machine owns the current home selection.
                        }
                        UiUpdate::NavigatedHome => {
                            // Navigation/state machine handles this.
                        }
                        UiUpdate::NavigatedToLimited => {
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
                            agreement_mode,
                            reversion_risk,
                        } => {
                            let mut toast: Option<(String, crate::tui::state_machine::ToastLevel)> =
                                None;
                            let mut dismiss_ceremony_started_toast = false;

                            tui.with_mut(|state| {
                                let mut dismiss_modal = false;

                                state.modal_queue.update_active(|modal| {
                                    if let crate::tui::state_machine::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step(),
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            s.ensure_ceremony_id(ceremony_id.clone());

                                            s.update_ceremony_from_status(
                                                accepted_guardians.len() as u16,
                                                total_count,
                                                threshold,
                                                is_complete,
                                                has_failed,
                                                error_message.clone(),
                                                pending_epoch,
                                                agreement_mode,
                                                reversion_risk,
                                            );

                                            s.update_responses_from_accepted(&accepted_guardians);

                                            if has_failed {
                                                let msg = error_message
                                                    .clone()
                                                    .unwrap_or_else(|| "Guardian ceremony failed".to_string());
                                                // Return to threshold selection so the user can retry.
                                                s.reset_to_threshold_after_failure();

                                                toast = Some((
                                                    msg,
                                                    crate::tui::state_machine::ToastLevel::Error,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Guardian ceremony complete! {threshold}-of-{total_count} committed"
                                                    ),
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            }
                                        }
                                    }
                                });

                                if dismiss_modal {
                                    state.modal_queue.dismiss();
                                }
                                if dismiss_ceremony_started_toast {
                                    state.toast_queue.dismiss();
                                }
                            });

                            if let Some((msg, level)) = toast {
                                enqueue_toast!(msg, level);
                            }
                        }

                        // =========================================================================
                        // Contacts
                        // =========================================================================
                        UiUpdate::ContactCountChanged(count) => {
                            let needs_update = {
                                let state = tui.read_clone();
                                state.contacts.contact_count != count
                                    || state.contacts.selected_index
                                        != clamp_list_index(state.contacts.selected_index, count)
                            };
                            if needs_update {
                                tui.with_mut(|state| {
                                    state.contacts.contact_count = count;
                                    state.contacts.selected_index =
                                        clamp_list_index(state.contacts.selected_index, count);
                                });
                            }
                        }
                        UiUpdate::NotificationsCountChanged(count) => {
                            let needs_update = {
                                let state = tui.read_clone();
                                state.notifications.item_count != count
                                    || state.notifications.selected_index
                                        != clamp_list_index(state.notifications.selected_index, count)
                            };
                            if needs_update {
                                tui.with_mut(|state| {
                                    state.notifications.item_count = count;
                                    state.notifications.selected_index =
                                        clamp_list_index(state.notifications.selected_index, count);
                                });
                            }
                        }
                        UiUpdate::NicknameUpdated {
                            contact_id: _,
                            nickname: _,
                        } => {
                            // CONTACTS_SIGNAL owns contact data; no local state update.
                        }
                        UiUpdate::ChatStarted { contact_id } => {
                            // Navigate to Chat screen after starting a direct chat
                            tracing::info!("Chat started with contact: {}", contact_id);
                            tui.with_mut(|state| {
                                state.router.go_to(Screen::Chat);
                            });
                        }
                        UiUpdate::LanPeerInvited { peer_id: _ } => {
                            enqueue_toast!(
                                "LAN peer invited".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::LanPeersCountChanged(count) => {
                            let needs_update = {
                                let state = tui.read_clone();
                                state.contacts.lan_peer_count != count
                                    || state.contacts.lan_selected_index
                                        != clamp_list_index(state.contacts.lan_selected_index, count)
                                    || (count == 0
                                        && !matches!(
                                            state.contacts.list_focus,
                                            crate::tui::state_machine::ContactsListFocus::Contacts
                                        ))
                            };
                            if needs_update {
                                tui.with_mut(|state| {
                                    state.contacts.lan_peer_count = count;
                                    if count == 0 {
                                        state.contacts.list_focus =
                                            crate::tui::state_machine::ContactsListFocus::Contacts;
                                    }
                                    state.contacts.lan_selected_index =
                                        clamp_list_index(state.contacts.lan_selected_index, count);
                                });
                            }
                        }

                        // =========================================================================
                        // Home operations
                        // =========================================================================
                        UiUpdate::HomeInviteSent { contact_id: _ } => {
                            enqueue_toast!(
                                "Invite sent".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::ModeratorGranted { contact_id: _ } => {
                            enqueue_toast!(
                                "Moderator granted".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::ModeratorRevoked { contact_id: _ } => {
                            enqueue_toast!(
                                "Moderator revoked".to_string(),
                                crate::tui::state_machine::ToastLevel::Info
                            );
                        }

                        // =========================================================================
                        // Account
                        // =========================================================================
                        UiUpdate::AccountCreated => {
                            tui.with_mut(|state| {
                                state.set_operation_state(
                                    OperationId::account_create(),
                                    OperationState::Succeeded,
                                );
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
                            if operation.to_ascii_lowercase().contains("invitation") {
                                tui.with_mut(|state| {
                                    state.set_operation_state(
                                        OperationId::invitation_accept(),
                                        OperationState::Failed,
                                    );
                                });
                            }
                            // For account creation, show error in the modal instead of toast.
                            if operation == "CreateAccount" {
                                tui.with_mut(|state| {
                                    state.set_operation_state(
                                        OperationId::account_create(),
                                        OperationState::Failed,
                                    );
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

    // Note: Domain data (channels, messages, guardians, etc.) is no longer passed to screens.
    // Each screen subscribes to signals directly via AppCoreContext.
    // See scripts/check/arch.sh --reactive for architectural enforcement.

    // Read TUI state for rendering via type-safe handle.
    // This MUST be used for all render-time state access - it reads the version to establish
    // reactivity, ensuring the component re-renders when state changes via tui.replace().
    // See TuiStateHandle and TuiStateSnapshot docs for the reactivity model.
    let tui_snapshot = tui.read_for_render();
    let harness_contacts = shared_contacts.read().ok().map(|contacts| contacts.clone());
    maybe_export_ui_snapshot(
        &tui_snapshot,
        &app_ctx.snapshot(),
        harness_contacts.as_deref(),
    );

    // Callbacks registry and individual callback extraction for screen props
    let callbacks = props.callbacks.clone();

    // Extract individual callbacks from registry for screen component props
    // (Screen components still use individual callback props for now)
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

    let on_update_mfa = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_mfa.clone());
    let on_update_nickname_suggestion = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_nickname_suggestion.clone());
    let on_update_threshold = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_update_threshold.clone());
    let on_add_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_add_device.clone());
    let on_remove_device = callbacks
        .as_ref()
        .map(|cb| cb.settings.on_remove_device.clone());

    let current_screen = screen.get();

    // Check if in insert mode (MessageInput has its own hint bar, so hide main hints)
    // Note: tui_snapshot was created earlier during render for all render-time state access
    let is_insert_mode = tui_snapshot.is_insert_mode();

    // Extract screen view props from TuiState using testable extraction functions
    let chat_props = extract_chat_view_props(&tui_snapshot);
    let contacts_props = extract_contacts_view_props(&tui_snapshot);
    let settings_props = extract_settings_view_props(&tui_snapshot);
    let notifications_props = extract_notifications_view_props(&tui_snapshot);
    let neighborhood_props = extract_neighborhood_view_props(&tui_snapshot);

    // =========================================================================
    // Global modal overlays
    // =========================================================================
    let global_modals = build_global_modals(current_screen, &tui_snapshot);

    // Extract toast state from queue (type-enforced single toast at a time)
    let queued_toast = tui_snapshot.toast_queue.current().cloned();

    // Global/screen hints come from one shared keybinding registry.
    let global_hints = global_footer_hints();
    let screen_hints = screen_footer_hints(current_screen);

    let state_indicator = state_indicator_label(&tui_snapshot);

    let tasks_for_events = tasks.clone();
    hooks.use_terminal_events({
        let mut screen = screen.clone();
        let mut should_exit = should_exit.clone();
        let mut tui = tui;
        // Clone AppCore for key rotation operations
        let app_core_for_ceremony = app_ctx.app_core.clone();
        let tasks_for_dispatch = tasks;
        // Clone update channel sender for ceremony UI updates
        let update_tx_for_ceremony = props.update_tx.clone();
        // Clone callbacks registry for command dispatch
        let callbacks = callbacks.clone();
        // Clone shared contacts Arc for guardian setup dispatch
        let shared_channels_for_dispatch = shared_channels.clone();
        let shared_neighborhood_homes_for_dispatch = shared_neighborhood_homes;
        let shared_neighborhood_home_meta_for_dispatch = shared_neighborhood_home_meta;
        let shared_invitations_for_dispatch = shared_invitations;
        let shared_pending_requests_for_dispatch = shared_pending_requests;
        // This Arc is updated by a reactive subscription, so reading from it
        // always gets current contacts (not stale props)
        let shared_contacts_for_dispatch = shared_contacts;
        let shared_discovered_peers_for_dispatch = shared_discovered_peers;
        let _shared_authority_id_for_dispatch = shared_authority_id;
        let app_ctx_for_dispatch = app_ctx.clone();
        let runtime_bridge_for_dispatch = props.runtime_bridge.clone();
        // Clone shared messages Arc for message retry dispatch
        // Used to look up failed messages by ID to get channel and content for retry
        let shared_messages_for_dispatch = shared_messages;
        // Used to map device selection for MFA wizard
        let shared_devices_for_dispatch = shared_devices;
        // Clone shared selection state for immediate sync on channel navigation
        let tui_selected_for_events = tui_selected_for_chat_screen.clone();
        let selected_channel_id_for_dispatch = selected_channel_id.clone();
        // Used for recovery eligibility checks (from threshold subscription)
        move |event| {
            if let Some(input_transition) = transition_from_terminal_event(
                event,
                &tui,
                &shared_channels_for_dispatch,
                &shared_neighborhood_homes_for_dispatch,
                &shared_neighborhood_home_meta_for_dispatch,
                &selected_channel_id_for_dispatch,
            ) {
                let current = input_transition.current;
                let mut new_state = input_transition.new_state;
                let commands = input_transition.commands;

                // Execute commands using callbacks registry
                if let Some(ref cb) = callbacks {
                    handle_channel_selection_change(
                        &current,
                        &new_state,
                        &shared_channels_for_dispatch,
                        &tui_selected_for_events,
                        &selected_channel_id_for_dispatch,
                        cb,
                    );
                    for cmd in commands {
                        match cmd {
                            TuiCommand::Exit => {
                                should_exit.set(true);
                            }
                            TuiCommand::Dispatch(dispatch_cmd) => {
                                // Handle dispatch commands via CallbackRegistry
                                match dispatch_cmd {
                                    DispatchCommand::CreateAccount { name } => {
                                        new_state.set_operation_state(
                                            OperationId::account_create(),
                                            OperationState::Submitting,
                                        );
                                        (cb.app.on_create_account)(name);
                                    }
                                    DispatchCommand::ImportDeviceEnrollmentDuringOnboarding {
                                        code,
                                    } => {
                                        (cb.app.on_import_device_enrollment_during_onboarding)(
                                            code,
                                        );
                                    }
                                    DispatchCommand::AddGuardian { contact_id } => {
                                        (cb.recovery.on_select_guardian)(contact_id.to_string());
                                    }

                                    // === Chat Screen Commands ===
                                    DispatchCommand::SelectChannel { channel_id } => {
                                        if let Ok(mut selected_id) = selected_channel_id_for_dispatch.write() {
                                            *selected_id = Some(channel_id.to_string());
                                        }
                                        (cb.chat.on_channel_select)(channel_id.to_string());
                                    }
                                    DispatchCommand::SendChatMessage { content } => {
                                        // Get channel_id from TUI's selected_channel to avoid
                                        // race condition with async channel selection updates
                                        let idx = new_state.chat.selected_channel;
                                        let is_slash_command = content.trim_start().starts_with('/');
                                        let channels = match shared_channels_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };
                                        let selected_channel_id = if channels.is_empty() {
                                                None
                                            } else {
                                                let clamped = idx.min(channels.len().saturating_sub(1));
                                                channels.get(clamped).map(|channel| channel.id.clone())
                                            };
                                        let fallback_channel_id = selected_channel_id.or_else(|| {
                                            match selected_channel_id_for_dispatch.read() {
                                                Ok(guard) => guard.clone(),
                                                Err(poisoned) => poisoned.into_inner().clone(),
                                            }
                                        });
                                        if let Some(channel_id) = fallback_channel_id {
                                            (cb.chat.on_send)(channel_id, content);
                                        } else if is_slash_command {
                                            // Allow slash commands even when no channel is selected.
                                            // This unblocks bootstrap flows such as `/join <name>`.
                                            let fallback_channel = new_state
                                                .neighborhood
                                                .entered_home_id
                                                .clone()
                                                .unwrap_or_else(|| "home".to_string());
                                            (cb.chat.on_send)(fallback_channel, content);
                                        } else {
                                            // Channel selection and scoped channel snapshots can
                                            // briefly lag after channel creation/navigation.
                                            // Retry send in the background before surfacing a hard failure.
                                            let retry_idx = idx;
                                            let retry_content = content.clone();
                                            let on_send = cb.chat.on_send.clone();
                                            let retry_channels = shared_channels_for_dispatch.clone();
                                            let retry_selected_id =
                                                selected_channel_id_for_dispatch.clone();
                                            tasks_for_dispatch.spawn(async move {
                                                for _ in 0..40 {
                                                    let resolved_from_channels = match retry_channels
                                                        .read()
                                                    {
                                                        Ok(guard) => {
                                                            if guard.is_empty() {
                                                                None
                                                            } else {
                                                                let clamped = retry_idx
                                                                    .min(guard.len().saturating_sub(1));
                                                                guard
                                                                    .get(clamped)
                                                                    .map(|channel| channel.id.clone())
                                                            }
                                                        }
                                                        Err(poisoned) => {
                                                            let guard = poisoned.into_inner();
                                                            if guard.is_empty() {
                                                                None
                                                            } else {
                                                                let clamped = retry_idx
                                                                    .min(guard.len().saturating_sub(1));
                                                                guard
                                                                    .get(clamped)
                                                                    .map(|channel| channel.id.clone())
                                                            }
                                                        }
                                                    };
                                                    let resolved = resolved_from_channels.or_else(|| {
                                                        match retry_selected_id.read() {
                                                            Ok(guard) => guard.clone(),
                                                            Err(poisoned) => poisoned.into_inner().clone(),
                                                        }
                                                    });
                                                    if let Some(channel_id) = resolved {
                                                        on_send(channel_id, retry_content.clone());
                                                        return;
                                                    }
                                                    tokio::time::sleep(std::time::Duration::from_millis(25))
                                                        .await;
                                                }
                                            });
                                            new_state.toast_warning(
                                                "Channel selection syncing; sending shortly",
                                            );
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
                                        let channels = match shared_channels_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };
                                        if let Some(channel) = channels.get(idx) {
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
                                    }
                                    DispatchCommand::OpenChatInfoModal => {
                                        let idx = new_state.chat.selected_channel;
                                        let channels = match shared_channels_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };
                                        if let Some(channel) = channels.get(idx) {
                                            let mut modal_state = crate::tui::state_machine::ChannelInfoModalState::for_channel(
                                                &channel.id,
                                                &channel.name,
                                                channel.topic.as_deref(),
                                            );

                                            // Best-effort: start with self; authoritative list arrives via list_participants.
                                            let mut participants = vec!["You".to_string()];

                                            if participants.len() <= 1 && channel.member_count > 1 {
                                                let extra = channel
                                                    .member_count
                                                    .saturating_sub(participants.len() as u32);
                                                if extra > 0 {
                                                    participants.push(format!("+{extra} others"));
                                                }
                                            }

                                            modal_state.participants = participants;
                                            new_state
                                                .modal_queue
                                                .enqueue(crate::tui::state_machine::QueuedModal::ChatInfo(
                                                    modal_state,
                                                ));
                                            (cb.chat.on_list_participants)(channel.id.clone());
                                        } else {
                                            new_state.toast_error("No channel selected");
                                        }
                                    }
                                    DispatchCommand::OpenChatCreateWizard => {
                                        let current_contacts = match shared_contacts_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };

                                        // Validate: need at least 1 contact (+ self = 2 participants)
                                        if current_contacts.is_empty() {
                                            new_state.toast_error(
                                                ChannelError::InsufficientParticipants {
                                                    required: MIN_CHANNEL_PARTICIPANTS,
                                                    available: 1, // Just self
                                                }
                                                .to_string(),
                                            );
                                            continue;
                                        }

                                        let mut candidates: Vec<crate::tui::state_machine::ChatMemberCandidate> =
                                            current_contacts
                                                .iter()
                                                // Channel member invites only support user authorities.
                                                .filter(|c| c.id.starts_with("authority-"))
                                                .map(|c| crate::tui::state_machine::ChatMemberCandidate {
                                                    id: c.id.clone(),
                                                    name: if !c.nickname.is_empty() {
                                                        c.nickname.clone()
                                                    } else if let Some(s) = &c.nickname_suggestion {
                                                        s.clone()
                                                    } else {
                                                        let short = c.id.chars().take(8).collect::<String>();
                                                        format!("{short}...")
                                                    },
                                                })
                                                .collect();
                                        let demo_alice_id = crate::ids::authority_id(&format!(
                                            "demo:{}:{}:authority",
                                            aura_app::ui::workflows::demo_config::DEMO_SEED_2024,
                                            "Alice"
                                        ))
                                        .to_string();
                                        let demo_carol_id = crate::ids::authority_id(&format!(
                                            "demo:{}:{}:authority",
                                            aura_app::ui::workflows::demo_config::DEMO_SEED_2024 + 1,
                                            "Carol"
                                        ))
                                        .to_string();
                                        let is_demo_mode = candidates.iter().any(|candidate| {
                                            candidate.id == demo_alice_id
                                                || candidate.id == demo_carol_id
                                        });
                                        let demo_name_rank = |contact_id: &str, name: &str| -> u8 {
                                            if !is_demo_mode {
                                                return 2;
                                            }
                                            if name.eq_ignore_ascii_case("Alice")
                                                || demo_alice_id == contact_id
                                            {
                                                0
                                            } else if name.eq_ignore_ascii_case("Carol")
                                                || demo_carol_id == contact_id
                                            {
                                                1
                                            } else {
                                                2
                                            }
                                        };
                                        candidates.sort_by(|left, right| {
                                            demo_name_rank(&left.id, &left.name)
                                                .cmp(&demo_name_rank(&right.id, &right.name))
                                                .then_with(|| {
                                                    left.name
                                                        .to_ascii_lowercase()
                                                        .cmp(&right.name.to_ascii_lowercase())
                                                })
                                        });

                                        let mut modal_state =
                                            crate::tui::state_machine::CreateChannelModalState::new();
                                        modal_state.contacts = candidates;
                                        modal_state.ensure_threshold();

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::ChatCreate(
                                                modal_state,
                                            ),
                                        );
                                    }

                                    DispatchCommand::CreateChannel {
                                        name,
                                        topic,
                                        mut members,
                                        threshold_k,
                                    } => {
                                        // Demo safeguard: keep the canonical trio room aligned with
                                        // Alice+Carol participation even if picker timing drifts.
                                        if name.eq_ignore_ascii_case("demo-trio-room") {
                                            let contacts = match shared_contacts_for_dispatch.read() {
                                                Ok(guard) => guard.clone(),
                                                Err(poisoned) => poisoned.into_inner().clone(),
                                            };
                                            let mut demo_members = Vec::new();
                                            let expected_demo_ids = [
                                                crate::ids::authority_id(&format!(
                                                    "demo:{}:{}:authority",
                                                    aura_app::ui::workflows::demo_config::DEMO_SEED_2024,
                                                    "Alice"
                                                ))
                                                .to_string(),
                                                crate::ids::authority_id(&format!(
                                                    "demo:{}:{}:authority",
                                                    aura_app::ui::workflows::demo_config::DEMO_SEED_2024 + 1,
                                                    "Carol"
                                                ))
                                                .to_string(),
                                            ];
                                            for expected_id in expected_demo_ids {
                                                if contacts.iter().any(|contact| contact.id == expected_id) {
                                                    if let Ok(parsed_id) =
                                                        expected_id.parse::<aura_core::AuthorityId>()
                                                    {
                                                        demo_members.push(parsed_id);
                                                    }
                                                }
                                            }
                                            // Fallback if deterministic IDs are unavailable in the contact list.
                                            for needle in ["Alice", "Carol"] {
                                                if demo_members.len() >= 2 {
                                                    break;
                                                }
                                                if let Some(contact_id) =
                                                    contacts.iter().find_map(|contact| {
                                                        let nickname = contact.nickname.trim();
                                                        let suggested = contact
                                                            .nickname_suggestion
                                                            .as_deref()
                                                            .unwrap_or("")
                                                            .trim();
                                                        if nickname.eq_ignore_ascii_case(needle)
                                                            || suggested.eq_ignore_ascii_case(needle)
                                                        {
                                                            Some(contact.id.clone())
                                                        } else {
                                                            None
                                                        }
                                                    })
                                                {
                                                    if let Ok(parsed_id) =
                                                        contact_id.parse::<aura_core::AuthorityId>()
                                                    {
                                                        demo_members.push(parsed_id);
                                                    }
                                                }
                                            }
                                            if !demo_members.is_empty() {
                                                tracing::debug!(
                                                    room = %name,
                                                    ?members,
                                                    ?demo_members,
                                                    "Applying demo trio membership override"
                                                );
                                                members = demo_members;
                                            }
                                        }
                                        members.sort();
                                        members.dedup();
                                        (cb.chat.on_create_channel)(
                                            name,
                                            topic,
                                            members.into_iter().map(|id| id.to_string()).collect(),
                                            threshold_k.get(),
                                        );
                                    }
                                    DispatchCommand::SetChannelTopic { channel_id, topic } => {
                                        (cb.chat.on_set_topic)(channel_id.to_string(), topic);
                                    }
                                    DispatchCommand::DeleteChannel { channel_id } => {
                                        (cb.chat.on_close_channel)(channel_id.to_string());
                                    }

                                    // === Contacts Screen Commands ===
                                    DispatchCommand::UpdateNickname {
                                        contact_id,
                                        nickname,
                                    } => {
                                        (cb.contacts.on_update_nickname)(
                                            contact_id.to_string(),
                                            nickname,
                                        );
                                    }
                                    DispatchCommand::OpenContactNicknameModal => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            if let Some(contact) = guard.get(idx) {
                                                // nickname is already populated with nickname_suggestion if empty (see Contact::from)
                                                let modal_state = crate::tui::state_machine::NicknameModalState::for_contact(
                                                    &contact.id,
                                                    &contact.nickname,
                                                ).with_suggestion(contact.nickname_suggestion.clone());
                                                new_state
                                                    .modal_queue
                                                    .enqueue(crate::tui::state_machine::QueuedModal::ContactsNickname(
                                                        modal_state,
                                                    ));
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
                                    }
                                    DispatchCommand::OpenCreateInvitationModal => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            let mut modal_state = if let Some(contact) = guard.get(idx) {
                                                crate::tui::state_machine::CreateInvitationModalState::for_receiver(
                                                    contact.id.clone(),
                                                    contact.nickname.clone(),
                                                )
                                            } else {
                                                crate::tui::state_machine::CreateInvitationModalState::new()
                                            };
                                            modal_state.type_index = 1;
                                            new_state
                                                .modal_queue
                                                .enqueue(crate::tui::state_machine::QueuedModal::ContactsCreate(
                                                    modal_state,
                                                ));
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
                                    }
                                    DispatchCommand::InviteLanPeer => {
                                        let idx = new_state.contacts.lan_selected_index;
                                        if let Ok(guard) = shared_discovered_peers_for_dispatch.read()
                                        {
                                            if let Some(peer) = guard.get(idx) {
                                                let authority_id = peer.authority_id.to_string();
                                                let address = peer.address.clone();
                                                if address.is_empty() {
                                                    new_state.toast_error(
                                                        "Selected peer has no LAN address",
                                                    );
                                                } else {
                                                    (cb.contacts.on_invite_lan_peer)(
                                                        authority_id,
                                                        address,
                                                    );
                                                }
                                            } else {
                                                new_state.toast_error("No LAN peer selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read LAN peers");
                                        }
                                    }
                                    DispatchCommand::StartChat => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            if let Some(contact) = guard.get(idx) {
                                                (cb.contacts.on_start_chat)(contact.id.clone());
                                                // Move to Chat immediately so operators can proceed
                                                // while the async start-chat command finalizes.
                                                new_state.router.go_to(Screen::Chat);
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
                                    }
                                    DispatchCommand::InviteSelectedContactToChannel => {
                                        let contact_idx = new_state.contacts.selected_index;
                                        let channel_idx = new_state.chat.selected_channel;
                                        let contacts = match shared_contacts_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };
                                        let channels = match shared_channels_for_dispatch.read() {
                                            Ok(guard) => guard.clone(),
                                            Err(poisoned) => poisoned.into_inner().clone(),
                                        };
                                        let Some(contact) = contacts.get(contact_idx) else {
                                            new_state.toast_error("No contact selected");
                                            continue;
                                        };
                                        let Some(channel) = channels.get(channel_idx) else {
                                            new_state.toast_error("No channel selected");
                                            continue;
                                        };
                                        (cb.contacts.on_invite_to_channel)(
                                            contact.id.clone(),
                                            channel.name.clone(),
                                        );
                                    }
                                    DispatchCommand::RemoveContact { contact_id } => {
                                        (cb.contacts.on_remove_contact)(contact_id.to_string());
                                    }
                                    DispatchCommand::OpenRemoveContactModal => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            if let Some(contact) = guard.get(idx) {
                                                // Get display name for confirmation message
                                                let display_name = if !contact.nickname.is_empty() {
                                                    contact.nickname.clone()
                                                } else if let Some(s) = &contact.nickname_suggestion {
                                                    s.clone()
                                                } else {
                                                    let short = contact.id.chars().take(8).collect::<String>();
                                                    format!("{short}...")
                                                };

                                                // Show confirmation modal
                                                new_state.modal_queue.enqueue(
                                                    crate::tui::state_machine::QueuedModal::Confirm {
                                                        title: "Remove Contact".to_string(),
                                                        message: format!(
                                                            "Are you sure you want to remove \"{display_name}\"?"
                                                        ),
                                                        on_confirm: Some(
                                                            crate::tui::state_machine::ConfirmAction::RemoveContact {
                                                                contact_id: contact.id.clone().into(),
                                                            },
                                                        ),
                                                    },
                                                );
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
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
                                        let selected = read_selected_notification(
                                            new_state.notifications.selected_index,
                                            &shared_invitations_for_dispatch,
                                            &shared_pending_requests_for_dispatch,
                                        );
                                        if let Some(NotificationSelection::ReceivedInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_accept)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a received invitation to accept",
                                            );
                                        }
                                    }
                                    DispatchCommand::DeclineInvitation => {
                                        let selected = read_selected_notification(
                                            new_state.notifications.selected_index,
                                            &shared_invitations_for_dispatch,
                                            &shared_pending_requests_for_dispatch,
                                        );
                                        if let Some(NotificationSelection::ReceivedInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_decline)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a received invitation to decline",
                                            );
                                        }
                                    }
                                    DispatchCommand::CreateInvitation {
                                        receiver_id,
                                        invitation_type,
                                        message,
                                        ttl_secs,
                                    } => {
                                        if std::env::var_os("AURA_HARNESS_MODE").is_some()
                                            && matches!(
                                                invitation_type,
                                                crate::tui::state_machine::InvitationKind::Contact
                                            )
                                        {
                                            let message_for_task = message.clone();
                                            let update_tx = update_tx_for_ceremony.clone();
                                            let runtime_bridge = runtime_bridge_for_dispatch.clone();
                                            tasks_for_dispatch.spawn(async move {
                                                if let Some(tx) = update_tx.clone() {
                                                    let _ = tx
                                                        .send(UiUpdate::ToastAdded(ToastMessage::info(
                                                            "create-invitation",
                                                            "Creating contact invitation...",
                                                        )))
                                                        .await;
                                                }
                                                let ttl_ms =
                                                    ttl_secs.map(|seconds| seconds.saturating_mul(1000));
                                                let result: Result<String, aura_core::AuraError> =
                                                    match runtime_bridge {
                                                        Some(runtime) => {
                                                            match tokio::time::timeout(
                                                                std::time::Duration::from_secs(10),
                                                                runtime.create_contact_invitation(
                                                                    receiver_id,
                                                                    None,
                                                                    message_for_task,
                                                                    ttl_ms,
                                                                ),
                                                            )
                                                            .await
                                                            {
                                                                Ok(Ok(invitation)) => {
                                                                    match tokio::time::timeout(
                                                                        std::time::Duration::from_secs(10),
                                                                        runtime.export_invitation(
                                                                            invitation.invitation_id.as_str(),
                                                                        ),
                                                                    )
                                                                    .await
                                                                    {
                                                                        Ok(result) => result.map_err(|error| {
                                                                            aura_core::AuraError::agent(format!(
                                                                                "Failed to export contact invitation: {error}"
                                                                            ))
                                                                        }),
                                                                        Err(_) => Err(aura_core::AuraError::internal(
                                                                            "Timed out exporting contact invitation",
                                                                        )),
                                                                    }
                                                                }
                                                                Ok(Err(error)) => Err(aura_core::AuraError::agent(
                                                                    format!(
                                                                        "Failed to create contact invitation: {error}"
                                                                    ),
                                                                )),
                                                                Err(_) => Err(aura_core::AuraError::internal(
                                                                    "Timed out creating contact invitation",
                                                                )),
                                                            }
                                                        }
                                                        None => Err(aura_core::AuraError::agent(
                                                            "Runtime bridge unavailable for harness contact invitation",
                                                        )),
                                                    };

                                                match result {
                                                    Ok(code) => {
                                                        if let Err(error) = copy_to_clipboard(&code) {
                                                            tracing::warn!(
                                                                error = %error,
                                                                "failed to copy harness invite code to clipboard"
                                                            );
                                                        }
                                                        if let Some(tx) = update_tx {
                                                            let _ = tx
                                                                .send(UiUpdate::InvitationExported { code })
                                                                .await;
                                                        }
                                                    }
                                                    Err(error) => {
                                                        tracing::warn!(
                                                            error = %error,
                                                            receiver_id = %receiver_id,
                                                            "failed to create harness contact invitation"
                                                        );
                                                        if let Some(tx) = update_tx {
                                                            let _ = tx
                                                                .send(UiUpdate::operation_failed(
                                                                    "create invitation",
                                                                    error.to_string(),
                                                                ))
                                                                .await;
                                                        }
                                                    }
                                                }
                                            });
                                        } else {
                                            (cb.invitations.on_create)(
                                                receiver_id,
                                                invitation_type.as_str().to_owned(),
                                                message,
                                                ttl_secs,
                                            );
                                        }
                                    }
                                    DispatchCommand::ImportInvitation { code } => {
                                        if std::env::var_os("AURA_HARNESS_MODE").is_some() {
                                            let code_for_task = code.clone();
                                            let update_tx = update_tx_for_ceremony.clone();
                                            let runtime_bridge = runtime_bridge_for_dispatch.clone();
                                            tasks_for_dispatch.spawn(async move {
                                                let result: Result<String, aura_core::AuraError> =
                                                    match runtime_bridge {
                                                        Some(runtime) => {
                                                            match tokio::time::timeout(
                                                                std::time::Duration::from_secs(10),
                                                                runtime.import_invitation(&code_for_task),
                                                            )
                                                            .await
                                                            {
                                                                Ok(Ok(invitation)) => {
                                                                    if matches!(
                                                                        invitation.invitation_type,
                                                                        aura_app::ui::types::InvitationBridgeType::Contact { .. }
                                                                            | aura_app::ui::types::InvitationBridgeType::Channel { .. }
                                                                            | aura_app::ui::types::InvitationBridgeType::Guardian { .. }
                                                                    ) {
                                                                        match tokio::time::timeout(
                                                                            std::time::Duration::from_secs(10),
                                                                            runtime.accept_invitation(
                                                                                invitation.invitation_id.as_str(),
                                                                            ),
                                                                        )
                                                                        .await
                                                                        {
                                                                            Ok(Ok(())) => {
                                                                                aura_app::ui::workflows::runtime::converge_runtime(&runtime).await;
                                                                                Ok(code_for_task)
                                                                            }
                                                                            Ok(Err(error)) => Err(aura_core::AuraError::agent(
                                                                                format!(
                                                                                    "Failed to accept invitation: {error}"
                                                                                ),
                                                                            )),
                                                                            Err(_) => Err(aura_core::AuraError::internal(
                                                                                "Timed out accepting invitation",
                                                                            )),
                                                                        }
                                                                    } else {
                                                                        Ok(code_for_task)
                                                                    }
                                                                }
                                                                Ok(Err(error)) => Err(aura_core::AuraError::agent(
                                                                    format!(
                                                                        "Failed to import invitation: {error}"
                                                                    ),
                                                                )),
                                                                Err(_) => Err(aura_core::AuraError::internal(
                                                                    "Timed out importing invitation",
                                                                )),
                                                            }
                                                        }
                                                        None => Err(aura_core::AuraError::agent(
                                                            "Runtime bridge unavailable for harness invitation import",
                                                        )),
                                                    };

                                                match result {
                                                    Ok(imported_code) => {
                                                        if let Some(tx) = update_tx {
                                                            let _ = tx
                                                                .send(UiUpdate::InvitationImported {
                                                                    invitation_code: imported_code,
                                                                })
                                                                .await;
                                                        }
                                                    }
                                                    Err(error) => {
                                                        if let Some(tx) = update_tx {
                                                            let _ = tx
                                                                .send(UiUpdate::operation_failed(
                                                                    "ImportInvitation",
                                                                    error.to_string(),
                                                                ))
                                                                .await;
                                                        }
                                                    }
                                                }
                                            });
                                        } else {
                                            (cb.invitations.on_import)(code);
                                        }
                                    }
                                    DispatchCommand::ExportInvitation => {
                                        let selected = read_selected_notification(
                                            new_state.notifications.selected_index,
                                            &shared_invitations_for_dispatch,
                                            &shared_pending_requests_for_dispatch,
                                        );
                                        if let Some(NotificationSelection::SentInvitation(
                                            invitation_id,
                                        )) = selected
                                        {
                                            (cb.invitations.on_export)(invitation_id);
                                        } else {
                                            new_state.toast_error(
                                                "Select a sent invitation to export",
                                            );
                                        }
                                    }
                                    DispatchCommand::RevokeInvitation { invitation_id } => {
                                        (cb.invitations.on_revoke)(invitation_id.to_string());
                                    }

                                    // === Recovery Commands ===
                                    DispatchCommand::StartRecovery => {
                                        // Check recovery eligibility before starting
                                        let (threshold_k, _threshold_n) = shared_threshold_for_dispatch
                                            .read()
                                            .map(|guard| *guard)
                                            .unwrap_or((0, 0));

                                        // Check if threshold is configured
                                        if threshold_k == 0 {
                                            new_state.toast_error(
                                                RecoveryError::NoThresholdConfigured.to_string(),
                                            );
                                            continue;
                                        }

                                        // Check if we have enough guardians
                                        let guardian_count = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.iter().filter(|c| c.is_guardian).count())
                                            .unwrap_or(0);

                                        if guardian_count < threshold_k as usize {
                                            new_state.toast_error(
                                                RecoveryError::InsufficientGuardians {
                                                    required: threshold_k,
                                                    available: guardian_count,
                                                }
                                                .to_string(),
                                            );
                                            continue;
                                        }

                                        (cb.recovery.on_start_recovery)();
                                    }
                                    DispatchCommand::ApproveRecovery => {
                                        if let Some(NotificationSelection::RecoveryRequest(req_id)) =
                                            read_selected_notification(
                                                new_state.notifications.selected_index,
                                                &shared_invitations_for_dispatch,
                                                &shared_pending_requests_for_dispatch,
                                            )
                                        {
                                            (cb.recovery.on_submit_approval)(req_id);
                                        } else if let Ok(guard) =
                                            shared_pending_requests_for_dispatch.read()
                                        {
                                            if let Some(req) = guard.first() {
                                                (cb.recovery.on_submit_approval)(req.id.clone());
                                            } else {
                                                new_state.toast_error("No pending recovery requests");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read requests");
                                        }
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

                                        // Validate using type-safe ceremony error
                                        if current_contacts.is_empty() {
                                            new_state.toast_error(
                                                GuardianSetupError::NoContacts.to_string(),
                                            );
                                            continue;
                                        }

                                        // Populate candidates from current contacts
                                        // Note: nickname is already populated with nickname_suggestion if empty (see Contact::from)
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

                                        // Create populated modal state using factory
                                        let modal_state = crate::tui::state_machine::GuardianSetupModalState::from_contacts_with_selection(candidates, selected);

                                        // Enqueue the modal to new_state (not tui_state, which gets overwritten)
                                        new_state.modal_queue.enqueue(crate::tui::state_machine::QueuedModal::GuardianSetup(modal_state));
                                    }

                                    DispatchCommand::OpenMfaSetup => {
                                        let current_devices = shared_devices_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        // Validate using type-safe ceremony error
                                        if current_devices.len() < MIN_MFA_DEVICES {
                                            new_state.toast_error(
                                                MfaSetupError::InsufficientDevices {
                                                    required: MIN_MFA_DEVICES,
                                                    available: current_devices.len(),
                                                }
                                                .to_string(),
                                            );
                                            continue;
                                        }

                                        let candidates: Vec<crate::tui::state_machine::GuardianCandidate> = current_devices
                                            .iter()
                                            .map(|d| {
                                                let name = if d.name.is_empty() {
                                                    let short = d.id.chars().take(8).collect::<String>();
                                                    format!("Device {short}")
                                                } else {
                                                    d.name.clone()
                                                };
                                                crate::tui::state_machine::GuardianCandidate {
                                                    id: d.id.clone(),
                                                    name,
                                                    is_current_guardian: d.is_current,
                                                }
                                            })
                                            .collect();

                                        let n = candidates.len() as u8;
                                        let threshold_k = aura_app::ui::types::default_guardian_threshold(n);

                                        // Create modal state for MFA setup (pre-selects all, sets threshold)
                                        let modal_state =
                                            crate::tui::state_machine::GuardianSetupModalState::for_mfa_setup(candidates, threshold_k);

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::MfaSetup(modal_state),
                                        );
                                    }

                                    // === Guardian Ceremony Commands ===
                                    DispatchCommand::StartGuardianCeremony { contact_ids, threshold_k } => {
                                        tracing::info!(
                                            "Starting guardian ceremony with {} contacts, threshold {}",
                                            contact_ids.len(),
                                            threshold_k.get()
                                        );

                                        let ids = contact_ids.clone();
                                        let n = contact_ids.len() as u16;
                                        let k_raw = threshold_k.get() as u16;

                                        // Create FrostThreshold with validation (FROST requires k >= 2)
                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for guardian ceremony: {}", e);
                                                if let Some(tx) = update_tx_for_ceremony.clone() {
                                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                                        "guardian-ceremony-failed",
                                                        format!("Invalid threshold: {e}"),
                                                    )));
                                                }
                                                continue;
                                            }
                                        };

                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();

                                        let tasks = tasks_for_events.clone();
                                        let tasks_handle = tasks.clone();
                                        tasks_handle.spawn(async move {
                                            let app = app_core.raw();
                                            match start_guardian_ceremony(app, threshold, n, ids).await {
                                                Ok(ceremony_id) => {
                                                    let k = threshold.value();
                                                    tracing::info!(
                                                        ceremony_id = ?ceremony_id,
                                                        threshold = k,
                                                        guardians = n,
                                                        "Guardian ceremony initiated, waiting for guardian responses"
                                                    );

                                                    if let Some(tx) = update_tx.clone() {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                                            "guardian-ceremony-started",
                                                            format!(
                                                                "Guardian ceremony started! Waiting for {k}-of-{n} guardians to respond"
                                                            ),
                                                        )));

                                                        // Prime the modal with an initial status update so `ceremony_id` is
                                                        // available immediately for UI cancel.
                                                        let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                            ceremony_id: ceremony_id.to_string(),
                                                            kind: aura_app::ui::types::CeremonyKind::GuardianRotation,
                                                            accepted_count: 0,
                                                            total_count: n,
                                                            threshold: k,
                                                            is_complete: false,
                                                            has_failed: false,
                                                            accepted_participants: Vec::new(),
                                                            error_message: None,
                                                            pending_epoch: None,
                                                            agreement_mode: aura_core::threshold::policy_for(
                                                                aura_core::threshold::CeremonyFlow::GuardianSetupRotation,
                                                            )
                                                            .initial_mode(),
                                                            reversion_risk: true,
                                                        });
                                                    }

                                                    // Spawn a task to monitor ceremony progress.
                                                    let app_core_monitor = app.clone();
                                                    let update_tx_monitor = update_tx.clone();
                                                    let tasks = tasks.clone();
                                                    let tasks_handle = tasks;
                                                    tasks_handle.spawn(async move {
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            ceremony_id.clone(),
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                                        ceremony_id: status.ceremony_id.to_string(),
                                                                        kind: status.kind,
                                                                        accepted_count: status.accepted_count,
                                                                        total_count: status.total_count,
                                                                        threshold: status.threshold,
                                                                        is_complete: status.is_complete,
                                                                        has_failed: status.has_failed,
                                                                        accepted_participants: status.accepted_participants.clone(),
                                                                        error_message: status.error_message.clone(),
                                                                        pending_epoch: status.pending_epoch,
                                                                        agreement_mode: status.agreement_mode,
                                                                        reversion_risk: status.reversion_risk,
                                                                    });
                                                                }
                                                            },
                                                            tokio::time::sleep,
                                                        )
                                                        .await;
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Failed to initiate guardian ceremony: {}",
                                                        e
                                                    );

                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::operation_failed(
                                                            "Guardian ceremony",
                                                            e.to_string(),
                                                        ));
                                                    }
                                                }
                                            }
                                        });
                                    }

                                    DispatchCommand::StartMfaCeremony { device_ids, threshold_k } => {
                                        tracing::info!(
                                            "Starting multifactor ceremony with {} devices, threshold {}",
                                            device_ids.len(),
                                            threshold_k.get()
                                        );

                                        let ids = device_ids.clone();
                                        let n = device_ids.len() as u16;
                                        let k_raw = threshold_k.get() as u16;

                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for multifactor ceremony: {}", e);
                                                if let Some(tx) = update_tx_for_ceremony.clone() {
                                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                                        "mfa-ceremony-failed",
                                                        format!("Invalid threshold: {e}"),
                                                    )));
                                                }
                                                continue;
                                            }
                                        };

                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();

                                        let tasks = tasks_for_events.clone();
                                        let tasks_handle = tasks.clone();
                                        tasks_handle.spawn(async move {
                                            let app = app_core.raw();

                                            match start_device_threshold_ceremony(
                                                app,
                                                threshold,
                                                n,
                                                ids.iter().map(|id| id.to_string()).collect(),
                                            )
                                            .await
                                            {
                                                Ok(ceremony_id) => {
                                                    let k = threshold.value();
                                                    tracing::info!(
                                                        "Multifactor ceremony initiated: {} ({}-of-{})",
                                                        ceremony_id,
                                                        k,
                                                        n
                                                    );

                                                    if let Some(tx) = update_tx.clone() {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                                            "mfa-ceremony-started",
                                                            format!(
                                                                "Multifactor ceremony started ({k}-of-{n})"
                                                            ),
                                                        )));
                                                    }

                                                    if let Some(tx) = update_tx.clone() {
                                                        let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                            ceremony_id: ceremony_id.to_string(),
                                                            kind: aura_app::ui::types::CeremonyKind::DeviceRotation,
                                                            accepted_count: 0,
                                                            total_count: n,
                                                            threshold: k,
                                                            is_complete: false,
                                                            has_failed: false,
                                                            accepted_participants: Vec::new(),
                                                            error_message: None,
                                                            pending_epoch: None,
                                                            agreement_mode: aura_core::threshold::policy_for(
                                                                aura_core::threshold::CeremonyFlow::DeviceMfaRotation,
                                                            )
                                                            .initial_mode(),
                                                            reversion_risk: true,
                                                        });
                                                    }

                                                    let app_core_monitor = app.clone();
                                                    let update_tx_monitor = update_tx.clone();
                                                    let tasks = tasks.clone();
                                                    let tasks_handle = tasks;
                                                    tasks_handle.spawn(async move {
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            ceremony_id.clone(),
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                                        ceremony_id: status.ceremony_id.to_string(),
                                                                        kind: status.kind,
                                                                        accepted_count: status.accepted_count,
                                                                        total_count: status.total_count,
                                                                        threshold: status.threshold,
                                                                        is_complete: status.is_complete,
                                                                        has_failed: status.has_failed,
                                                                        accepted_participants: status.accepted_participants.clone(),
                                                                        error_message: status.error_message.clone(),
                                                                        pending_epoch: status.pending_epoch,
                                                                        agreement_mode: status.agreement_mode,
                                                                        reversion_risk: status.reversion_risk,
                                                                    });
                                                                }
                                                            },
                                                            tokio::time::sleep,
                                                        )
                                                        .await;
                                                    });
                                                }
                                                Err(e) => {
                                                    tracing::error!(
                                                        "Failed to initiate multifactor ceremony: {}",
                                                        e
                                                    );

                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::operation_failed(
                                                            "Multifactor ceremony",
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

                                        let tasks = tasks_for_events.clone();
                                        tasks.spawn(async move {
                                            let app = app_core.raw();

                                            let ceremony_id_typed = CeremonyId::new(ceremony_id.clone());
                                            if let Err(e) = cancel_key_rotation_ceremony(app, &ceremony_id_typed).await {
                                                tracing::error!("Failed to cancel guardian ceremony: {}", e);
                                                if let Some(tx) = update_tx.clone() {
                                                    let _ = tx.try_send(UiUpdate::operation_failed(
                                                        "Cancel guardian ceremony",
                                                        e.to_string(),
                                                    ));
                                                }
                                                return;
                                            }

                                            if let Some(tx) = update_tx {
                                                let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                                    "guardian-ceremony-canceled",
                                                    "Guardian ceremony canceled",
                                                )));
                                            }
                                        });
                                    }
                                    DispatchCommand::CancelKeyRotationCeremony { ceremony_id } => {
                                        tracing::info!(ceremony_id = %ceremony_id, "Canceling ceremony");

                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();

                                        let tasks = tasks_for_events.clone();
                                        tasks.spawn(async move {
                                            let app = app_core.raw();

                                            let ceremony_id_typed = CeremonyId::new(ceremony_id.clone());
                                            if let Err(e) = cancel_key_rotation_ceremony(app, &ceremony_id_typed).await {
                                                tracing::error!("Failed to cancel ceremony: {}", e);
                                                if let Some(tx) = update_tx.clone() {
                                                    let _ = tx.try_send(UiUpdate::operation_failed(
                                                        "Cancel ceremony",
                                                        e.to_string(),
                                                    ));
                                                }
                                                return;
                                            }

                                            if let Some(tx) = update_tx {
                                                let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::info(
                                                    "ceremony-canceled",
                                                    "Ceremony canceled",
                                                )));
                                            }
                                        });
                                    }

                                    // === Settings Screen Commands ===
                                    DispatchCommand::UpdateNicknameSuggestion { nickname_suggestion } => {
                                        (cb.settings.on_update_nickname_suggestion)(nickname_suggestion);
                                    }
                                    DispatchCommand::UpdateMfaPolicy { policy } => {
                                        (cb.settings.on_update_mfa)(policy);
                                    }
                                    DispatchCommand::AddDevice { name, invitee_authority_id } => {
                                        (cb.settings.on_add_device)(name, invitee_authority_id);
                                    }
                                    DispatchCommand::RemoveDevice { device_id } => {
                                        (cb.settings.on_remove_device)(device_id);
                                    }
                                    DispatchCommand::OpenDeviceSelectModal => {
                                        let current_devices = shared_devices_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        if current_devices.is_empty() {
                                            new_state.toast_info("No devices to remove");
                                            continue;
                                        }

                                        // Check if there are any non-current devices
                                        let has_removable = current_devices.iter().any(|d| !d.is_current);
                                        if !has_removable {
                                            new_state.toast_info("Cannot remove the current device");
                                            continue;
                                        }

                                        // Convert to Device type for the modal
                                        let devices: Vec<crate::tui::types::Device> = current_devices
                                            .iter()
                                            .map(|d| crate::tui::types::Device {
                                                id: d.id.clone(),
                                                name: if d.name.is_empty() {
                                                    let short = d.id.chars().take(8).collect::<String>();
                                                    format!("Device {short}")
                                                } else {
                                                    d.name.clone()
                                                },
                                                is_current: d.is_current,
                                                last_seen: d.last_seen,
                                            })
                                            .collect();

                                        let modal_state = crate::tui::state_machine::DeviceSelectModalState::with_devices(devices);
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::SettingsDeviceSelect(modal_state),
                                        );
                                    }
                                    DispatchCommand::ImportDeviceEnrollmentOnMobile { code } => {
                                        (cb.settings.on_import_device_enrollment_on_mobile)(code);
                                    }
                                    DispatchCommand::OpenAuthorityPicker => {
                                        // Build list of authorities from app-global state
                                        let authorities = new_state.authorities.clone();
                                        if authorities.len() <= 1 {
                                            new_state.toast_info("Only one authority available");
                                        } else {
                                            // Convert authorities to contact-like format for picker
                                            let contacts: Vec<(crate::tui::state_machine::AuthorityRef, String)> = authorities
                                                .iter()
                                                .map(|a| (a.id.clone().into(), format!("{} ({})", a.nickname_suggestion, a.short_id)))
                                                .collect();

                                            let modal_state = crate::tui::state_machine::ContactSelectModalState::single(
                                                "Select Authority",
                                                contacts,
                                            );
                                            new_state.modal_queue.enqueue(
                                                crate::tui::state_machine::QueuedModal::AuthorityPicker(modal_state),
                                            );
                                        }
                                    }
                                    DispatchCommand::SwitchAuthority { authority_id } => {
                                        let authority_id_str = authority_id.to_string();
                                        if let Some(idx) = new_state.authorities
                                            .iter()
                                            .position(|a| a.id == authority_id_str)
                                        {
                                            let nickname = new_state
                                                .authorities
                                                .get(idx)
                                                .and_then(|auth| {
                                                    if auth.nickname_suggestion.trim().is_empty() {
                                                        None
                                                    } else {
                                                        Some(auth.nickname_suggestion.clone())
                                                    }
                                                });
                                            new_state.current_authority_index = idx;
                                            app_ctx_for_dispatch.request_authority_switch(
                                                authority_id,
                                                nickname.clone(),
                                            );
                                            new_state.modal_queue.dismiss();
                                            new_state.toast_info("Reloading selected authority");
                                            new_state.should_exit = true;
                                        } else {
                                            new_state.toast_error("Authority not found");
                                        }
                                    }
                                    // Note: Threshold/guardian changes now use OpenGuardianSetup
                                    // which is handled above with the guardian ceremony commands.

                                    // === Neighborhood Screen Commands ===
                                    DispatchCommand::EnterHome => {
                                        let idx = new_state.neighborhood.grid.current();
                                        if let Ok(guard) = shared_neighborhood_homes_for_dispatch.read() {
                                            if let Some(home_id) = guard.get(idx) {
                                                // Keep entered_home_id authoritative as a real home ID.
                                                // The state-machine layer sets an index sentinel first.
                                                new_state.neighborhood.entered_home_id = Some(home_id.clone());
                                                // Default to Limited-level traversal depth
                                                (cb.neighborhood.on_enter_home)(
                                                    home_id.clone(),
                                                    new_state.neighborhood.enter_depth,
                                                );
                                            } else {
                                                new_state.toast_error("No home selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read neighborhood homes");
                                        }
                                    }
                                    DispatchCommand::GoHome => {
                                        (cb.neighborhood.on_go_home)();
                                    }
                                    DispatchCommand::BackToLimited => {
                                        (cb.neighborhood.on_back_to_limited)();
                                    }
                                    DispatchCommand::OpenHomeCreate => {
                                        // Open home creation modal
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::NeighborhoodHomeCreate(
                                                crate::tui::state_machine::HomeCreateModalState::new(),
                                            ),
                                        );
                                    }
                                    DispatchCommand::OpenModeratorAssignmentModal => {
                                        let contacts = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::NeighborhoodModeratorAssignment(
                                                crate::tui::state_machine::ModeratorAssignmentModalState::new(
                                                    contacts,
                                                ),
                                            ),
                                        );
                                    }
                                    DispatchCommand::SubmitModeratorAssignment { target_id, assign } => {
                                        (cb.neighborhood.on_set_moderator)(
                                            new_state.neighborhood.entered_home_id.clone(),
                                            target_id.to_string(),
                                            assign,
                                        );
                                        new_state.modal_queue.dismiss();
                                    }
                                    DispatchCommand::OpenAccessOverrideModal => {
                                        let contacts = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::NeighborhoodAccessOverride(
                                                crate::tui::state_machine::AccessOverrideModalState::new(
                                                    contacts,
                                                ),
                                            ),
                                        );
                                    }
                                    DispatchCommand::SubmitAccessOverride {
                                        target_id,
                                        access_level,
                                    } => {
                                        new_state.modal_queue.dismiss();
                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();
                                        let home_id = new_state.neighborhood.entered_home_id.clone();
                                        let target_for_toast = target_id.clone();
                                        let tasks = tasks_for_events.clone();
                                        tasks.spawn(async move {
                                            match access_workflows::set_access_override(
                                                app_core.raw(),
                                                home_id.as_deref(),
                                                target_id,
                                                access_level.into(),
                                            )
                                            .await
                                            {
                                                Ok(()) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::success(
                                                                "access-override",
                                                                format!(
                                                                    "Access override set for {}: {}",
                                                                    target_for_toast,
                                                                    access_level.label()
                                                                ),
                                                            ),
                                                        ));
                                                    }
                                                }
                                                Err(error) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::error(
                                                                "access-override",
                                                                format!(
                                                                    "Failed to set access override: {error}"
                                                                ),
                                                            ),
                                                        ));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    DispatchCommand::OpenHomeCapabilityConfigModal => {
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::NeighborhoodCapabilityConfig(
                                                crate::tui::state_machine::HomeCapabilityConfigModalState::default(),
                                            ),
                                        );
                                    }
                                    DispatchCommand::SubmitHomeCapabilityConfig { config } => {
                                        new_state.modal_queue.dismiss();
                                        let app_core = app_core_for_ceremony.clone();
                                        let update_tx = update_tx_for_ceremony.clone();
                                        let home_id = new_state.neighborhood.entered_home_id.clone();
                                        let tasks = tasks_for_events.clone();
                                        tasks.spawn(async move {
                                            match access_workflows::configure_home_capabilities(
                                                app_core.raw(),
                                                home_id.as_deref(),
                                                &config.full_csv(),
                                                &config.partial_csv(),
                                                &config.limited_csv(),
                                            )
                                            .await
                                            {
                                                Ok(()) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::success(
                                                                "capability-config",
                                                                "Capability config saved",
                                                            ),
                                                        ));
                                                    }
                                                }
                                                Err(error) => {
                                                    if let Some(tx) = update_tx {
                                                        let _ = tx.try_send(UiUpdate::ToastAdded(
                                                            ToastMessage::error(
                                                                "capability-config",
                                                                format!(
                                                                    "Failed to save capability config: {error}"
                                                                ),
                                                            ),
                                                        ));
                                                    }
                                                }
                                            }
                                        });
                                    }
                                    DispatchCommand::CreateHome { name, description } => {
                                        (cb.neighborhood.on_create_home)(name, description);
                                        new_state.modal_queue.dismiss();
                                    }
                                    DispatchCommand::CreateNeighborhood { name } => {
                                        (cb.neighborhood.on_create_neighborhood)(name);
                                    }
                                    DispatchCommand::AddSelectedHomeToNeighborhood => {
                                        let idx = new_state.neighborhood.grid.current();
                                        if let Ok(guard) = shared_neighborhood_homes_for_dispatch.read() {
                                            if let Some(home_id) = guard.get(idx) {
                                                (cb.neighborhood.on_add_home_to_neighborhood)(
                                                    home_id.clone(),
                                                );
                                            } else {
                                                new_state.toast_error("No home selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read neighborhood homes");
                                        }
                                    }
                                    DispatchCommand::AddHomeToNeighborhood { target } => {
                                        (cb.neighborhood.on_add_home_to_neighborhood)(
                                            target.as_command_arg(),
                                        );
                                    }
                                    DispatchCommand::LinkSelectedHomeOneHopLink => {
                                        let idx = new_state.neighborhood.grid.current();
                                        if let Ok(guard) = shared_neighborhood_homes_for_dispatch.read() {
                                            if let Some(home_id) = guard.get(idx) {
                                                (cb.neighborhood.on_link_home_one_hop_link)(
                                                    home_id.clone(),
                                                );
                                            } else {
                                                new_state.toast_error("No home selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read neighborhood homes");
                                        }
                                    }
                                    DispatchCommand::LinkHomeOneHopLink { target } => {
                                        (cb.neighborhood.on_link_home_one_hop_link)(
                                            target.as_command_arg(),
                                        );
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

                // Sync final TuiState changes to iocraft hooks.
                // Important: dispatch commands above can mutate `new_state.router` and
                // `new_state.should_exit`, so synchronization must happen after command execution.
                if new_state.screen() != screen.get() {
                    screen.set(new_state.screen());
                }
                if new_state.should_exit && !should_exit.get() {
                    should_exit.set(true);
                }

                // Update TuiState (and always bump render version)
                tui.replace(new_state);
            }

            // All key events are handled by the state machine above.
            // Modal handling goes through transition() -> command execution.
        }
    });

    // Nav bar status is updated reactively from signals.
    let network_status = nav_signals.network_status.get();
    let now_ms = nav_signals.now_ms.get();
    let transport_peers = nav_signals.transport_peers.get();
    let known_online = nav_signals.known_online.get();

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
                    Screen::Chat => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ChatScreen(
                                view: chat_props.clone(),
                                selected_channel: Some(tui_selected_for_chat_screen),
                                selected_channel_id: Some(selected_channel_id),
                                shared_channels: Some(shared_channels),
                                on_send: on_send.clone(),
                                on_retry_message: on_retry_message.clone(),
                                on_channel_select: on_channel_select.clone(),
                                on_create_channel: on_create_channel.clone(),
                                on_set_topic: on_set_topic.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    }],
                    Screen::Contacts => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            ContactsScreen(
                                view: contacts_props.clone(),
                                now_ms: now_ms,
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
                                view: neighborhood_props.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            SettingsScreen(
                                view: settings_props.clone(),
                                on_update_mfa: on_update_mfa.clone(),
                                on_update_nickname_suggestion: on_update_nickname_suggestion.clone(),
                                on_update_threshold: on_update_threshold.clone(),
                                on_add_device: on_add_device.clone(),
                                on_remove_device: on_remove_device.clone(),
                            )
                        }
                    }],
                    Screen::Notifications => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            NotificationsScreen(
                                view: notifications_props.clone(),
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
                network_status: network_status.clone(),
                now_ms: now_ms,
                transport_peers: transport_peers,
                known_online: known_online,
                state_indicator: Some(state_indicator),
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
            #(render_contacts_code_modal(&contacts_props))
            #(render_guardian_setup_modal(&contacts_props))

            // === CHAT SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_chat_create_modal(&chat_props))
            #(render_topic_modal(&chat_props))
            #(render_channel_info_modal(&chat_props))

            // === SETTINGS SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            // Note: Threshold changes now use OpenGuardianSetup (see contacts screen modals)
            #(render_nickname_suggestion_modal(&settings_props))
            #(render_add_device_modal(&settings_props))
            #(render_device_import_modal(&settings_props))
            #(render_device_enrollment_modal(&settings_props))
            #(render_device_select_modal(&settings_props))
            #(render_remove_device_modal(&settings_props))
            #(render_mfa_setup_modal(&settings_props))

            // === NEIGHBORHOOD SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_home_create_modal(&neighborhood_props))
            #(render_moderator_assignment_modal(&neighborhood_props))
            #(render_access_override_modal(&neighborhood_props))
            #(render_capability_config_modal(&neighborhood_props))

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
    // Reactive Pattern: All data is provided via signals, not polling.
    // Props below are intentionally empty seeds that are overwritten on mount.
    // ========================================================================
    // Screens subscribe to their respective signals and update reactively:
    // - ChatScreen subscribes to CHAT_SIGNAL
    // - NotificationsScreen subscribes to INVITATIONS_SIGNAL + RECOVERY_SIGNAL
    // - ContactsScreen subscribes to CONTACTS_SIGNAL + DISCOVERED_PEERS_SIGNAL
    // - NeighborhoodScreen subscribes to NEIGHBORHOOD_SIGNAL + HOMES_SIGNAL + CHAT_SIGNAL + CONTACTS_SIGNAL
    // - SettingsScreen subscribes to SETTINGS_SIGNAL (+ RECOVERY_SIGNAL for recovery data)
    //
    // Props passed below are ONLY used as empty/default initial values.
    // Screens ignore these and use signal data immediately on mount.

    let channels = Vec::new();
    let messages = Vec::new();
    let guardians = Vec::new();
    let invitations = Vec::new();
    let contacts = Vec::new();
    let discovered_peers: Vec<DiscoveredPeerInfo> = Vec::new();

    // Neighborhood data - reactively updated via signals
    let neighborhood_name = String::from("Neighborhood");
    let homes: Vec<HomeSummary> = Vec::new();

    // Settings data - reactively updated via SETTINGS_SIGNAL
    let devices = Vec::new();
    let nickname_suggestion = {
        let reactive = {
            let core = ctx_arc.app_core_raw().read().await;
            core.reactive().clone()
        };
        reactive
            .read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .nickname_suggestion
    };
    let threshold_k = 0;
    let threshold_n = 0;

    // Status bar values are updated reactively after mount.
    // Avoid blocking before entering fullscreen (important for demo mode).
    let network_status = NetworkStatus::Disconnected;
    let transport_peers: usize = 0;
    let known_online: usize = 0;

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());
    let runtime_bridge = {
        let core = ctx_arc.app_core_raw().read().await;
        core.runtime().cloned()
    };

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
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        nickname_suggestion: nickname_suggestion,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        access_level: AccessLevel::Limited,
                        // Account setup
                        show_account_setup: show_account_setup,
                        // Network status
                        network_status: network_status.clone(),
                        transport_peers: transport_peers,
                        known_online: known_online,
                        // Demo mode (get from context)
                        demo_mode: ctx_arc.is_demo_mode(),
                        demo_alice_code: ctx_arc.demo_alice_code(),
                        demo_carol_code: ctx_arc.demo_carol_code(),
                        demo_mobile_device_id: ctx_arc.demo_mobile_device_id(),
                        demo_mobile_authority_id: ctx_arc.demo_mobile_authority_id(),
                        // Reactive update channel
                        update_rx: Some(update_rx_holder),
                        update_tx: Some(update_tx.clone()),
                        // Callbacks registry
                        callbacks: Some(callbacks),
                        runtime_bridge: runtime_bridge.clone(),
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
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        nickname_suggestion: nickname_suggestion,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        access_level: AccessLevel::Limited,
                        // Account setup
                        show_account_setup: show_account_setup,
                        // Network status
                        network_status: network_status,
                        transport_peers: transport_peers,
                        known_online: known_online,
                        // Reactive update channel
                        update_rx: Some(update_rx_holder),
                        update_tx: Some(update_tx.clone()),
                        // Callbacks registry
                        callbacks: Some(callbacks),
                        runtime_bridge: runtime_bridge.clone(),
                    )
                }
            }
        };

        app.fullscreen().await
    }
}

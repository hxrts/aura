// Allow field reassignment for large structs with many conditional fields
#![allow(clippy::field_reassign_with_default)]
// Allow manual map patterns in element! macro contexts for clarity
#![allow(clippy::manual_map)]

use self::dispatch::submit_ceremony_operation;
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

use aura_app::ui_contract::{OperationId, SemanticOperationKind};
use iocraft::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

use aura_app::ceremonies::{
    ChannelError, GuardianSetupError, MfaSetupError, RecoveryError, MIN_CHANNEL_PARTICIPANTS,
    MIN_MFA_DEVICES,
};
use aura_app::harness_mode_enabled;
use aura_app::scenario_contract::SemanticCommandValue;
use aura_app::ui::contract::HarnessUiCommand;
use aura_app::ui::contract::OperationState;
use aura_app::ui::prelude::*;
use aura_app::ui::signals::{NetworkStatus, ERROR_SIGNAL, SETTINGS_SIGNAL};
use aura_app::ui::workflows::network as network_workflows;
use aura_app::ui::workflows::runtime as runtime_workflows;
use aura_app::ui::workflows::settings::refresh_settings_from_runtime;
use aura_app::ui::workflows::system as system_workflows;
use aura_app::ui_contract::{RuntimeEventKind, RuntimeFact};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::{execute_with_retry_budget, RetryRunError};
use aura_effects::time::PhysicalTimeHandler;

use crate::error::TerminalError;
use crate::tui::callbacks::CallbackRegistry;
use crate::tui::channel_selection::{CommittedChannelSelection, SharedCommittedChannelSelection};
use crate::tui::components::{DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastMessage};
use crate::tui::context::{IoContext, ShellExitIntent};
use crate::tui::harness_state::{
    accept_harness_command_submission, apply_harness_command, clear_harness_command_sender,
    complete_pending_semantic_submission, ensure_harness_command_listener,
    fail_pending_semantic_submission, maybe_export_ui_snapshot, publish_loading_ui_snapshot,
    register_harness_command_sender, reject_harness_command_submission,
    track_pending_semantic_submission, PendingSemanticValueKind, TuiSemanticInputs,
};
use crate::tui::hooks::{AppCoreContext, AppSnapshotAvailability, CallbackContext};
use crate::tui::keymap::{global_footer_hints, screen_footer_hints};
use crate::tui::layout::dim;
use crate::tui::navigation::clamp_list_index;
use crate::tui::screens::app::subscriptions::{
    use_authoritative_semantic_facts_subscription, use_authority_id_subscription,
    use_channels_subscription, use_contacts_subscription, use_devices_subscription,
    use_discovered_peers_subscription, use_invitations_subscription, use_messages_subscription,
    use_nav_status_signals, use_neighborhood_home_meta_subscription,
    use_neighborhood_homes_subscription, use_notifications_subscription,
    use_pending_requests_subscription, use_threshold_subscription, SharedDevices,
};
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    ChatScreen, ContactsScreen, NeighborhoodScreen, NotificationsScreen, SettingsScreen,
};
use crate::tui::state::InvitationKind;
use crate::tui::types::{
    AccessLevel, Channel, Contact, Device, Guardian, HomeSummary, Invitation, Message, MfaPolicy,
};
// State machine integration
use crate::tui::iocraft_adapter::convert_iocraft_event;
use crate::tui::props::{
    extract_chat_view_props, extract_contacts_view_props, extract_neighborhood_view_props,
    extract_notifications_view_props, extract_settings_view_props,
};
use crate::tui::state::{transition, DispatchCommand, QueuedModal, TuiCommand, TuiState};
use crate::tui::timeout_support::{execute_with_terminal_timeout, TerminalTimeoutError};
use crate::tui::updates::{harness_command_channel, ui_update_channel, UiUpdate, UiUpdateSender};
use std::sync::Mutex;
use std::time::Duration;

mod dispatch;
mod dispatch_command_handlers;
mod dispatch_handlers_neighborhood;
mod events;
mod input;
mod props;
mod render;
mod runtime;
mod runtime_support;
mod state;
mod update_handlers;
mod updates;
use dispatch::{
    authoritative_binding_for_requested_join, complete_ready_join_binding_submissions,
    execute_harness_followup_command, handle_dispatch_command, terminal_error_to_toast_level,
    EventCommandLoopAction, EventDispatchContext, HarnessDispatchContext,
};
use events::{handle_channel_selection_change, resolve_committed_selected_channel_id};
use input::transition_from_terminal_event;
use props::{IoAppProps, RuntimeShellPropsSeed};
use render::{build_global_modals, state_indicator_label};
use runtime::build_runtime_app;
use runtime_support::{
    authoritative_app_snapshot_with_retry, authoritative_settings_authorities_for_command,
    authoritative_settings_devices_for_command, effect_sleep, shell_retry_policy,
};
use state::{sync_neighborhood_navigation_state, TuiStateHandle};
use updates::{process_ui_update, UiUpdateContext, UiUpdateLoopAction};

///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<ShellExitIntent> {
    let (update_tx, update_rx) = ui_update_channel();
    let update_rx_holder = Arc::new(Mutex::new(Some(update_rx)));
    let (harness_command_tx, harness_command_rx) = harness_command_channel();
    let harness_command_rx_holder = Arc::new(Mutex::new(Some(harness_command_rx)));
    let (bootstrap_handoff_tx, bootstrap_handoff_rx) = tokio::sync::oneshot::channel();
    let bootstrap_handoff_tx_holder = Arc::new(Mutex::new(Some(bootstrap_handoff_tx)));
    ctx.clear_bootstrap_runtime_handoff_committed()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    ensure_harness_command_listener().await?;
    register_harness_command_sender(harness_command_tx).await?;

    let ctx_arc = Arc::new(ctx);
    let callbacks = CallbackRegistry::new(ctx_arc.clone(), update_tx.clone());
    let callback_context = CallbackContext::new(callbacks.clone());
    let show_account_setup = !ctx_arc.has_account();

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

    let app_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());
    let io_app_props = IoAppProps::from_runtime_seed(RuntimeShellPropsSeed {
        nickname_suggestion,
        show_account_setup,
        pending_runtime_bootstrap: ctx_arc.pending_runtime_bootstrap(),
        update_rx: update_rx_holder,
        harness_command_rx: harness_command_rx_holder,
        bootstrap_handoff_tx: bootstrap_handoff_tx_holder.clone(),
        update_tx: update_tx.clone(),
        callbacks,
        #[cfg(feature = "development")]
        demo_mode: ctx_arc.is_demo_mode(),
        #[cfg(feature = "development")]
        demo_alice_code: ctx_arc.demo_alice_code(),
        #[cfg(feature = "development")]
        demo_carol_code: ctx_arc.demo_carol_code(),
        #[cfg(feature = "development")]
        demo_mobile_device_id: ctx_arc.demo_mobile_device_id(),
        #[cfg(feature = "development")]
        demo_mobile_authority_id: ctx_arc.demo_mobile_authority_id(),
    });
    let mut app = build_runtime_app(app_context, callback_context, io_app_props);
    let result = if show_account_setup {
        let app_future = app.fullscreen();
        tokio::pin!(app_future);
        tokio::select! {
            result = &mut app_future => result,
            result = async {
                bootstrap_handoff_rx.await.map_err(|error| {
                    std::io::Error::other(format!(
                        "bootstrap runtime handoff notification dropped before shell exit: {error}"
                    ))
                })
            } => {
                result?;
                if !ctx_arc.bootstrap_runtime_handoff_committed() {
                    return Err(std::io::Error::other(
                        "bootstrap runtime handoff notified without committed marker",
                    ));
                }
                match execute_with_terminal_timeout(
                    "bootstrap_runtime_handoff_exit",
                    Duration::from_secs(5),
                    || async { app_future.as_mut().await },
                )
                .await
                {
                    Ok(result) => Ok(result),
                    Err(TerminalTimeoutError::Timeout { .. }) => Err(std::io::Error::other(
                        "bootstrap runtime handoff committed but fullscreen generation did not exit within 5s",
                    )),
                    Err(TerminalTimeoutError::Setup { context, detail }) => {
                        Err(std::io::Error::other(format!(
                            "{context}: failed to configure bounded bootstrap exit wait: {detail}"
                        )))
                    }
                    Err(TerminalTimeoutError::Operation(error)) => Err(error),
                }
            }
        }
    } else {
        app.fullscreen().await
    };
    let _ = clear_harness_command_sender().await;
    result?;
    ctx_arc.take_shell_exit_intent().ok_or_else(|| {
        std::io::Error::other(
            "fullscreen generation exited without explicit ShellExitIntent; see docs/122_ownership_model.md",
        )
    })
}

pub(super) fn request_bootstrap_reload(io_ctx: &IoContext) {
    io_ctx.request_bootstrap_reload();
}

#[component]
pub fn IoApp(props: &IoAppProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let screen = hooks.use_state(|| Screen::Neighborhood);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    let bg_shutdown =
        hooks.use_ref(|| std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));

    let show_setup = props.show_account_setup;
    let pending_runtime_bootstrap = props.pending_runtime_bootstrap;
    let bootstrap_handoff_tx = props.bootstrap_handoff_tx.clone();
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
            state.pending_runtime_bootstrap = pending_runtime_bootstrap;
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
                let mut state = TuiState::with_account_setup();
                state.pending_runtime_bootstrap = pending_runtime_bootstrap;
                state
            } else {
                let mut state = TuiState::new();
                state.pending_runtime_bootstrap = pending_runtime_bootstrap;
                state
            }
        }
    });
    let tui_state_version = hooks.use_state(|| 0usize);
    let tui = TuiStateHandle::new(tui_state.clone(), tui_state_version.clone());

    let update_rx_holder = props.update_rx.clone();
    let harness_command_rx_holder = props.harness_command_rx.clone();
    let update_tx_holder = props.update_tx.clone();
    let update_tx_for_commands = update_tx_holder.clone();
    let update_tx_for_events = update_tx_holder.clone();

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
    let projection_export_version = hooks.use_state(|| 0usize);

    // =========================================================================
    // Contacts subscription: SharedContacts for dispatch handlers to read
    // =========================================================================
    // Unlike props.contacts (which is empty), this Arc is kept up-to-date
    // by a reactive subscription. Dispatch handler closures capture the Arc,
    // not the data, so they always read current contacts.
    // Also sends ContactCountChanged updates to keep TuiState in sync for navigation.
    let shared_contacts = use_contacts_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    let shared_discovered_peers =
        use_discovered_peers_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Authority subscription: current authority id for dispatch handlers
    // =========================================================================
    let shared_authority_id =
        use_authority_id_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Shared selected channel identity for subscriptions and dispatch
    // =========================================================================
    let tui_selected_ref = hooks.use_ref(|| {
        std::sync::Arc::new(parking_lot::RwLock::new(None::<CommittedChannelSelection>))
    });
    let tui_selected: SharedCommittedChannelSelection = tui_selected_ref.read().clone();
    let last_exported_devices_ref =
        hooks.use_ref(|| std::sync::Arc::new(parking_lot::RwLock::new(Vec::<Device>::new())));
    let last_exported_devices: std::sync::Arc<parking_lot::RwLock<Vec<Device>>> =
        last_exported_devices_ref.read().clone();
    let ready_join_channel_instances_ref =
        hooks.use_ref(|| Arc::new(Mutex::new(HashSet::<String>::new())));
    let ready_join_channel_instances = ready_join_channel_instances_ref.read().clone();
    // =========================================================================
    // Channels subscription: SharedChannels for dispatch handlers to read
    // =========================================================================
    // Must be created before messages subscription since messages depend on channels
    let shared_channels = use_channels_subscription(
        &mut hooks,
        &app_ctx,
        shared_authority_id.clone(),
        tui_selected.clone(),
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );

    // =========================================================================
    // Messages subscription: SharedMessages for dispatch handlers to read
    // =========================================================================
    // Used to look up failed messages by ID for retry operations.
    // The Arc is kept up-to-date by a reactive subscription to CHAT_SIGNAL.
    let shared_messages = use_messages_subscription(
        &mut hooks,
        &app_ctx,
        tui_selected.clone(),
        projection_export_version.clone(),
    );

    // Clone for ChatScreen to compute per-channel message counts
    let tui_selected_for_chat_screen = tui_selected.clone();

    // =========================================================================
    // Devices subscription: SharedDevices for dispatch handlers to read
    // =========================================================================
    let shared_devices = use_devices_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    let callbacks_ref =
        hooks.use_ref(|| Arc::new(parking_lot::RwLock::new(props.callbacks.clone())));
    let shared_callbacks = callbacks_ref.read().clone();
    *shared_callbacks.write() = props.callbacks.clone();

    // =========================================================================
    // Invitations subscription: SharedInvitations for notification action dispatch
    // =========================================================================
    let shared_invitations = use_invitations_subscription(
        &mut hooks,
        &app_ctx,
        update_tx_holder.clone(),
        projection_export_version.clone(),
    );
    use_authoritative_semantic_facts_subscription(&mut hooks, &app_ctx, update_tx_holder.clone());

    // =========================================================================
    // Neighborhood homes subscription: SharedNeighborhoodHomes for dispatch handlers to read
    // =========================================================================
    let shared_neighborhood_homes = use_neighborhood_homes_subscription(
        &mut hooks,
        &app_ctx,
        projection_export_version.clone(),
    );
    let shared_neighborhood_home_meta = use_neighborhood_home_meta_subscription(
        &mut hooks,
        &app_ctx,
        projection_export_version.clone(),
    );

    // =========================================================================
    // Pending requests subscription: SharedPendingRequests for dispatch handlers to read
    // =========================================================================
    let shared_pending_requests =
        use_pending_requests_subscription(&mut hooks, &app_ctx, projection_export_version.clone());

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
        let shutdown = bg_shutdown.read().clone();
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
                            let toast = crate::tui::state::QueuedToast::new(
                                toast_id,
                                msg,
                                crate::tui::state::ToastLevel::Error,
                            );
                            state.toast_queue.enqueue(toast);
                        }
                    });
                }
            }

            let retry_policy = shell_retry_policy();
            let time = PhysicalTimeHandler::new();
            let result = execute_with_retry_budget(&time, &retry_policy, |_attempt| {
                let app_core = app_core.clone();
                let mut tui = tui.clone();
                let shutdown = shutdown.clone();
                async move {
                    if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                        return Ok::<(), String>(());
                    }

                    let mut stream = {
                        let core = app_core.raw().read().await;
                        core.subscribe(&*ERROR_SIGNAL)
                            .map_err(|e| format!("error signal subscription failed: {e}"))?
                    };

                    while let Ok(err_opt) = stream.recv().await {
                        if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                            return Ok(());
                        }
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
                                let toast = crate::tui::state::QueuedToast::new(
                                    toast_id,
                                    msg,
                                    crate::tui::state::ToastLevel::Error,
                                );
                                state.toast_queue.enqueue(toast);
                            }
                        });
                    }

                    Err("ERROR_SIGNAL subscription stream ended".to_string())
                }
            })
            .await;

            match result {
                Ok(()) => {}
                Err(RetryRunError::AttemptsExhausted {
                    attempts_used,
                    last_error,
                }) => tracing::warn!(
                    attempts_used,
                    last_error,
                    "ERROR_SIGNAL subscription abandoned after max retries"
                ),
                Err(RetryRunError::Timeout(error)) => tracing::warn!(
                    error = %error,
                    "ERROR_SIGNAL retry budget timed out"
                ),
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
        let shutdown = bg_shutdown.read().clone();
        async move {
            loop {
                effect_sleep(std::time::Duration::from_millis(100)).await;
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
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
        let shutdown = bg_shutdown.read().clone();
        async move {
            loop {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                let timestamp_ms =
                    match aura_app::ui::workflows::time::current_time_ms(&app_core).await {
                        Ok(ts) => ts,
                        Err(e) => {
                            tracing::debug!(error = %e, "current_time_ms failed in peer discovery");
                            0
                        }
                    };
                if let Err(e) = network_workflows::discover_peers(&app_core, timestamp_ms).await {
                    tracing::debug!(error = %e, "discover_peers failed");
                }
                effect_sleep(network_workflows::DISCOVERED_PEERS_REFRESH_INTERVAL).await;
            }
        }
    });

    // =========================================================================
    // Harness Runtime Maintenance
    //
    // In harness mode, keep ceremony ingestion, sync, and discovery moving while
    // the UI is idle on a screen. Shared-flow receive steps otherwise depend too
    // heavily on incidental user actions to drive runtime convergence.
    // =========================================================================
    hooks.use_future({
        let app_core = app_ctx.app_core.raw().clone();
        let shutdown = bg_shutdown.read().clone();
        let harness_mode = harness_mode_enabled();
        async move {
            if !harness_mode {
                return;
            }
            loop {
                if shutdown.load(std::sync::atomic::Ordering::Acquire) {
                    break;
                }
                let runtime = {
                    let core = app_core.read().await;
                    core.runtime().cloned()
                };

                if let Some(runtime) = runtime {
                    let _ = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "terminal_harness_runtime_maintenance",
                        "trigger_discovery",
                        std::time::Duration::from_secs(3),
                        || runtime.trigger_discovery(),
                    )
                    .await;
                    let _ = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "terminal_harness_runtime_maintenance",
                        "process_ceremony_messages_before_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.process_ceremony_messages(),
                    )
                    .await;
                    let _ = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "terminal_harness_runtime_maintenance",
                        "trigger_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.trigger_sync(),
                    )
                    .await;
                    let _ = runtime_workflows::timeout_runtime_call(
                        &runtime,
                        "terminal_harness_runtime_maintenance",
                        "process_ceremony_messages_after_sync",
                        std::time::Duration::from_secs(3),
                        || runtime.process_ceremony_messages(),
                    )
                    .await;
                }

                let _ = system_workflows::refresh_account(&app_core).await;

                effect_sleep(tokio::time::Duration::from_secs(1)).await;
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
    hooks.use_future({
        let command_rx_holder = harness_command_rx_holder.clone();
        {
            let mut screen = screen.clone();
            let mut should_exit = should_exit.clone();
            let app_ctx_for_commands = app_ctx.clone();
            let shared_callbacks_for_commands = shared_callbacks;
            let mut tui = tui.clone();
            let shared_contacts_for_commands = shared_contacts.clone();
            let shared_invitations_for_commands = shared_invitations.clone();
            let shared_pending_requests_for_commands = shared_pending_requests.clone();
            let shared_channels_for_commands = shared_channels.clone();
            let shared_devices_for_commands = shared_devices.clone();
            let shared_messages_for_commands = shared_messages.clone();
            let last_exported_devices_for_commands = last_exported_devices.clone();
            let tui_selected_for_commands = tui_selected_for_chat_screen.clone();
            async move {
                let Some(command_rx_holder) = command_rx_holder else {
                    return;
                };
                let mut rx = {
                    let Ok(mut guard) = command_rx_holder.lock() else {
                        tracing::warn!(
                            "failed to lock harness command receiver holder during shell setup"
                        );
                        return;
                    };
                    let Some(rx) = guard.take() else {
                        tracing::warn!(
                            "harness command receiver already owned by an earlier shell generation"
                        );
                        return;
                    };
                    rx
                };

                while let Some(submission) = rx.recv().await {
                    let submission_id = submission.submission_id.clone();
                    let app_snapshot_for_command = match authoritative_app_snapshot_with_retry(
                        &app_ctx_for_commands,
                        "authoritative snapshot unavailable",
                    )
                    .await
                    {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            if let Err(reject_error) =
                                reject_harness_command_submission(submission_id, error).await
                            {
                                tracing::warn!(
                                    error = %reject_error,
                                    "failed to reject harness command after snapshot contention"
                                );
                            }
                            continue;
                        }
                    };
                    let harness_contacts_for_command = shared_contacts_for_commands.read().clone();
                    let harness_channels_for_command = shared_channels_for_commands.read().clone();
                    let harness_devices_for_command =
                        authoritative_settings_devices_for_command(
                            &app_ctx_for_commands,
                            &shared_devices_for_commands,
                        )
                        .await;
                    let (authorities_for_command, current_authority_index_for_command) =
                        authoritative_settings_authorities_for_command(&app_ctx_for_commands).await;
                    let harness_messages_for_command = shared_messages_for_commands.read().clone();

                    let apply_result = tui.with_mut(|state| {
                        let callbacks_for_commands = shared_callbacks_for_commands.read().clone();
                        let mut operation_handle = None;
                        if !authorities_for_command.is_empty() {
                            state.authorities = authorities_for_command.clone();
                            state.current_authority_index = current_authority_index_for_command
                                .min(state.authorities.len().saturating_sub(1));
                        }
                        let followup = apply_harness_command(
                            state,
                            submission.command.clone(),
                            TuiSemanticInputs {
                                app_snapshot: &app_snapshot_for_command,
                                contacts: &harness_contacts_for_command,
                                settings_devices: &harness_devices_for_command,
                                chat_channels: &harness_channels_for_command,
                                chat_messages: &harness_messages_for_command,
                            },
                        )?;
                        for command in followup {
                            if let Some(handle) = execute_harness_followup_command(
                                state,
                                command,
                                HarnessDispatchContext {
                                    callbacks: &callbacks_for_commands,
                                    app_ctx: &app_ctx_for_commands,
                                    update_tx: &update_tx_for_commands,
                                    shared_invitations: &shared_invitations_for_commands,
                                    shared_pending_requests: &shared_pending_requests_for_commands,
                                    shared_contacts: &shared_contacts_for_commands,
                                    shared_channels: &shared_channels_for_commands,
                                    shared_devices: &shared_devices_for_commands,
                                    shared_messages: &shared_messages_for_commands,
                                    last_exported_devices: &last_exported_devices_for_commands,
                                    selected_channel: &tui_selected_for_commands,
                                },
                            )? {
                                operation_handle = Some(handle);
                            }
                        }
                        Ok::<_, String>((state.screen(), operation_handle))
                    });
                    let (next_screen, operation_handle) = match apply_result {
                        Ok(result) => result,
                        Err(error) => {
                            if let Err(send_error) =
                                reject_harness_command_submission(submission_id, error).await
                            {
                                tracing::warn!(
                                    error = %send_error,
                                    "failed to reject harness command after local apply failure"
                                );
                            }
                            continue;
                        }
                    };
                    if next_screen != screen.get() {
                        screen.set(next_screen);
                    }

                    let updated_state = tui.read_clone();
                    let updated_channels_for_command = shared_channels_for_commands.read().clone();
                    let committed_selection = tui_selected_for_commands.read().clone();
                    let immediate_join_binding = authoritative_binding_for_requested_join(
                        &submission.command,
                        &updated_channels_for_command,
                        Some(updated_state.chat.selected_channel),
                        committed_selection
                            .as_ref()
                            .map(CommittedChannelSelection::binding),
                    );
                    let settlement = match operation_handle {
                        Some(operation)
                            if operation.operation_id()
                                == &aura_app::ui_contract::OperationId::create_channel()
                                || operation.operation_id()
                                    == &aura_app::ui_contract::OperationId::join_channel() =>
                        {
                            if operation.operation_id()
                                == &aura_app::ui_contract::OperationId::join_channel()
                            {
                                if let Some(binding) = immediate_join_binding {
                                    accept_harness_command_submission(
                                        submission_id,
                                        Some(operation),
                                        Some(binding),
                                    )
                                    .await
                                } else {
                                    track_pending_semantic_submission(
                                        submission_id,
                                        operation,
                                        PendingSemanticValueKind::ChannelBinding,
                                    )
                                    .await
                                }
                            } else {
                                track_pending_semantic_submission(
                                    submission_id,
                                    operation,
                                    PendingSemanticValueKind::ChannelBinding,
                                )
                                .await
                            }
                        }
                        Some(operation)
                            if operation.operation_id()
                                == &aura_app::ui_contract::OperationId::invitation_create()
                                && matches!(
                                    submission.command,
                                    HarnessUiCommand::CreateContactInvitation { .. }
                                ) =>
                        {
                            track_pending_semantic_submission(
                                submission_id,
                                operation,
                                PendingSemanticValueKind::ContactInvitationCode,
                            )
                            .await
                        }
                        Some(operation) => {
                            accept_harness_command_submission(submission_id, Some(operation), None)
                                .await
                        }
                        None => accept_harness_command_submission(submission_id, None, None).await,
                    };
                    if let Err(error) = settlement {
                        tracing::warn!(
                            error = %error,
                            "failed to settle harness command submission"
                        );
                    }

                    if updated_state.should_exit && !should_exit.get() {
                        should_exit.set(true);
                        bg_shutdown.read().store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }

                    let app_snapshot = match authoritative_app_snapshot_with_retry(
                        &app_ctx_for_commands,
                        "failed to export authoritative TUI projection after applying harness command",
                    )
                    .await
                    {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            tracing::warn!(error = %error);
                            continue;
                        }
                    };
                    let harness_contacts = shared_contacts_for_commands.read().clone();
                    let harness_devices = shared_devices_for_commands.read().clone();
                    let harness_channels = shared_channels_for_commands.read().clone();
                    let harness_messages = shared_messages_for_commands.read().clone();

                    let export_result = maybe_export_ui_snapshot(
                        &updated_state,
                        TuiSemanticInputs {
                            app_snapshot: &app_snapshot,
                            contacts: &harness_contacts,
                            settings_devices: &harness_devices,
                            chat_channels: &harness_channels,
                            chat_messages: &harness_messages,
                        },
                    );
                    if let Err(error) = export_result {
                        tracing::warn!(
                            error = %error,
                            "failed to export authoritative TUI projection after applying harness command"
                        );
                    }
                }
            }
        }
    });
    hooks.use_future({
        let rx_holder = update_rx_holder.clone();
        {
            let nickname_suggestion_state = nickname_suggestion_state.clone();
            let mut should_exit = should_exit.clone();
            let app_ctx_for_updates = app_ctx.clone();
            let bootstrap_handoff_tx = bootstrap_handoff_tx.clone();
            let bg_shutdown = bg_shutdown.clone();
            // Toast queue migration: mutate TuiState via TuiStateHandle (always bumps render version)
            let tui = tui.clone();
            let shared_contacts_for_updates = shared_contacts.clone();
            let shared_channels_for_updates = shared_channels.clone();
            let shared_devices_for_updates = shared_devices.clone();
            let shared_messages_for_updates = shared_messages.clone();
            // Shared selection state for messages subscription synchronization
            let tui_selected_for_updates = tui_selected;
            let ready_join_channel_instances_for_updates = ready_join_channel_instances;
            async move {
                let Some(rx_holder) = rx_holder else {
                    return;
                };
                // Take the receiver from the holder (only happens once)
                let mut rx = {
                    let Ok(mut guard) = rx_holder.lock() else {
                        tracing::warn!(
                            "failed to lock UI update receiver holder during shell setup"
                        );
                        return;
                    };
                    let Some(rx) = guard.take() else {
                        tracing::warn!(
                            "UI update receiver already owned by an earlier shell generation"
                        );
                        return;
                    };
                    rx
                };

                // Process updates as they arrive
                while let Some(update) = rx.recv().await {
                    let outcome = process_ui_update(
                        update,
                        &mut UiUpdateContext {
                            show_setup,
                            nickname_suggestion_state: nickname_suggestion_state.clone(),
                            should_exit: should_exit.clone(),
                            app_ctx: app_ctx_for_updates.clone(),
                            bootstrap_handoff_tx: bootstrap_handoff_tx.clone(),
                            bg_shutdown: bg_shutdown.clone(),
                            tui: tui.clone(),
                            tasks_for_updates: tasks_for_updates.clone(),
                            shared_contacts_for_updates: shared_contacts_for_updates.clone(),
                            shared_channels_for_updates: shared_channels_for_updates.clone(),
                            shared_devices_for_updates: shared_devices_for_updates.clone(),
                            shared_messages_for_updates: shared_messages_for_updates.clone(),
                            tui_selected_for_updates: tui_selected_for_updates.clone(),
                            ready_join_channel_instances_for_updates:
                                ready_join_channel_instances_for_updates.clone(),
                        },
                    )
                    .await;
                    if matches!(outcome, UiUpdateLoopAction::ContinueLoop) {
                        continue;
                    }

                    let updated_state = tui.read_clone();
                    if updated_state.should_exit && !should_exit.get() {
                        should_exit.set(true);
                        bg_shutdown
                            .read()
                            .store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                }
            }
        }
    });

    // Read TUI state for rendering via type-safe handle.
    // This MUST be used for all render-time state access - it reads the version to establish
    // reactivity, ensuring the component re-renders when state changes via tui.replace().
    // See TuiStateHandle and TuiStateSnapshot docs for the reactivity model.
    let tui_snapshot = tui.read_for_render();

    // Handle exit request after hooks below so hook order remains stable across renders.
    // The owned TuiState is the authoritative source here; relying only on the hook-local
    // flag can miss async update paths like bootstrap handoff.
    let render_should_exit = should_exit.get() || tui_snapshot.should_exit;

    // Note: Domain data (channels, messages, guardians, etc.) is no longer passed to screens.
    // Each screen subscribes to signals directly via AppCoreContext.
    // See scripts/check/arch.sh --reactive for architectural enforcement.

    let _projection_export_version = projection_export_version.get();
    let app_snapshot = match app_ctx.snapshot() {
        AppSnapshotAvailability::Available(snapshot) => Some(snapshot),
        AppSnapshotAvailability::Contended => {
            tracing::warn!(
                "failed to publish TUI harness snapshot during render: state lock contended"
            );
            None
        }
    };
    let render_short_circuit = render_should_exit || app_snapshot.is_none();
    let harness_devices = shared_devices.read().clone();
    let harness_contacts = shared_contacts.read().clone();
    let harness_channels = shared_channels.read().clone();
    let harness_messages = shared_messages.read().clone();
    if !harness_devices.is_empty() {
        *last_exported_devices.write() = harness_devices.clone();
    }
    if let Some(app_snapshot) = app_snapshot.as_ref() {
        if let Err(error) = maybe_export_ui_snapshot(
            &tui_snapshot,
            TuiSemanticInputs {
                app_snapshot,
                contacts: &harness_contacts,
                settings_devices: &harness_devices,
                chat_channels: &harness_channels,
                chat_messages: &harness_messages,
            },
        ) {
            tracing::warn!(
                error = %error,
                "failed to publish TUI harness snapshot during render"
            );
        }
    }
    // Callbacks registry and individual callback extraction for screen props
    let callbacks = props.callbacks.clone();

    // Extract individual callbacks from registry for screen component props
    // (Screen components still use individual callback props for now)
    let on_run_slash_command = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_run_slash_command.clone());
    let on_retry_message = callbacks
        .as_ref()
        .map(|cb| cb.chat.on_retry_message.clone());
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
    let toast_messages = queued_toast.map(|toast| vec![ToastMessage::from(&toast)]);

    // Global/screen hints come from one shared keybinding registry.
    let global_hints = global_footer_hints();
    let screen_hints = screen_footer_hints(current_screen);

    let state_indicator = state_indicator_label(&tui_snapshot);

    let tasks_for_events = tasks.clone();
    hooks.use_terminal_events({
        let mut screen = screen.clone();
        let mut should_exit = should_exit.clone();
        let mut tui = tui;
        let _tasks_for_dispatch = tasks;
        // Clone update channel sender for ceremony UI updates
        let update_tx_for_ceremony = props.update_tx.clone();
        let update_tx_for_dispatch = update_tx_for_events.clone();
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
        // Clone shared messages Arc for message retry dispatch
        // Used to look up failed messages by ID to get channel and content for retry
        let shared_messages_for_dispatch = shared_messages.clone();
        // Used to map device selection for MFA wizard
        let shared_devices_for_dispatch = shared_devices;
        // Clone shared selection state for immediate sync on channel navigation
        let tui_selected_for_events = tui_selected_for_chat_screen.clone();
        // Used for recovery eligibility checks (from threshold subscription)
        move |event| {
            if let Some(input_transition) = transition_from_terminal_event(
                event,
                &tui,
                &shared_channels_for_dispatch,
                &shared_neighborhood_homes_for_dispatch,
                &shared_neighborhood_home_meta_for_dispatch,
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
                    );
                    for cmd in commands {
                        match cmd {
                            TuiCommand::Exit => {
                                app_ctx_for_dispatch.io_context().request_user_quit();
                                should_exit.set(true);
                                bg_shutdown
                                    .read()
                                    .store(true, std::sync::atomic::Ordering::Release);
                            }
                            TuiCommand::HarnessRemoveVisibleDevice { device_id } => {
                                let current_devices = shared_devices_for_dispatch.read().clone();
                                let Some(device_id) = device_id
                                    .or_else(|| {
                                        current_devices
                                            .iter()
                                            .find(|device| !device.is_current)
                                            .map(|device| device.id.clone())
                                    })
                                    .or_else(|| {
                                        (current_devices.len() > 1)
                                            .then(|| {
                                                current_devices
                                                    .last()
                                                    .map(|device| device.id.clone())
                                            })
                                            .flatten()
                                    })
                                else {
                                    new_state.toast_error("No removable device is visible");
                                    continue;
                                };
                                let Some(update_tx) = update_tx_for_events.clone() else {
                                    new_state.toast_error("UI update sender is unavailable");
                                    continue;
                                };
                                let operation = submit_ceremony_operation(
                                    app_ctx_for_dispatch.app_core.raw().clone(),
                                    tasks_for_events.clone(),
                                    update_tx,
                                    OperationId::remove_device(),
                                    SemanticOperationKind::RemoveDevice,
                                );
                                (cb.settings.on_remove_device)(device_id.into(), operation);
                            }
                            TuiCommand::Dispatch(dispatch_cmd) => {
                                let outcome = handle_dispatch_command(
                                    dispatch_cmd,
                                    &mut new_state,
                                    &EventDispatchContext {
                                        app_ctx: &app_ctx_for_dispatch,
                                        callbacks: cb,
                                        tasks_for_events: &tasks_for_events,
                                        update_tx_for_events: &update_tx_for_events,
                                        update_tx_for_dispatch: &update_tx_for_dispatch,
                                        update_tx_for_ceremony: &update_tx_for_ceremony,
                                        shared_channels_for_dispatch: &shared_channels_for_dispatch,
                                        shared_neighborhood_homes_for_dispatch:
                                            &shared_neighborhood_homes_for_dispatch,
                                        shared_invitations_for_dispatch:
                                            &shared_invitations_for_dispatch,
                                        shared_pending_requests_for_dispatch:
                                            &shared_pending_requests_for_dispatch,
                                        shared_contacts_for_dispatch: &shared_contacts_for_dispatch,
                                        shared_discovered_peers_for_dispatch:
                                            &shared_discovered_peers_for_dispatch,
                                        shared_messages_for_dispatch: &shared_messages_for_dispatch,
                                        shared_devices_for_dispatch: &shared_devices_for_dispatch,
                                        shared_threshold_for_dispatch:
                                            &shared_threshold_for_dispatch,
                                        tui_selected_for_events: &tui_selected_for_events,
                                    },
                                );
                                if matches!(outcome, EventCommandLoopAction::ContinueCommand) {
                                    continue;
                                }
                            }
                            TuiCommand::ShowToast { message, level } => {
                                let toast_id = new_state.next_toast_id;
                                new_state.next_toast_id += 1;
                                let toast =
                                    crate::tui::state::QueuedToast::new(toast_id, message, level);
                                new_state.toast_queue.enqueue(toast);
                            }
                            TuiCommand::DismissToast { id: _ } => {
                                new_state.toast_queue.dismiss();
                            }
                            TuiCommand::ClearAllToasts => {
                                new_state.toast_queue.clear();
                            }
                            TuiCommand::Render => {}
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
                    bg_shutdown
                        .read()
                        .store(true, std::sync::atomic::Ordering::Release);
                }

                // Update TuiState (and always bump render version)
                tui.replace(new_state);
            }

            // All key events are handled by the state machine above.
            // Modal handling goes through transition() -> command execution.
        }
    });

    if render_should_exit {
        system.exit();
        return element! { View {} };
    }
    if render_short_circuit {
        return element! { View {} };
    }

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
                #(if current_screen == Screen::Chat {
                    Some(element! {
                        View(width: 100pct, height: 100pct) {
                            ChatScreen(
                                view: chat_props.clone(),
                                selected_channel: Some(tui_selected_for_chat_screen),
                                shared_channels: Some(shared_channels),
                                shared_messages: Some(shared_messages),
                                on_run_slash_command: on_run_slash_command.clone(),
                                on_retry_message: on_retry_message.clone(),
                                on_create_channel: on_create_channel.clone(),
                                on_set_topic: on_set_topic.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    })
                } else {
                    None
                })
                #(if current_screen == Screen::Contacts {
                    Some(element! {
                        View(width: 100pct, height: 100pct) {
                            ContactsScreen(
                                view: contacts_props.clone(),
                                now_ms: now_ms,
                                on_update_nickname: on_update_nickname.clone(),
                                on_start_chat: on_start_chat.clone(),
                                on_invite_lan_peer: on_invite_lan_peer.clone(),
                            )
                        }
                    })
                } else {
                    None
                })
                #(if current_screen == Screen::Neighborhood {
                    Some(element! {
                        View(width: 100pct, height: 100pct) {
                            NeighborhoodScreen(
                                view: neighborhood_props.clone(),
                                update_tx: update_tx_holder.clone(),
                            )
                        }
                    })
                } else {
                    None
                })
                #(if current_screen == Screen::Settings {
                    Some(element! {
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
                    })
                } else {
                    None
                })
                #(if current_screen == Screen::Notifications {
                    Some(element! {
                        View(width: 100pct, height: 100pct) {
                            NotificationsScreen(
                                view: notifications_props,
                            )
                        }
                    })
                } else {
                    None
                })
            }

            // Footer with key hints and status (3 rows)
            Footer(
                hints: screen_hints,
                global_hints,
                disabled: is_insert_mode,
                network_status,
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
            #(if let Some(toasts) = toast_messages {
                Some(element! {
                    ToastContainer(toasts: toasts)
                })
            } else {
                None
            })
        }
    }
}

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
    render_account_setup_modal, render_add_device_modal, render_home_create_modal,
    render_channel_info_modal, render_chat_create_modal, render_confirm_modal,
    render_contact_modal, render_contacts_code_modal, render_contacts_create_modal,
    render_contacts_import_modal, render_device_enrollment_modal, render_device_import_modal,
    render_display_name_modal, render_guardian_modal, render_guardian_setup_modal,
    render_help_modal, render_mfa_setup_modal, render_nickname_modal, render_remove_device_modal,
    render_topic_modal, GlobalModalProps,
};

use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::signal_defs::{NetworkStatus, ERROR_SIGNAL, SETTINGS_SIGNAL};
use aura_app::workflows::settings::refresh_settings_from_runtime;
use aura_app::workflows::{
    cancel_key_rotation_ceremony, monitor_key_rotation_ceremony, start_device_threshold_ceremony,
    start_guardian_ceremony,
};
use aura_app::AppError;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::FrostThreshold;

use crate::tui::callbacks::CallbackRegistry;
use crate::tui::components::{
    DiscoveredPeerInfo, Footer, NavBar, ToastContainer, ToastLevel, ToastMessage,
};
use crate::tui::context::IoContext;
use crate::tui::hooks::{AppCoreContext, CallbackContext};
use crate::tui::layout::dim;
use crate::tui::screens::app::subscriptions::{
    use_channels_subscription, use_contacts_subscription, use_devices_subscription,
    use_messages_subscription, use_nav_status_signals, use_neighborhood_homes_subscription,
    use_notifications_subscription, use_pending_requests_subscription, use_threshold_subscription,
};
use crate::tui::screens::router::Screen;
use crate::tui::screens::{
    ChatScreen, ContactsScreen, NeighborhoodScreenV2, NotificationsScreen, SettingsScreen,
};
use crate::tui::types::{
    HomeSummary, Channel, Contact, Device, Guardian, Invitation, KeyHint, Message, MfaPolicy,
    TraversalDepth,
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
    pub display_name: String,
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
    pub traversal_depth: TraversalDepth,
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
    // Reactive update channel - receiver wrapped in Arc<Mutex<Option>> for take-once semantics
    /// UI update receiver for reactive updates from callbacks
    pub update_rx: Option<Arc<Mutex<Option<UiUpdateReceiver>>>>,
    /// UI update sender for sending updates from event handlers
    pub update_tx: Option<UiUpdateSender>,
    /// Callback registry for all domain actions
    pub callbacks: Option<CallbackRegistry>,
}

/// Main application with screen navigation

/// Type-safe handle for TuiState that enforces proper reactivity patterns.
///
/// # Reactivity Model
///
/// iocraft's `Ref<T>` (from `use_ref`) does NOT trigger re-renders when modified.
/// iocraft's `State<T>` (from `use_state`) DOES trigger re-renders when `.set()` is called,
/// but ONLY if the component read the State during render via `.get()`.
///
/// This handle enforces the correct pattern:
/// - **During render**: Use `read_for_render()` which reads the version (establishing reactivity)
///   and returns a snapshot of the state. This is the ONLY way to read state during render.
/// - **During callbacks**: Use `replace()` or `with_mut()` which update the Ref and bump the
///   version, triggering a re-render.
///
/// # Compile-Time Safety
///
/// By making `read_for_render()` the only way to access state during render, we ensure
/// the version is always read. If you try to bypass this (e.g., holding onto the raw Ref),
/// you won't have a `TuiStateSnapshot` to pass to prop extraction functions.
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

    /// Read state for rendering. This MUST be used during the render phase.
    ///
    /// This method:
    /// 1. Reads `version.get()` to establish reactivity (so the component re-renders when
    ///    `replace()` or `with_mut()` are called)
    /// 2. Returns a `TuiRenderState` that provides access to the TuiState
    ///
    /// # Why This Exists
    ///
    /// iocraft only re-renders a component when a `State<T>` it read during render changes.
    /// The TuiState lives in a `Ref<T>` which doesn't trigger re-renders. We use a separate
    /// `State<usize>` version counter that gets bumped on every state change.
    ///
    /// By reading the version here, we subscribe to changes. By returning a TuiRenderState,
    /// we ensure all render-time state access goes through this method.
    ///
    /// # Type Safety
    ///
    /// The returned `TuiRenderState` can only be created via this method, ensuring the
    /// version is always read during render. This makes the "forgot to read version" bug
    /// impossible - you can't access TuiState for rendering without going through here.
    fn read_for_render(&self) -> TuiRenderState {
        // Read version to establish reactivity - this is the key to making re-renders work!
        let _version = self.version.get();
        TuiRenderState {
            state: self.state.read().clone(),
        }
    }

    /// Clone the current state (for use in event handlers where you need ownership).
    ///
    /// Note: This does NOT read the version, so it should only be used in callbacks,
    /// not during render. For render-time access, use `read_for_render()`.
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

/// A render-time state snapshot that can only be created via `TuiStateHandle::read_for_render()`.
///
/// This type enforces that the version State is read during render (establishing reactivity).
/// All render-time access to TuiState must go through this type.
///
/// The state is cloned once per render, which is acceptable since:
/// - Render happens at most once per frame (~60Hz)
/// - The state is read-only during render anyway
/// - This avoids unsafe code and keeps the API clean
///
/// Implements `Deref<Target = TuiState>` for convenient access to state fields.
struct TuiRenderState {
    state: TuiState,
}

impl std::ops::Deref for TuiRenderState {
    type Target = TuiState;
    fn deref(&self) -> &TuiState {
        &self.state
    }
}

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

    // Display name state - State<T> automatically triggers re-renders on .set()
    let display_name_state = hooks.use_state({
        let initial = props.display_name.clone();
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

    // =========================================================================
    // Messages subscription: SharedMessages for dispatch handlers to read
    // =========================================================================
    // Used to look up failed messages by ID for retry operations.
    // The Arc is kept up-to-date by a reactive subscription to CHAT_SIGNAL.
    let shared_messages = use_messages_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Devices subscription: SharedDevices for dispatch handlers to read
    // =========================================================================
    let shared_devices = use_devices_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Channels subscription: SharedChannels for dispatch handlers to read
    // =========================================================================
    let shared_channels = use_channels_subscription(&mut hooks, &app_ctx);

    // =========================================================================
    // Neighborhood homes subscription: SharedNeighborhoodHomes for dispatch handlers to read
    // =========================================================================
    let shared_neighborhood_homes = use_neighborhood_homes_subscription(&mut hooks, &app_ctx);

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
    // Threshold values from settings - currently unused since threshold changes
    // now go through OpenGuardianSetup. Kept for future direct display updates.
    let _shared_threshold = use_threshold_subscription(&mut hooks, &app_ctx);

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
                // Only tick if there's an active toast to avoid unnecessary re-renders
                let has_active_toast = tui.state.read().toast_queue.is_active();
                if has_active_toast {
                    tui.with_mut(|state| {
                        state.toast_queue.tick();
                    });
                }
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
            let mut display_name_state = display_name_state.clone();
            let app_core = app_ctx.app_core.clone();
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
                        UiUpdate::DeviceEnrollmentStarted {
                            ceremony_id,
                            device_name,
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
                                                device_name,
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
                                                    let _ = refresh_settings_from_runtime(&app_core).await;
                                                });
                                            }
                                        }
                                    } else if let crate::tui::state_machine::QueuedModal::GuardianSetup(ref mut s) = modal {
                                        if matches!(
                                            s.step,
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            // Ensure ceremony id is set for cancel UX.
                                            if s.ceremony.ceremony_id.is_none() {
                                                s.ceremony.set_ceremony_id(ceremony_id.clone());
                                            }

                                            s.ceremony.update_from_status(
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
                                                s.ceremony.clear();
                                                s.ceremony_responses.clear();

                                                toast = Some((msg, crate::tui::state_machine::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    match kind {
                                                        aura_app::runtime_bridge::CeremonyKind::GuardianRotation => format!(
                                                            "Guardian ceremony complete! {}-of-{} committed",
                                                            threshold, total_count
                                                        ),
                                                        aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment => {
                                                            "Device enrollment complete".to_string()
                                                        }
                                                        aura_app::runtime_bridge::CeremonyKind::DeviceRemoval => {
                                                            "Device removal complete".to_string()
                                                        }
                                                        aura_app::runtime_bridge::CeremonyKind::DeviceRotation => {
                                                            format!(
                                                                "Device threshold ceremony complete ({}-of-{})",
                                                                threshold, total_count
                                                            )
                                                        }
                                                    },
                                                    crate::tui::state_machine::ToastLevel::Success,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                                if matches!(
                                                    kind,
                                                    aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
                                                        | aura_app::runtime_bridge::CeremonyKind::DeviceRemoval
                                                        | aura_app::runtime_bridge::CeremonyKind::DeviceRotation
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
                                            s.step,
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            if s.ceremony.ceremony_id.is_none() {
                                                s.ceremony.set_ceremony_id(ceremony_id.clone());
                                            }

                                            s.ceremony.update_from_status(
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

                                            for (id, _name, response) in &mut s.ceremony_responses {
                                                if accepted_devices.iter().any(|d| d == id) {
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
                                                    .unwrap_or_else(|| "Multifactor ceremony failed".to_string());
                                                s.error = Some(msg.clone());

                                                s.step = crate::tui::state_machine::GuardianSetupStep::ChooseThreshold;
                                                s.ceremony.clear();
                                                s.ceremony_responses.clear();

                                                toast = Some((msg, crate::tui::state_machine::ToastLevel::Error));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Multifactor ceremony complete! {}-of-{} committed",
                                                        threshold, total_count
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
                                && matches!(kind, aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment)
                                && (is_complete || has_failed)
                            {
                                let app_core = app_core.raw().clone();
                                let tasks = tasks_for_updates.clone();
                                tasks.spawn(async move {
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
                        UiUpdate::InvitationExported { code } => {
                            tui.with_mut(|state| {
                                state
                                    .modal_queue
                                    .enqueue(crate::tui::state_machine::QueuedModal::ContactsCode(
                                        crate::tui::state_machine::InvitationCodeModalState::for_code(
                                            code,
                                        ),
                                    ));
                            });
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
                        UiUpdate::HomeEntered { home_id: _ } => {
                            // Navigation/state machine owns the current home selection.
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
                                            s.step,
                                            crate::tui::state_machine::GuardianSetupStep::CeremonyInProgress
                                        ) {
                                            if s.ceremony.ceremony_id.is_none() {
                                                s.ceremony.set_ceremony_id(ceremony_id.clone());
                                            }

                                            s.ceremony.update_from_status(
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
                                                s.ceremony.clear();
                                                s.ceremony_responses.clear();

                                                toast = Some((
                                                    msg,
                                                    crate::tui::state_machine::ToastLevel::Error,
                                                ));
                                                dismiss_ceremony_started_toast = true;
                                            } else if is_complete {
                                                dismiss_modal = true;
                                                toast = Some((
                                                    format!(
                                                        "Guardian ceremony complete! {}-of-{} committed",
                                                        threshold, total_count
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
                            // Update contact count for keyboard navigation (navigate_list)
                            tui.with_mut(|state| {
                                state.contacts.contact_count = count;
                            });
                        }
                        UiUpdate::NotificationsCountChanged(count) => {
                            tui.with_mut(|state| {
                                state.notifications.item_count = count;
                                if count == 0 {
                                    state.notifications.selected_index = 0;
                                } else if state.notifications.selected_index >= count {
                                    state.notifications.selected_index = count.saturating_sub(1);
                                }
                            });
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

                        // =========================================================================
                        // Home operations
                        // =========================================================================
                        UiUpdate::HomeMessageSent {
                            home_id: _,
                            content: _,
                        } => {
                            enqueue_toast!(
                                "Home message sent".to_string(),
                                crate::tui::state_machine::ToastLevel::Success
                            );
                        }
                        UiUpdate::HomeInviteSent { contact_id: _ } => {
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

    // Note: Domain data (channels, messages, guardians, etc.) is no longer passed to screens.
    // Each screen subscribes to signals directly via AppCoreContext.
    // See check-arch.sh --reactive for architectural enforcement.

    // Read TUI state for rendering via type-safe handle.
    // This MUST be used for all render-time state access - it reads the version to establish
    // reactivity, ensuring the component re-renders when state changes via tui.replace().
    // See TuiStateHandle and TuiStateSnapshot docs for the reactivity model.
    let tui_snapshot = tui.read_for_render();

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
    let mut global_modals = GlobalModalProps::default();
    global_modals.current_screen_name = current_screen.name().to_string();

    if let Some(modal) = tui_snapshot.modal_queue.current() {
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
                global_modals.guardian_modal_selected_ids = state.selected_ids.clone();
                global_modals.guardian_modal_multi_select = state.multi_select;
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
                global_modals.contact_modal_selected_ids = state.selected_ids.clone();
                global_modals.contact_modal_multi_select = state.multi_select;
            }
            QueuedModal::ChatMemberSelect(state) => {
                global_modals.contact_modal_visible = true;
                global_modals.contact_modal_title = state.picker.title.clone();
                global_modals.contact_modal_contacts = state
                    .picker
                    .contacts
                    .iter()
                    .map(|(id, name)| Contact::new(id.clone(), name.clone()))
                    .collect();
                global_modals.contact_modal_selected = state.picker.selected_index;
                global_modals.contact_modal_selected_ids = state.picker.selected_ids.clone();
                global_modals.contact_modal_multi_select = state.picker.multi_select;
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
    let queued_toast = tui_snapshot.toast_queue.current().cloned();

    // Global hints that appear on all screens (bottom row)
    let global_hints = vec![
        KeyHint::new("↑↓←→", "Nav"),
        KeyHint::new("Tab", "Next"),
        KeyHint::new("?", "Help"),
        KeyHint::new("q", "Quit"),
    ];

    // Build screen-specific hints based on current screen (top row)
    let screen_hints: Vec<KeyHint> = match current_screen {
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
            KeyHint::new("a", "Accept"),
            KeyHint::new("n", "Invite"),
        ],
        Screen::Neighborhood => vec![
            KeyHint::new("Enter", "Enter"),
            KeyHint::new("Esc", "Map"),
            KeyHint::new("a", "Accept"),
            KeyHint::new("i", "Insert"),
            KeyHint::new("n", "New"),
        ],
        Screen::Notifications => vec![KeyHint::new("j/k", "Move"), KeyHint::new("h/l", "Focus")],
        Screen::Settings => vec![
            KeyHint::new("Enter", "Select"),
            KeyHint::new("Space", "Toggle"),
        ],
    };

    let tasks_for_events = tasks.clone();
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
        let shared_neighborhood_homes_for_dispatch = shared_neighborhood_homes.clone();
        let shared_pending_requests_for_dispatch = shared_pending_requests.clone();
        // This Arc is updated by a reactive subscription, so reading from it
        // always gets current contacts (not stale props)
        let shared_contacts_for_dispatch = shared_contacts.clone();
        // Clone shared messages Arc for message retry dispatch
        // Used to look up failed messages by ID to get channel and content for retry
        let shared_messages_for_dispatch = shared_messages.clone();
        // Used to map device selection for MFA wizard
        let shared_devices_for_dispatch = shared_devices.clone();
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

                                    // === Home Messaging Commands ===
                                    DispatchCommand::SendHomeMessage { content } => {
                                        (cb.home.on_send)(content);
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
                                                let mut modal_state = crate::tui::state_machine::ChannelInfoModalState::for_channel(
                                                    &channel.id,
                                                    &channel.name,
                                                    channel.topic.as_deref(),
                                                );

                                                // Best-effort: populate participants from locally-known contacts.
                                                let mut participants = vec!["You".to_string()];
                                                if let Ok(contacts) = shared_contacts_for_dispatch.read() {
                                                    if channel.id.starts_with("dm:") {
                                                        let target = channel.id.trim_start_matches("dm:");
                                                        let name = contacts
                                                            .iter()
                                                            .find(|c| c.id == target)
                                                            .map(|c| {
                                                                if !c.nickname.is_empty() {
                                                                    c.nickname.clone()
                                                                } else if let Some(s) = &c.suggested_name {
                                                                    s.clone()
                                                                } else {
                                                                    c.id.chars().take(8).collect::<String>() + "..."
                                                                }
                                                            })
                                                            .unwrap_or_else(|| target.to_string());
                                                        participants.push(name);
                                                    } else {
                                                        for c in contacts.iter() {
                                                            let name = if !c.nickname.is_empty() {
                                                                c.nickname.clone()
                                                            } else if let Some(s) = &c.suggested_name {
                                                                s.clone()
                                                            } else {
                                                                c.id.chars().take(8).collect::<String>() + "..."
                                                            };
                                                            participants.push(name);
                                                        }
                                                    }
                                                }

                                                modal_state.participants = participants;
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
                                    DispatchCommand::OpenChatCreateWizard => {
                                        let current_contacts = shared_contacts_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        let candidates: Vec<crate::tui::state_machine::ChatMemberCandidate> =
                                            current_contacts
                                                .iter()
                                                .map(|c| crate::tui::state_machine::ChatMemberCandidate {
                                                    id: c.id.clone(),
                                                    name: c.nickname.clone(),
                                                })
                                                .collect();

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
                                        members,
                                        threshold_k,
                                    } => {
                                        (cb.chat.on_create_channel)(
                                            name,
                                            topic,
                                            members,
                                            threshold_k,
                                        );
                                    }
                                    DispatchCommand::SetChannelTopic { channel_id, topic } => {
                                        (cb.chat.on_set_topic)(channel_id, topic);
                                    }
                                    DispatchCommand::DeleteChannel { channel_id } => {
                                        (cb.chat.on_close_channel)(channel_id);
                                    }

                                    // === Contacts Screen Commands ===
                                    DispatchCommand::UpdateNickname {
                                        contact_id,
                                        nickname,
                                    } => {
                                        (cb.contacts.on_update_nickname)(contact_id, nickname);
                                    }
                                    DispatchCommand::OpenContactNicknameModal => {
                                        let idx = new_state.contacts.selected_index;
                                        if let Ok(guard) = shared_contacts_for_dispatch.read() {
                                            if let Some(contact) = guard.get(idx) {
                                                // nickname is already populated with suggested_name if empty (see Contact::from)
                                                let modal_state = crate::tui::state_machine::NicknameModalState::for_contact(
                                                    &contact.id,
                                                    &contact.nickname,
                                                ).with_suggestion(contact.suggested_name.clone());
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
                                            if let Some(contact) = guard.get(idx) {
                                                let receiver_name = if !contact.nickname.is_empty()
                                                {
                                                    contact.nickname.clone()
                                                } else if let Some(s) = &contact.suggested_name {
                                                    s.clone()
                                                } else {
                                                    let short = contact
                                                        .id
                                                        .chars()
                                                        .take(8)
                                                        .collect::<String>();
                                                    format!("{short}...")
                                                };

                                                let modal_state =
                                                    crate::tui::state_machine::CreateInvitationModalState::for_receiver(
                                                        contact.id.clone(),
                                                        receiver_name,
                                                    );
                                                new_state
                                                    .modal_queue
                                                    .enqueue(crate::tui::state_machine::QueuedModal::ContactsCreate(
                                                        modal_state,
                                                    ));
                                            } else {
                                                new_state.toast_error("No contact selected");
                                            }
                                        } else {
                                            new_state.toast_error("Failed to read contacts");
                                        }
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
                                        (cb.contacts.on_remove_contact)(contact_id);
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
                                        new_state.toast_error(
                                            "Invitation list is not available; use Contacts to import codes",
                                        );
                                    }
                                    DispatchCommand::DeclineInvitation => {
                                        new_state.toast_error(
                                            "Invitation list is not available; use Contacts to import codes",
                                        );
                                    }
                                    DispatchCommand::CreateInvitation {
                                        receiver_id,
                                        invitation_type,
                                        message,
                                        ttl_secs,
                                    } => {
                                        (cb.invitations.on_create)(
                                            receiver_id,
                                            invitation_type,
                                            message,
                                            ttl_secs,
                                        );
                                    }
                                    DispatchCommand::ImportInvitation { code } => {
                                        (cb.invitations.on_import)(code);
                                    }
                                    DispatchCommand::ExportInvitation => {
                                        new_state.toast_error(
                                            "Invitation list is not available; use Contacts to create codes",
                                        );
                                    }
                                    DispatchCommand::RevokeInvitation { invitation_id } => {
                                        (cb.invitations.on_revoke)(invitation_id);
                                    }

                                    // === Recovery Commands ===
                                    DispatchCommand::StartRecovery => {
                                        (cb.recovery.on_start_recovery)();
                                    }
                                    DispatchCommand::ApproveRecovery => {
                                        if let Ok(guard) = shared_pending_requests_for_dispatch.read() {
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

                                        // Populate candidates from current contacts
                                        // Note: nickname is already populated with suggested_name if empty (see Contact::from)
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

                                    DispatchCommand::OpenMfaSetup => {
                                        let current_devices = shared_devices_for_dispatch
                                            .read()
                                            .map(|guard| guard.clone())
                                            .unwrap_or_default();

                                        // Require at least 2 devices to set up multi-factor authority
                                        if current_devices.len() < 2 {
                                            new_state.toast_error(
                                                "You must add a device to set up a multi-factor authority. \
                                                 Go to Devices → Add device first.",
                                            );
                                            continue;
                                        }

                                        let candidates: Vec<crate::tui::state_machine::GuardianCandidate> = current_devices
                                            .iter()
                                            .map(|d| {
                                                let name = if d.name.is_empty() {
                                                    let short = d.id.chars().take(8).collect::<String>();
                                                    format!("Device {}", short)
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

                                        let selected: Vec<usize> = (0..candidates.len()).collect();
                                        let n = selected.len() as u8;
                                        let threshold_k = if n >= 2 { (n / 2) + 1 } else { 2 };

                                        let mut modal_state =
                                            crate::tui::state_machine::GuardianSetupModalState::default();
                                        modal_state.contacts = candidates;
                                        modal_state.selected_indices = selected;
                                        modal_state.threshold_k = threshold_k;

                                        if !new_state.settings.demo_mobile_device_id.is_empty() {
                                            new_state.toast_info(
                                                "Demo: press Ctrl+M to select the Mobile device for MFA.",
                                            );
                                        }

                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::MfaSetup(modal_state),
                                        );
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
                                        let k_raw = threshold_k as u16;

                                        // Create FrostThreshold with validation (FROST requires k >= 2)
                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for guardian ceremony: {}", e);
                                                if let Some(tx) = update_tx_for_ceremony.clone() {
                                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                                        "guardian-ceremony-failed",
                                                        format!("Invalid threshold: {}", e),
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

                                            match start_guardian_ceremony(app, threshold, n, ids.clone())
                                                .await
                                            {
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
                                                                "Guardian ceremony started! Waiting for {}-of-{} guardians to respond",
                                                                k, n
                                                            ),
                                                        )));

                                                        // Prime the modal with an initial status update so `ceremony_id` is
                                                        // available immediately for UI cancel.
                                                        let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                            ceremony_id: ceremony_id.clone(),
                                                            kind: aura_app::runtime_bridge::CeremonyKind::GuardianRotation,
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
                                                    let tasks_handle = tasks.clone();
                                                    tasks_handle.spawn(async move {
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            ceremony_id.clone(),
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                                        ceremony_id: status.ceremony_id.clone(),
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
                                            threshold_k
                                        );

                                        let ids = device_ids.clone();
                                        let n = device_ids.len() as u16;
                                        let k_raw = threshold_k as u16;

                                        let threshold = match FrostThreshold::new(k_raw) {
                                            Ok(t) => t,
                                            Err(e) => {
                                                tracing::error!("Invalid threshold for multifactor ceremony: {}", e);
                                                if let Some(tx) = update_tx_for_ceremony.clone() {
                                                    let _ = tx.try_send(UiUpdate::ToastAdded(ToastMessage::error(
                                                        "mfa-ceremony-failed",
                                                        format!("Invalid threshold: {}", e),
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
                                                ids.clone(),
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
                                                                "Multifactor ceremony started ({}-of-{})",
                                                                k, n
                                                            ),
                                                        )));
                                                    }

                                                    if let Some(tx) = update_tx.clone() {
                                                        let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                            ceremony_id: ceremony_id.clone(),
                                                            kind: aura_app::runtime_bridge::CeremonyKind::DeviceRotation,
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
                                                    let tasks_handle = tasks.clone();
                                                    tasks_handle.spawn(async move {
                                                        let _ = monitor_key_rotation_ceremony(
                                                            &app_core_monitor,
                                                            ceremony_id.clone(),
                                                            tokio::time::Duration::from_millis(500),
                                                            |status| {
                                                                if let Some(tx) = update_tx_monitor.clone() {
                                                                    let _ = tx.try_send(UiUpdate::KeyRotationCeremonyStatus {
                                                                        ceremony_id: status.ceremony_id.clone(),
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

                                            if let Err(e) =
                                                cancel_key_rotation_ceremony(app, &ceremony_id).await
                                            {
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

                                            if let Err(e) =
                                                cancel_key_rotation_ceremony(app, &ceremony_id).await
                                            {
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
                                    DispatchCommand::UpdateDisplayName { display_name } => {
                                        (cb.settings.on_update_display_name)(display_name);
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
                                    DispatchCommand::ImportDeviceEnrollmentOnMobile { code } => {
                                        (cb.settings.on_import_device_enrollment_on_mobile)(code);
                                    }
                                    DispatchCommand::OpenAuthorityPicker => {
                                        // Build list of authorities from state
                                        let authorities = new_state.settings.authorities.clone();
                                        if authorities.len() <= 1 {
                                            new_state.toast_info("Only one authority available");
                                        } else {
                                            // Convert authorities to contact-like format for picker
                                            let contacts: Vec<(String, String)> = authorities
                                                .iter()
                                                .map(|a| (a.id.clone(), format!("{} ({})", a.display_name, a.short_id)))
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
                                        // Find the authority index and update state
                                        if let Some(idx) = new_state.settings.authorities
                                            .iter()
                                            .position(|a| a.id == authority_id)
                                        {
                                            new_state.settings.current_authority_index = idx;
                                            if let Some(auth) = new_state.settings.authorities.get(idx) {
                                                new_state.toast_success(format!(
                                                    "Switched to authority: {}",
                                                    auth.display_name
                                                ));
                                            }
                                        } else {
                                            new_state.toast_error("Authority not found");
                                        }
                                        // UI-only for now; runtime authority changes are managed elsewhere.
                                    }
                                    // Note: Threshold/guardian changes now use OpenGuardianSetup
                                    // which is handled above with the guardian ceremony commands.

                                    // === Neighborhood Screen Commands ===
                                    DispatchCommand::EnterHome => {
                                        let idx = new_state.neighborhood.grid.current();
                                        if let Ok(guard) = shared_neighborhood_homes_for_dispatch.read() {
                                            if let Some(home_id) = guard.get(idx) {
                                                // Default to Street-level traversal depth
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
                                    DispatchCommand::BackToStreet => {
                                        (cb.neighborhood.on_back_to_street)();
                                    }
                                    DispatchCommand::OpenHomeCreate => {
                                        // Open home creation modal
                                        new_state.modal_queue.enqueue(
                                            crate::tui::state_machine::QueuedModal::NeighborhoodHomeCreate(
                                                crate::tui::state_machine::HomeCreateModalState::new(),
                                            ),
                                        );
                                    }
                                    DispatchCommand::CreateHome { name, description } => {
                                        // UI-only for now; home creation is not wired to runtime yet.
                                        new_state.toast_success(format!("Home '{}' created", name));
                                        new_state.modal_queue.dismiss();
                                        let _ = description; // Suppress unused warning until wired
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
                            NeighborhoodScreenV2(
                                view: neighborhood_props.clone(),
                            )
                        }
                    }],
                    Screen::Settings => vec![element! {
                        View(width: 100pct, height: 100pct) {
                            SettingsScreen(
                                view: settings_props.clone(),
                                on_update_mfa: on_update_mfa.clone(),
                                on_update_display_name: on_update_display_name.clone(),
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
            #(render_display_name_modal(&settings_props))
            #(render_add_device_modal(&settings_props))
            #(render_device_import_modal(&settings_props))
            #(render_device_enrollment_modal(&settings_props))
            #(render_remove_device_modal(&settings_props))
            #(render_mfa_setup_modal(&settings_props))

            // === NEIGHBORHOOD SCREEN MODALS ===
            // Rendered via modal_overlays module for maintainability
            #(render_home_create_modal(&neighborhood_props))

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
    // - NeighborhoodScreenV2 subscribes to NEIGHBORHOOD_SIGNAL + HOMES_SIGNAL + CHAT_SIGNAL + CONTACTS_SIGNAL
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
    let network_status = NetworkStatus::Disconnected;
    let transport_peers: usize = 0;
    let known_online: usize = 0;

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
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        display_name: display_name,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        traversal_depth: TraversalDepth::Street,
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
                        // Invitations data
                        invitations: invitations,
                        guardians: guardians,
                        // Settings screen data
                        devices: devices,
                        display_name: display_name,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: MfaPolicy::SensitiveOnly,
                        // Contacts screen data
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        // Neighborhood screen data
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        traversal_depth: TraversalDepth::Street,
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
                    )
                }
            }
        };

        app.fullscreen().await
    }
}

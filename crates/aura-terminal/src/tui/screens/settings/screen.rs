//! # Settings Screen
//!
//! Account settings with editable profile and configuration modals.
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to settings state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::ui::signals::SETTINGS_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;
use std::sync::Arc;
use std::time::Duration;

use aura_app::ui::signals::{
    DiscoveredPeerMethod, NetworkStatus, SyncStatus, DISCOVERED_PEERS_SIGNAL,
    NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL, SYNC_STATUS_SIGNAL,
    TRANSPORT_PEERS_SIGNAL,
};
use aura_app::ui::types::format_relative_time_from;

use crate::tui::callbacks::{
    AddDeviceCallback, RemoveDeviceCallback, UpdateNicknameSuggestionCallback,
    UpdateThresholdCallback,
};
use crate::tui::components::SimpleSelectableItem;
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::SettingsViewProps;
use crate::tui::theme::Theme;
use crate::tui::types::{
    AuthorityInfo, AuthoritySubSection, Device, MfaPolicy, RecoveryStatus, SettingsSection,
};

// =============================================================================
// Callback Types (specialized, kept local)
// =============================================================================

/// MFA callback takes MfaPolicy type - kept local due to specialized parameter
pub type MfaCallback = Arc<dyn Fn(MfaPolicy) + Send + Sync>;

// =============================================================================
// Settings Screen Props
// =============================================================================

/// Props for SettingsScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
///
/// ## Reactive Data Model
///
/// Domain data (nickname_suggestion, devices, guardians, etc.) is NOT passed as props.
/// Instead, the component subscribes to signals directly via AppCoreContext.
/// This ensures a single source of truth and prevents stale data bugs.
#[derive(Default, Props)]
pub struct SettingsScreenProps {
    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_settings_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: SettingsViewProps,

    // === Callbacks ===
    pub on_update_mfa: Option<MfaCallback>,
    pub on_update_nickname_suggestion: Option<UpdateNicknameSuggestionCallback>,
    pub on_update_threshold: Option<UpdateThresholdCallback>,
    pub on_add_device: Option<AddDeviceCallback>,
    pub on_remove_device: Option<RemoveDeviceCallback>,
}

// =============================================================================
// Settings Screen Component
// =============================================================================

/// The settings screen
///
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available, this component will subscribe to settings
/// state signals and automatically update when data changes.
#[component]
pub fn SettingsScreen(
    props: &SettingsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // Get AppCoreContext for reactive signal subscription (required for domain data)
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // Initialize reactive state with defaults - will be populated by signal subscriptions
    let reactive_nickname_suggestion = hooks.use_state(String::new);
    let reactive_devices = hooks.use_state(Vec::new);
    let reactive_threshold = hooks.use_state(|| (0u8, 0u8));
    let reactive_guardian_count = hooks.use_state(|| 0usize);
    let reactive_recovery_status = hooks.use_state(RecoveryStatus::default);
    let reactive_authorities = hooks.use_state(Vec::<AuthorityInfo>::new);
    let reactive_network_status = hooks.use_state(NetworkStatus::default);
    let reactive_sync_status = hooks.use_state(SyncStatus::default);
    let reactive_transport_peers = hooks.use_state(|| 0usize);
    let reactive_discovery_counts = hooks.use_state(|| (0usize, 0usize, 0usize, 0u64));
    let reactive_now_ms = hooks.use_state(|| None::<u64>);

    // Subscribe to settings signal for domain data
    hooks.use_future({
        let mut reactive_nickname_suggestion = reactive_nickname_suggestion.clone();
        let mut reactive_devices = reactive_devices.clone();
        let mut reactive_threshold = reactive_threshold.clone();
        let mut reactive_authorities = reactive_authorities.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                reactive_nickname_suggestion.set(settings_state.nickname_suggestion.clone());
                let devices: Vec<Device> = settings_state
                    .devices
                    .iter()
                    .map(|d| Device {
                        id: d.id.to_string(),
                        name: d.name.clone(),
                        is_current: d.is_current,
                        last_seen: d.last_seen,
                    })
                    .collect();
                reactive_devices.set(devices);
                reactive_threshold.set((settings_state.threshold_k, settings_state.threshold_n));

                // Populate authorities from signal
                let authorities = if settings_state.authority_id.is_empty() {
                    Vec::new()
                } else {
                    vec![AuthorityInfo::new(
                        settings_state.authority_id.clone(),
                        settings_state.authority_nickname,
                    )
                    .current()]
                };
                reactive_authorities.set(authorities);
            })
            .await;
        }
    });

    // Subscribe to recovery signal for guardian count and recovery status
    hooks.use_future({
        let mut reactive_guardian_count = reactive_guardian_count.clone();
        let mut reactive_recovery_status = reactive_recovery_status.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*RECOVERY_SIGNAL, move |recovery_state| {
                reactive_guardian_count.set(recovery_state.guardian_count());
                reactive_recovery_status.set(RecoveryStatus::from(&recovery_state));
            })
            .await;
        }
    });

    // Subscribe to network status signal for observability
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_network_status = reactive_network_status.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*NETWORK_STATUS_SIGNAL, move |status| {
                reactive_network_status.set(status);
            })
            .await;
        }
    });

    // Subscribe to sync status signal for observability
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_sync_status = reactive_sync_status.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SYNC_STATUS_SIGNAL, move |status| {
                reactive_sync_status.set(status);
            })
            .await;
        }
    });

    // Subscribe to transport peers signal for observability
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_transport_peers = reactive_transport_peers.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*TRANSPORT_PEERS_SIGNAL, move |count| {
                reactive_transport_peers.set(count);
            })
            .await;
        }
    });

    // Subscribe to discovered peers for observability
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_discovery_counts = reactive_discovery_counts.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*DISCOVERED_PEERS_SIGNAL, move |state| {
                let mut lan_count = 0usize;
                let mut rendezvous_count = 0usize;
                for peer in &state.peers {
                    match peer.method {
                        DiscoveredPeerMethod::Lan => lan_count += 1,
                        DiscoveredPeerMethod::Rendezvous => rendezvous_count += 1,
                    }
                }
                reactive_discovery_counts.set((
                    state.peers.len(),
                    lan_count,
                    rendezvous_count,
                    state.last_updated_ms,
                ));
            })
            .await;
        }
    });

    // Keep a best-effort physical clock for relative-time UI formatting.
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut reactive_now_ms = reactive_now_ms.clone();
        async move {
            loop {
                let runtime = app_core.raw().read().await.runtime().cloned();
                if let Some(runtime) = runtime {
                    if let Ok(ts) = runtime.current_time_ms().await {
                        reactive_now_ms.set(Some(ts));
                    }
                }
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }
        }
    });

    // Use reactive state for rendering
    let nickname_suggestion = reactive_nickname_suggestion.read().clone();
    let devices = reactive_devices.read().clone();
    let (threshold_k, threshold_n) = *reactive_threshold.read();
    let guardian_count = *reactive_guardian_count.read();
    let recovery_status = reactive_recovery_status.read().clone();
    let authorities = reactive_authorities.read().clone();
    let network_status = *reactive_network_status.read();
    let sync_status = reactive_sync_status.read().clone();
    let transport_peers = *reactive_transport_peers.read();
    let (discovered_total, discovered_lan, discovered_rendezvous, discovered_last_ms) =
        *reactive_discovery_counts.read();
    let now_ms = *reactive_now_ms.read();

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_section = props.view.section;
    let current_device_index = props.view.selected_index;
    let current_mfa = props.view.mfa_policy;

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Build detail content
    let detail_lines: Vec<(String, Color)> = match current_section {
        SettingsSection::Profile => {
            let name = if nickname_suggestion.is_empty() {
                "(not set)".into()
            } else {
                nickname_suggestion
            };
            vec![
                (format!("Nickname: {name}"), Theme::TEXT),
                (String::new(), Theme::TEXT),
                (
                    "Your nickname is shared with contacts".into(),
                    Theme::TEXT_MUTED,
                ),
                ("and shown in your contact card.".into(), Theme::TEXT_MUTED),
                (String::new(), Theme::TEXT),
                ("[Enter] Edit".into(), Theme::SECONDARY),
            ]
        }
        SettingsSection::Threshold => {
            if threshold_n > 0 {
                // Threshold scheme is configured
                vec![
                    (
                        format!("Current Threshold: {threshold_k} of {threshold_n} guardians"),
                        Theme::SECONDARY,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        format!("Available Guardians: {guardian_count}"),
                        Theme::TEXT,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        "Guardians help recover your account if you".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("lose access to your devices.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[Enter] Edit threshold".into(), Theme::SECONDARY),
                ]
            } else if guardian_count > 0 {
                // Guardians exist but threshold not configured yet
                vec![
                    (
                        format!("Guardians Available: {guardian_count}"),
                        Theme::SECONDARY,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        "Configure how many guardians are needed".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("to recover your account.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[Enter] Configure threshold".into(), Theme::SECONDARY),
                ]
            } else {
                // No guardians at all
                vec![
                    ("No guardians configured".into(), Theme::WARNING),
                    (String::new(), Theme::TEXT),
                    (
                        "Add guardians from your contacts to".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("enable account recovery.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[Enter] Set up guardians".into(), Theme::SECONDARY),
                ]
            }
        }
        SettingsSection::Devices => {
            if devices.is_empty() {
                vec![
                    ("No devices found".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    (
                        "Devices are derived from the commitment".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("tree state for your account.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[a] Add device".into(), Theme::SECONDARY),
                    ("[i] Import device code".into(), Theme::TEXT_MUTED),
                ]
            } else {
                let mut lines: Vec<(String, Color)> = devices
                    .iter()
                    .enumerate()
                    .map(|(idx, d)| {
                        let sel = idx == current_device_index;
                        let c = if sel { Theme::SECONDARY } else { Theme::TEXT };
                        let label = if d.is_current {
                            format!("  {} (Local)", d.name)
                        } else {
                            format!("  {}", d.name)
                        };
                        (label, c)
                    })
                    .collect();
                lines.push((String::new(), Theme::TEXT));
                lines.push(("[a] Add device".into(), Theme::SECONDARY));
                lines.push(("[i] Import device code".into(), Theme::TEXT_MUTED));
                lines.push(("[r] Remove device".into(), Theme::TEXT_MUTED));
                lines
            }
        }
        SettingsSection::Recovery => {
            vec![
                (
                    format!("Recovery Status: {}", recovery_status.state.label()),
                    Theme::SECONDARY,
                ),
                (String::new(), Theme::TEXT),
                (
                    "If you lose access to your devices, you can".into(),
                    Theme::TEXT_MUTED,
                ),
                (
                    "request recovery from your guardians.".into(),
                    Theme::TEXT_MUTED,
                ),
                (String::new(), Theme::TEXT),
                (
                    "Your guardians will receive a notification".into(),
                    Theme::TEXT_MUTED,
                ),
                (
                    "and can approve your recovery request.".into(),
                    Theme::TEXT_MUTED,
                ),
                (String::new(), Theme::TEXT),
                ("[Enter] Start recovery request".into(), Theme::SECONDARY),
            ]
        }
        SettingsSection::Authority => {
            // Get authority state - use reactive authorities from signal, UI state from props
            let authority_sub = props.view.authority_sub_section;
            let current_auth_idx = props.view.current_authority_index;

            // Get current authority info if available (from reactive state)
            let current_auth = authorities.get(current_auth_idx);
            let has_multiple = authorities.len() > 1;

            match authority_sub {
                AuthoritySubSection::Info => {
                    let mut lines = vec![];

                    // Show current authority info
                    if let Some(auth) = current_auth {
                        let local_suffix = if auth.is_current { " (Local)" } else { "" };
                        lines.push((
                            format!("Authority: {}{}", auth.id, local_suffix),
                            Theme::SECONDARY,
                        ));
                    } else {
                        lines.push(("No authority configured".into(), Theme::WARNING));
                    }

                    lines.push((String::new(), Theme::TEXT));
                    lines.push((
                        "Authorities are cryptographic actors that".into(),
                        Theme::TEXT_MUTED,
                    ));
                    lines.push((
                        "can participate in authenticated actions.".into(),
                        Theme::TEXT_MUTED,
                    ));

                    // Switch authority hint (if multiple)
                    if has_multiple {
                        lines.push((String::new(), Theme::TEXT));
                        lines.push((
                            format!("{} authorities available", authorities.len()),
                            Theme::TEXT_MUTED,
                        ));
                        lines.push(("[s] Switch authority".into(), Theme::SECONDARY));
                    }

                    lines.push((String::new(), Theme::TEXT));
                    lines.push(("[←/h] [→/l] Navigate sections".into(), Theme::TEXT_MUTED));

                    lines
                }
                AuthoritySubSection::Mfa => {
                    vec![
                        ("Multifactor Authentication".into(), Theme::SECONDARY),
                        (String::new(), Theme::TEXT),
                        (
                            "Create a threshold signer set across your devices".into(),
                            Theme::TEXT_MUTED,
                        ),
                        ("to approve sensitive operations.".into(), Theme::TEXT_MUTED),
                        (String::new(), Theme::TEXT),
                        (format!("Policy: {}", current_mfa.name()), Theme::TEXT),
                        (current_mfa.description().into(), Theme::TEXT_MUTED),
                        (String::new(), Theme::TEXT),
                        ("[Enter] Configure multifactor".into(), Theme::SECONDARY),
                        ("[←/h] [→/l] Navigate sections".into(), Theme::TEXT_MUTED),
                    ]
                }
            }
        }
        SettingsSection::Observability => {
            let (network_label, network_color, last_sync_ms) = match network_status {
                NetworkStatus::Disconnected => ("Disconnected".to_string(), Theme::WARNING, 0),
                NetworkStatus::NoPeers => ("Connected (no peers)".to_string(), Theme::TEXT, 0),
                NetworkStatus::Syncing => ("Connected (syncing)".to_string(), Theme::SECONDARY, 0),
                NetworkStatus::Synced { last_sync_ms } => (
                    "Connected (synced)".to_string(),
                    Theme::SECONDARY,
                    last_sync_ms,
                ),
            };

            let (sync_label, sync_color) = match sync_status {
                SyncStatus::Idle => ("Idle".to_string(), Theme::TEXT),
                SyncStatus::Syncing { progress } => {
                    (format!("Syncing ({progress}%)"), Theme::SECONDARY)
                }
                SyncStatus::Synced => ("Synced".to_string(), Theme::SECONDARY),
                SyncStatus::Failed { message } => (format!("Failed: {message}"), Theme::WARNING),
            };

            let last_sync_display = if last_sync_ms > 0 {
                if let Some(now) = now_ms {
                    format_relative_time_from(now, last_sync_ms)
                } else {
                    "Unknown".to_string()
                }
            } else {
                "Never".to_string()
            };

            let discovery_display = if discovered_last_ms > 0 {
                if let Some(now) = now_ms {
                    format_relative_time_from(now, discovered_last_ms)
                } else {
                    "Unknown".to_string()
                }
            } else {
                "Never".to_string()
            };

            vec![
                ("Network".into(), Theme::SECONDARY),
                (format!("Status: {network_label}"), network_color),
                (
                    format!("Transport peers: {transport_peers}"),
                    Theme::TEXT,
                ),
                (String::new(), Theme::TEXT),
                ("Sync".into(), Theme::SECONDARY),
                (format!("Status: {sync_label}"), sync_color),
                (format!("Last sync: {last_sync_display}"), Theme::TEXT),
                (String::new(), Theme::TEXT),
                ("Discovery".into(), Theme::SECONDARY),
                (
                    format!(
                        "Peers: {discovered_total} (LAN {discovered_lan}, Rendezvous {discovered_rendezvous})"
                    ),
                    Theme::TEXT,
                ),
                (format!("Last update: {discovery_display}"), Theme::TEXT),
            ]
        }
    };

    // Layout: Full 25 rows for content (no input bar on this screen)
    element! {
        View(flex_direction: FlexDirection::Column, width: dim::TOTAL_WIDTH, height: dim::MIDDLE_HEIGHT, overflow: Overflow::Hidden) {
            // Main row layout - full 25 rows
            View(flex_direction: FlexDirection::Row, height: dim::MIDDLE_HEIGHT, gap: dim::TWO_PANEL_GAP, overflow: Overflow::Hidden) {
                // Left panel: Section list (fixed width from layout constants)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: Theme::BORDER,
                    padding_left: 1,
                    padding_right: 1,
                    padding_bottom: 1,
                    width: dim::TWO_PANEL_LEFT_WIDTH,
                ) {
                    Text(content: "Settings", weight: Weight::Bold, color: Theme::PRIMARY)
                    View(flex_direction: FlexDirection::Column, margin_top: 1) {
                        SimpleSelectableItem(label: "Profile".to_string(), selected: current_section == SettingsSection::Profile)
                        SimpleSelectableItem(label: "Guardian Threshold".to_string(), selected: current_section == SettingsSection::Threshold)
                        SimpleSelectableItem(label: "Request Recovery".to_string(), selected: current_section == SettingsSection::Recovery)
                        SimpleSelectableItem(label: "Devices".to_string(), selected: current_section == SettingsSection::Devices)
                        SimpleSelectableItem(label: "Authority".to_string(), selected: current_section == SettingsSection::Authority)
                        SimpleSelectableItem(label: "Observability".to_string(), selected: current_section == SettingsSection::Observability)
                    }
                }

                // Right panel: Detail view (fixed width from layout constants)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: Theme::BORDER,
                    padding_left: 1,
                    padding_right: 1,
                    padding_bottom: 1,
                    width: dim::TWO_PANEL_RIGHT_WIDTH,
                    overflow: Overflow::Hidden,
                ) {
                    Text(content: current_section.title(), weight: Weight::Bold, color: Theme::PRIMARY)
                    View(flex_direction: FlexDirection::Column, margin_top: 1, overflow: Overflow::Scroll) {
                        #(detail_lines.iter().map(|(text, color)| {
                            let t = text.clone();
                            let c = *color;
                            element! {
                                Text(content: t, color: c)
                            }
                        }))
                    }
                }
            }
        }
    }
}

/// Run the settings screen (requires AppCoreContext for domain data)
pub async fn run_settings_screen() -> std::io::Result<()> {
    // Note: This standalone runner won't have domain data without AppCoreContext.
    // Domain data is obtained via signal subscriptions when context is available.
    element! {
        SettingsScreen(
            view: SettingsViewProps::default(),
        )
    }
    .fullscreen()
    .await
}

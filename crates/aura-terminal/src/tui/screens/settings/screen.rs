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
//! Uses `aura_app::signal_defs::SETTINGS_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;
use std::sync::Arc;

use aura_app::signal_defs::{RECOVERY_SIGNAL, SETTINGS_SIGNAL};

use crate::tui::callbacks::{
    AddDeviceCallback, RemoveDeviceCallback, UpdateDisplayNameCallback, UpdateThresholdCallback,
};
use crate::tui::components::SimpleSelectableItem;
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::SettingsViewProps;
use crate::tui::theme::Theme;
use crate::tui::types::{AuthoritySubSection, Device, MfaPolicy, RecoveryStatus, SettingsSection};

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
/// Domain data (display_name, devices, guardians, etc.) is NOT passed as props.
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
    pub on_update_display_name: Option<UpdateDisplayNameCallback>,
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
    let reactive_display_name = hooks.use_state(String::new);
    let reactive_devices = hooks.use_state(Vec::new);
    let reactive_threshold = hooks.use_state(|| (0u8, 0u8));
    let reactive_guardian_count = hooks.use_state(|| 0usize);
    let reactive_recovery_status = hooks.use_state(RecoveryStatus::default);

    // Subscribe to settings signal for domain data
    hooks.use_future({
        let mut reactive_display_name = reactive_display_name.clone();
        let mut reactive_devices = reactive_devices.clone();
        let mut reactive_threshold = reactive_threshold.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            subscribe_signal_with_retry(app_core, &*SETTINGS_SIGNAL, move |settings_state| {
                reactive_display_name.set(settings_state.display_name);
                let devices: Vec<Device> = settings_state
                    .devices
                    .iter()
                    .map(|d| Device {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        is_current: d.is_current,
                        last_seen: d.last_seen,
                    })
                    .collect();
                reactive_devices.set(devices);
                reactive_threshold.set((settings_state.threshold_k, settings_state.threshold_n));
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
                reactive_guardian_count.set(recovery_state.guardians.len());
                reactive_recovery_status.set(RecoveryStatus::from(&recovery_state));
            })
            .await;
        }
    });

    // Use reactive state for rendering
    let display_name = reactive_display_name.read().clone();
    let devices = reactive_devices.read().clone();
    let (threshold_k, threshold_n) = *reactive_threshold.read();
    let guardian_count = *reactive_guardian_count.read();
    let recovery_status = reactive_recovery_status.read().clone();

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
            let name = if display_name.is_empty() {
                "(not set)".into()
            } else {
                display_name
            };
            vec![
                (format!("Display Name: {name}"), Theme::TEXT),
                (String::new(), Theme::TEXT),
                (
                    "Your display name is shared with contacts".into(),
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
                    (format!("Available Guardians: {threshold_n}"), Theme::TEXT),
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
                        let ind = if d.is_current { "* " } else { "  " };
                        let c = if sel { Theme::SECONDARY } else { Theme::TEXT };
                        (format!("{}{}", ind, d.name), c)
                    })
                    .collect();
                lines.push((String::new(), Theme::TEXT));
                lines.push(("[a] Add device".into(), Theme::SECONDARY));
                lines.push(("[i] Import device code".into(), Theme::TEXT_MUTED));
                lines.push(("[d] Remove selected".into(), Theme::TEXT_MUTED));
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
            // Get authority state from props
            let authority_sub = props.view.authority_sub_section;
            let authorities = &props.view.authorities;
            let current_auth_idx = props.view.current_authority_index;

            // Get current authority info if available
            let current_auth = authorities.get(current_auth_idx);
            let has_multiple = authorities.len() > 1;

            match authority_sub {
                AuthoritySubSection::Info => {
                    let mut lines = vec![];

                    // Show current authority info
                    if let Some(auth) = current_auth {
                        lines.push((
                            format!("Authority: {}", auth.display_name),
                            Theme::SECONDARY,
                        ));
                        lines.push((format!("ID: {}", auth.short_id), Theme::TEXT_MUTED));
                    } else {
                        lines.push(("No authority configured".into(), Theme::WARNING));
                    }

                    lines.push((String::new(), Theme::TEXT));
                    lines.push((
                        "Your authority represents your identity".into(),
                        Theme::TEXT_MUTED,
                    ));
                    lines.push((
                        "and controls access to your data.".into(),
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

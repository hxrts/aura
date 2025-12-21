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

use aura_app::signal_defs::SETTINGS_SIGNAL;

use crate::tui::callbacks::{
    AddDeviceCallback, RemoveDeviceCallback, UpdateDisplayNameCallback, UpdateThresholdCallback,
};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::SettingsViewProps;
use crate::tui::theme::Theme;
use crate::tui::types::{Device, MfaPolicy, SettingsSection};

// =============================================================================
// Callback Types (specialized, kept local)
// =============================================================================

/// MFA callback takes MfaPolicy type - kept local due to specialized parameter
pub type MfaCallback = Arc<dyn Fn(MfaPolicy) + Send + Sync>;

// =============================================================================
// Menu Item Component
// =============================================================================

#[derive(Default, Props)]
struct MenuItemProps {
    label: String,
    selected: bool,
}

#[component]
fn MenuItem(props: &MenuItemProps) -> impl Into<AnyElement<'static>> {
    let bg = if props.selected {
        Theme::LIST_BG_SELECTED
    } else {
        Theme::LIST_BG_NORMAL
    };
    let fg = if props.selected {
        Theme::LIST_TEXT_SELECTED
    } else {
        Theme::LIST_TEXT_NORMAL
    };
    let indicator = if props.selected { "> " } else { "  " };
    let text = format!("{}{}", indicator, props.label);

    element! {
        View(
            background_color: bg,
            padding_left: 1,
            padding_right: 1,
        ) {
            Text(content: text, color: fg)
        }
    }
}

// =============================================================================
// Settings Screen Props
// =============================================================================

/// Props for SettingsScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
#[derive(Default, Props)]
pub struct SettingsScreenProps {
    // === Domain data ===
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub contact_count: usize,
    pub devices: Vec<Device>,
    pub mfa_policy: MfaPolicy,

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
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props
    let reactive_display_name = hooks.use_state({
        let initial = props.display_name.clone();
        move || initial
    });

    let reactive_devices = hooks.use_state({
        let initial = props.devices.clone();
        move || initial
    });

    let reactive_threshold = hooks.use_state({
        let initial = (props.threshold_k, props.threshold_n);
        move || initial
    });

    // Subscribe to settings signal updates if AppCoreContext is available
    if let Some(ref ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_display_name = reactive_display_name.clone();
            let mut reactive_devices = reactive_devices.clone();
            let mut reactive_threshold = reactive_threshold.clone();
            let app_core = ctx.app_core.clone();
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
                    reactive_threshold
                        .set((settings_state.threshold_k, settings_state.threshold_n));
                })
                .await;
            }
        });
    }

    // Use reactive state for rendering
    let display_name = reactive_display_name.read().clone();
    let devices = reactive_devices.read().clone();
    let (threshold_k, threshold_n) = *reactive_threshold.read();

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_section = props.view.section;
    let is_list_focused = true; // Default to list focused (no panel_focus in SettingsViewProps)
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
                display_name.clone()
            };
            vec![
                (format!("Display Name: {}", name), Theme::TEXT),
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
                vec![
                    (
                        format!(
                            "Current Threshold: {} of {} guardians",
                            threshold_k, threshold_n
                        ),
                        Theme::SECONDARY,
                    ),
                    (String::new(), Theme::TEXT),
                    (format!("Available Guardians: {}", threshold_n), Theme::TEXT),
                    (String::new(), Theme::TEXT),
                    (
                        "Guardians help recover your account if you".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("lose access to your devices.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[Enter] Edit threshold".into(), Theme::SECONDARY),
                ]
            } else {
                vec![
                    ("Guardian configuration unavailable".into(), Theme::WARNING),
                    (String::new(), Theme::TEXT),
                    (
                        "You need at least one guardian before".into(),
                        Theme::TEXT_MUTED,
                    ),
                    (
                        "you can configure the recovery threshold.".into(),
                        Theme::TEXT_MUTED,
                    ),
                ]
            }
        }
        SettingsSection::Devices => {
            if devices.is_empty() {
                vec![
                    ("No devices registered".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    (
                        "This device will be added when you create".into(),
                        Theme::TEXT_MUTED,
                    ),
                    ("your first Block.".into(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    ("[a] Add device".into(), Theme::SECONDARY),
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
                lines.push(("[d] Remove selected".into(), Theme::TEXT_MUTED));
                lines
            }
        }
        SettingsSection::Mfa => {
            vec![
                (
                    format!("Current Policy: {}", current_mfa.name()),
                    Theme::SECONDARY,
                ),
                (String::new(), Theme::TEXT),
                (current_mfa.description().into(), Theme::TEXT_MUTED),
                (String::new(), Theme::TEXT),
                (
                    "Multifactor authentication adds an extra".into(),
                    Theme::TEXT_MUTED,
                ),
                (
                    "layer of security to your account.".into(),
                    Theme::TEXT_MUTED,
                ),
                (String::new(), Theme::TEXT),
                ("[Space] Cycle policy".into(), Theme::TEXT),
            ]
        }
    };

    // Border colors
    let list_border = if is_list_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };
    let detail_border = if !is_list_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    // Layout: Full 25 rows for content (no input bar on this screen)
    element! {
        View(flex_direction: FlexDirection::Column, width: dim::TOTAL_WIDTH, height: dim::MIDDLE_HEIGHT, overflow: Overflow::Hidden) {
            // Main row layout - full 25 rows
            View(flex_direction: FlexDirection::Row, height: dim::MIDDLE_HEIGHT, gap: 1, overflow: Overflow::Hidden) {
                // Left panel: Section list (fixed width in characters)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: list_border,
                    padding: 1,
                    width: 28,
                ) {
                    Text(content: "Settings", weight: Weight::Bold, color: Theme::PRIMARY)
                    View(flex_direction: FlexDirection::Column, margin_top: 1) {
                        MenuItem(label: "Profile".to_string(), selected: current_section == SettingsSection::Profile)
                        MenuItem(label: "Guardian Threshold".to_string(), selected: current_section == SettingsSection::Threshold)
                        MenuItem(label: "Devices".to_string(), selected: current_section == SettingsSection::Devices)
                        MenuItem(label: "Multifactor Auth".to_string(), selected: current_section == SettingsSection::Mfa)
                    }
                }

                // Right panel: Detail view (fixed width: 80 - 28 - 1 gap = 51)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: detail_border,
                    padding: 1,
                    width: 51,
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

/// Run the settings screen with sample data
pub async fn run_settings_screen() -> std::io::Result<()> {
    let devices = vec![
        Device::new("d1", "MacBook Pro").current(),
        Device::new("d2", "iPhone"),
        Device::new("d3", "iPad"),
    ];

    element! {
        SettingsScreen(
            display_name: "Alice".to_string(),
            threshold_k: 2u8,
            threshold_n: 3u8,
            contact_count: 5usize,
            devices: devices,
            mfa_policy: MfaPolicy::SensitiveOnly,
        )
    }
    .fullscreen()
    .await
}

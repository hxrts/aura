//! # Settings Screen
//!
//! Account settings with editable profile and configuration modals.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;
use std::sync::Arc;

// NOTE: Modal components (ConfirmModal, TextInputModal, ThresholdModal) have been moved to app.rs
use crate::tui::layout::dim;
use crate::tui::props::SettingsViewProps;
use crate::tui::theme::Theme;
use crate::tui::types::{Device, MfaPolicy, SettingsSection};

// =============================================================================
// Callback Types
// =============================================================================

pub type MfaCallback = Arc<dyn Fn(MfaPolicy) + Send + Sync>;
pub type UpdateNicknameCallback = Arc<dyn Fn(String) + Send + Sync>;
pub type UpdateThresholdCallback = Arc<dyn Fn(u8, u8) + Send + Sync>;
pub type AddDeviceCallback = Arc<dyn Fn(String) + Send + Sync>;
pub type RemoveDeviceCallback = Arc<dyn Fn(String) + Send + Sync>;

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
    pub on_update_nickname: Option<UpdateNicknameCallback>,
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
#[component]
pub fn SettingsScreen(props: &SettingsScreenProps) -> impl Into<AnyElement<'static>> {
    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_section = props.view.section;
    let is_list_focused = true; // Default to list focused (no panel_focus in SettingsViewProps)
    let current_device_index = props.view.selected_index;
    let current_mfa = props.view.mfa_policy;
    let devices = props.devices.clone();
    let display_name = props.display_name.clone();
    let threshold_k = props.threshold_k;
    let threshold_n = props.threshold_n;

    // NOTE: Modals have been moved to app.rs root level. See modal_frame.rs for details.

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

                // Right panel: Detail view (remaining width ~51 chars)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: detail_border,
                    padding: 1,
                    flex_grow: 1.0,
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

            // NOTE: All modals have been moved to app.rs root level and wrapped with ModalFrame
            // for consistent positioning. See the "SETTINGS SCREEN MODALS" section in app.rs.
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

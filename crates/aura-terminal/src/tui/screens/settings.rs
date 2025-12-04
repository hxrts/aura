//! # Settings Screen
//!
//! Account settings

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::KeyHintsBar;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Device, KeyHint, MfaPolicy, SettingsSection};

/// Callback type for MFA policy changes (require_mfa: bool)
pub type MfaCallback = Arc<dyn Fn(bool) + Send + Sync>;

/// Props for SectionList
#[derive(Default, Props)]
pub struct SectionListProps {
    pub selected: SettingsSection,
    pub focused: bool,
}

/// Section navigation list
#[component]
pub fn SectionList(props: &SectionListProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let selected = props.selected;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: "Settings", weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(flex_direction: FlexDirection::Column, padding: Spacing::PANEL_PADDING) {
                #(SettingsSection::all().iter().map(|&section| {
                    let is_selected = section == selected;
                    let bg = if is_selected { Theme::BG_SELECTED } else { Theme::BG_DARK };
                    let color = if is_selected { Theme::PRIMARY } else { Theme::TEXT };
                    let title = section.title().to_string();
                    element! {
                        View(background_color: bg, padding_left: Spacing::XS) {
                            Text(content: title, color: color)
                        }
                    }
                }))
            }
        }
    }
}

/// Props for DetailPanel
#[derive(Default, Props)]
pub struct DetailPanelProps {
    pub section: SettingsSection,
    pub focused: bool,
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub contact_count: usize,
    pub devices: Vec<Device>,
    pub device_index: usize,
    pub mfa_policy: MfaPolicy,
}

/// Detail panel that shows content based on selected section
#[component]
pub fn DetailPanel(props: &DetailPanelProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let section = props.section;
    let title = section.title().to_string();

    // Build content lines based on section
    let lines: Vec<(String, Color)> = match section {
        SettingsSection::Profile => {
            let name_display = if props.display_name.is_empty() {
                "(not set)".to_string()
            } else {
                props.display_name.clone()
            };
            vec![
                (format!("Display Name: {}", name_display), Theme::TEXT),
                (String::new(), Theme::TEXT),
                (
                    "Your display name is shared with contacts".to_string(),
                    Theme::TEXT_MUTED,
                ),
                (
                    "and shown in your contact card.".to_string(),
                    Theme::TEXT_MUTED,
                ),
            ]
        }
        SettingsSection::Threshold => {
            if props.contact_count > 0 {
                let threshold_text = if props.threshold_n == 0 {
                    "Not configured".to_string()
                } else {
                    format!(
                        "{} of {} guardians required",
                        props.threshold_k, props.threshold_n
                    )
                };
                vec![
                    (
                        format!("Current Threshold: {}", threshold_text),
                        Theme::SECONDARY,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        format!("Available Contacts: {}", props.contact_count),
                        Theme::TEXT,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        "Guardians help recover your account if you".to_string(),
                        Theme::TEXT_MUTED,
                    ),
                    (
                        "lose access to your devices.".to_string(),
                        Theme::TEXT_MUTED,
                    ),
                ]
            } else {
                vec![
                    (
                        "Guardian configuration unavailable".to_string(),
                        Theme::WARNING,
                    ),
                    (String::new(), Theme::TEXT),
                    (
                        "You need at least one contact before".to_string(),
                        Theme::TEXT_MUTED,
                    ),
                    (
                        "you can set up guardians for recovery.".to_string(),
                        Theme::TEXT_MUTED,
                    ),
                ]
            }
        }
        SettingsSection::Devices => {
            if props.devices.is_empty() {
                vec![
                    ("No devices registered".to_string(), Theme::TEXT_MUTED),
                    (String::new(), Theme::TEXT),
                    (
                        "This device will be added when you create".to_string(),
                        Theme::TEXT_MUTED,
                    ),
                    ("your first Block.".to_string(), Theme::TEXT_MUTED),
                ]
            } else {
                props
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(idx, device)| {
                        let is_selected = idx == props.device_index;
                        let indicator = if device.is_current { "* " } else { "  " };
                        let color = if is_selected {
                            Theme::SECONDARY
                        } else {
                            Theme::TEXT
                        };
                        (format!("{}{}", indicator, device.name), color)
                    })
                    .collect()
            }
        }
        SettingsSection::Mfa => {
            vec![
                (
                    format!("Current Policy: {}", props.mfa_policy.name()),
                    Theme::SECONDARY,
                ),
                (String::new(), Theme::TEXT),
                (
                    props.mfa_policy.description().to_string(),
                    Theme::TEXT_MUTED,
                ),
                (String::new(), Theme::TEXT),
                (
                    "Multifactor authentication adds an extra".to_string(),
                    Theme::TEXT_MUTED,
                ),
                (
                    "layer of security to your account.".to_string(),
                    Theme::TEXT_MUTED,
                ),
                (String::new(), Theme::TEXT),
                ("[Space] Cycle policy".to_string(), Theme::TEXT),
            ]
        }
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            View(padding_left: Spacing::PANEL_PADDING) {
                Text(content: title, weight: Weight::Bold, color: Theme::PRIMARY)
            }
            View(flex_direction: FlexDirection::Column, padding: Spacing::PANEL_PADDING) {
                #(lines.iter().map(|(text, color)| {
                    let t = text.clone();
                    let c = *color;
                    element! {
                        View {
                            Text(content: t, color: c)
                        }
                    }
                }))
            }
        }
    }
}

/// Props for SettingsScreen
#[derive(Default, Props)]
pub struct SettingsScreenProps {
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub contact_count: usize,
    pub devices: Vec<Device>,
    pub mfa_policy: MfaPolicy,
    /// Callback when MFA policy changes
    pub on_update_mfa: Option<MfaCallback>,
}

/// The settings screen
#[component]
pub fn SettingsScreen(
    props: &SettingsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let section = hooks.use_state(|| SettingsSection::Profile);
    let detail_focused = hooks.use_state(|| false);
    let device_index = hooks.use_state(|| 0usize);
    let mfa_policy = hooks.use_state(|| props.mfa_policy);

    let hints = vec![
        KeyHint::new("↑↓", "Navigate"),
        KeyHint::new("Tab", "Switch panel"),
        KeyHint::new("Space", "Toggle/Cycle"),
        KeyHint::new("Esc", "Back"),
    ];

    let current_section = section.get();
    let is_detail_focused = detail_focused.get();
    let current_device_index = device_index.get();
    let current_mfa = mfa_policy.get();
    let devices = props.devices.clone();
    let display_name = props.display_name.clone();
    let threshold_k = props.threshold_k;
    let threshold_n = props.threshold_n;
    let contact_count = props.contact_count;

    // Clone callback for event handler
    let on_update_mfa = props.on_update_mfa.clone();

    hooks.use_terminal_events({
        let mut section = section.clone();
        let mut detail_focused = detail_focused.clone();
        let mut device_index = device_index.clone();
        let mut mfa_policy = mfa_policy.clone();
        let device_count = devices.len();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, .. }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if !detail_focused.get() {
                        section.set(section.get().prev());
                    } else if section.get() == SettingsSection::Devices && device_count > 0 {
                        let idx = device_index.get();
                        if idx > 0 {
                            device_index.set(idx - 1);
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !detail_focused.get() {
                        section.set(section.get().next());
                    } else if section.get() == SettingsSection::Devices && device_count > 0 {
                        let idx = device_index.get();
                        if idx + 1 < device_count {
                            device_index.set(idx + 1);
                        }
                    }
                }
                KeyCode::Tab => {
                    detail_focused.set(!detail_focused.get());
                }
                KeyCode::Char(' ') => {
                    if detail_focused.get() && section.get() == SettingsSection::Mfa {
                        let new_policy = mfa_policy.get().next();
                        mfa_policy.set(new_policy);
                        // Dispatch callback with new require_mfa value
                        if let Some(ref callback) = on_update_mfa {
                            callback(new_policy.requires_mfa());
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    });

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Main content: sidebar + detail
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                gap: Spacing::XS,
            ) {
                // Sidebar (30%)
                View(width: 30pct) {
                    SectionList(selected: current_section, focused: !is_detail_focused)
                }
                // Detail panel (70%)
                DetailPanel(
                    section: current_section,
                    focused: is_detail_focused,
                    display_name: display_name,
                    threshold_k: threshold_k,
                    threshold_n: threshold_n,
                    contact_count: contact_count,
                    devices: devices,
                    device_index: current_device_index,
                    mfa_policy: current_mfa,
                )
            }

            // Key hints
            KeyHintsBar(hints: hints)
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

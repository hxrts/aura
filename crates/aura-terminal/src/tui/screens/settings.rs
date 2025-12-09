//! # Settings Screen
//!
//! Account settings with editable profile and configuration modals.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{
    ConfirmModal, TextInputModal, TextInputState, ThresholdModal, ThresholdState,
};
use crate::tui::navigation::{is_nav_key_press, InputThrottle, NavKey, NavThrottle, TwoPanelFocus};
use crate::tui::theme::Theme;
use crate::tui::types::{Device, MfaPolicy, SettingsSection};

// =============================================================================
// State Types
// =============================================================================

/// State for device removal confirmation modal
#[derive(Debug, Clone)]
pub struct ConfirmRemoveState {
    pub visible: bool,
    pub device_id: String,
    pub device_name: String,
    pub confirm_focused: bool,
}

impl ConfirmRemoveState {
    pub fn new() -> Self {
        Self {
            visible: false,
            device_id: String::new(),
            device_name: String::new(),
            confirm_focused: false,
        }
    }

    pub fn show(&mut self, device_id: &str, device_name: &str) {
        self.visible = true;
        self.device_id = device_id.to_string();
        self.device_name = device_name.to_string();
        self.confirm_focused = false;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.device_id.clear();
        self.device_name.clear();
    }

    pub fn toggle_focus(&mut self) {
        self.confirm_focused = !self.confirm_focused;
    }
}

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

#[derive(Default, Props)]
pub struct SettingsScreenProps {
    pub display_name: String,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub contact_count: usize,
    pub devices: Vec<Device>,
    pub mfa_policy: MfaPolicy,
    pub on_update_mfa: Option<MfaCallback>,
    pub on_update_nickname: Option<UpdateNicknameCallback>,
    pub on_update_threshold: Option<UpdateThresholdCallback>,
    pub on_add_device: Option<AddDeviceCallback>,
    pub on_remove_device: Option<RemoveDeviceCallback>,
}

// =============================================================================
// Settings Screen Component
// =============================================================================

#[component]
pub fn SettingsScreen(
    props: &SettingsScreenProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    // State
    let mut section = hooks.use_state(|| SettingsSection::Profile);
    let mut panel_focus = hooks.use_state(|| TwoPanelFocus::List);
    let mut device_index = hooks.use_state(|| 0usize);
    let mut mfa_policy = hooks.use_state(|| props.mfa_policy);

    // Modal states
    let initial_display_name = props.display_name.clone();
    let mut edit_name_state = hooks.use_ref(TextInputState::new);
    let mut edit_name_version = hooks.use_state(|| 0usize);

    let initial_threshold_k = props.threshold_k;
    let initial_threshold_n = props.threshold_n;
    let mut threshold_state = hooks.use_ref(ThresholdState::new);
    let mut threshold_version = hooks.use_state(|| 0usize);

    let mut device_add_state = hooks.use_ref(TextInputState::new);
    let mut device_add_version = hooks.use_state(|| 0usize);

    let mut confirm_remove_state = hooks.use_ref(ConfirmRemoveState::new);
    let mut confirm_remove_version = hooks.use_state(|| 0usize);

    let mut input_throttle = hooks.use_ref(InputThrottle::new);
    let mut nav_throttle = hooks.use_ref(NavThrottle::new);

    // Current values
    let current_section = section.get();
    let current_focus = panel_focus.get();
    let is_list_focused = current_focus == TwoPanelFocus::List;
    let current_device_index = device_index.get();
    let current_mfa = mfa_policy.get();
    let devices = props.devices.clone();
    let display_name = props.display_name.clone();
    let threshold_k = props.threshold_k;
    let threshold_n = props.threshold_n;

    // Callbacks
    let on_update_mfa = props.on_update_mfa.clone();
    let on_update_nickname = props.on_update_nickname.clone();
    let on_update_threshold = props.on_update_threshold.clone();
    let on_add_device = props.on_add_device.clone();
    let on_remove_device = props.on_remove_device.clone();

    // Modal render state
    let modal_visible = edit_name_state.read().visible;
    let modal_value = edit_name_state.read().value.clone();
    let modal_error = edit_name_state.read().error.clone().unwrap_or_default();
    let modal_submitting = edit_name_state.read().submitting;

    let threshold_modal_visible = threshold_state.read().visible;
    let threshold_modal_k = threshold_state.read().threshold_k;
    let threshold_modal_n = threshold_state.read().threshold_n;
    let threshold_modal_has_changed = threshold_state.read().has_changed();
    let threshold_modal_error = threshold_state.read().error.clone().unwrap_or_default();
    let threshold_modal_submitting = threshold_state.read().submitting;

    let device_modal_visible = device_add_state.read().visible;
    let device_modal_value = device_add_state.read().value.clone();
    let device_modal_error = device_add_state.read().error.clone().unwrap_or_default();
    let device_modal_submitting = device_add_state.read().submitting;

    let confirm_remove_visible = confirm_remove_state.read().visible;
    let confirm_remove_device_name = confirm_remove_state.read().device_name.clone();
    let confirm_remove_focused = confirm_remove_state.read().confirm_focused;

    // Event handling
    hooks.use_terminal_events({
        let device_count = devices.len();
        let devices_for_closure = devices.clone();
        move |event| {
            let name_modal_open = edit_name_state.read().visible;
            let threshold_modal_open = threshold_state.read().visible;
            let device_modal_open = device_add_state.read().visible;
            let confirm_modal_open = confirm_remove_state.read().visible;

            // Handle name modal
            if name_modal_open {
                match event {
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Esc, ..
                    }) => {
                        edit_name_state.write().hide();
                        edit_name_version.set(edit_name_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    }) => {
                        if edit_name_state.read().can_submit() {
                            let new_name = edit_name_state.read().value.clone();
                            edit_name_state.write().start_submitting();
                            if let Some(ref cb) = on_update_nickname {
                                cb(new_name);
                            }
                            edit_name_state.write().hide();
                            edit_name_version.set(edit_name_version.get() + 1);
                        }
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Backspace,
                        ..
                    }) => {
                        edit_name_state.write().pop_char();
                        edit_name_version.set(edit_name_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Char(c),
                        ..
                    }) => {
                        if input_throttle.write().try_input() {
                            edit_name_state.write().push_char(c);
                            edit_name_version.set(edit_name_version.get() + 1);
                        }
                    }
                    _ => {}
                }
                return;
            }

            // Handle threshold modal
            if threshold_modal_open {
                match event {
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Esc, ..
                    }) => {
                        threshold_state.write().hide();
                        threshold_version.set(threshold_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    }) => {
                        if threshold_state.read().can_submit() {
                            let new_k = threshold_state.read().threshold_k;
                            let n = threshold_state.read().threshold_n;
                            threshold_state.write().start_submitting();
                            if let Some(ref cb) = on_update_threshold {
                                cb(new_k, n);
                            }
                            threshold_state.write().hide();
                            threshold_version.set(threshold_version.get() + 1);
                        }
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Left,
                        ..
                    }) => {
                        threshold_state.write().decrement();
                        threshold_version.set(threshold_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Right,
                        ..
                    }) => {
                        threshold_state.write().increment();
                        threshold_version.set(threshold_version.get() + 1);
                    }
                    _ => {}
                }
                return;
            }

            // Handle device add modal
            if device_modal_open {
                match event {
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Esc, ..
                    }) => {
                        device_add_state.write().hide();
                        device_add_version.set(device_add_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    }) => {
                        if device_add_state.read().can_submit() {
                            let name = device_add_state.read().value.clone();
                            device_add_state.write().start_submitting();
                            if let Some(ref cb) = on_add_device {
                                cb(name);
                            }
                            device_add_state.write().hide();
                            device_add_version.set(device_add_version.get() + 1);
                        }
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Backspace,
                        ..
                    }) => {
                        device_add_state.write().pop_char();
                        device_add_version.set(device_add_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Char(c),
                        ..
                    }) => {
                        if input_throttle.write().try_input() {
                            device_add_state.write().push_char(c);
                            device_add_version.set(device_add_version.get() + 1);
                        }
                    }
                    _ => {}
                }
                return;
            }

            // Handle confirm modal
            if confirm_modal_open {
                match event {
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Esc, ..
                    }) => {
                        confirm_remove_state.write().hide();
                        confirm_remove_version.set(confirm_remove_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Enter,
                        ..
                    }) => {
                        if confirm_remove_state.read().confirm_focused {
                            let id = confirm_remove_state.read().device_id.clone();
                            if let Some(ref cb) = on_remove_device {
                                cb(id);
                            }
                        }
                        confirm_remove_state.write().hide();
                        confirm_remove_version.set(confirm_remove_version.get() + 1);
                    }
                    TerminalEvent::Key(KeyEvent {
                        code: KeyCode::Tab | KeyCode::Left | KeyCode::Right,
                        ..
                    }) => {
                        confirm_remove_state.write().toggle_focus();
                        confirm_remove_version.set(confirm_remove_version.get() + 1);
                    }
                    _ => {}
                }
                return;
            }

            // Handle navigation
            if let Some(nav_key) = is_nav_key_press(&event) {
                if nav_throttle.write().try_navigate() {
                    let focus = panel_focus.get();
                    match nav_key {
                        NavKey::Left | NavKey::Right => {
                            panel_focus.set(focus.navigate(nav_key));
                        }
                        NavKey::Up => {
                            if focus == TwoPanelFocus::List {
                                section.set(section.get().prev());
                            } else if section.get() == SettingsSection::Devices && device_count > 0
                            {
                                let idx = device_index.get();
                                device_index.set(if idx == 0 { device_count - 1 } else { idx - 1 });
                            }
                        }
                        NavKey::Down => {
                            if focus == TwoPanelFocus::List {
                                section.set(section.get().next());
                            } else if section.get() == SettingsSection::Devices && device_count > 0
                            {
                                let idx = device_index.get();
                                device_index.set(if idx >= device_count - 1 { 0 } else { idx + 1 });
                            }
                        }
                    }
                }
                return;
            }

            // Handle action keys
            match event {
                TerminalEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    if panel_focus.get() == TwoPanelFocus::Detail {
                        match section.get() {
                            SettingsSection::Profile => {
                                edit_name_state.write().show(
                                    "Edit Display Name",
                                    &initial_display_name,
                                    "Enter your display name...",
                                    None,
                                );
                                edit_name_version.set(edit_name_version.get() + 1);
                            }
                            SettingsSection::Threshold if initial_threshold_n > 0 => {
                                threshold_state
                                    .write()
                                    .show(initial_threshold_k, initial_threshold_n);
                                threshold_version.set(threshold_version.get() + 1);
                            }
                            _ => {}
                        }
                    }
                }
                TerminalEvent::Key(KeyEvent {
                    code: KeyCode::Char(' '),
                    ..
                }) => {
                    if panel_focus.get() == TwoPanelFocus::Detail
                        && section.get() == SettingsSection::Mfa
                    {
                        let new_policy = mfa_policy.get().next();
                        mfa_policy.set(new_policy);
                        if let Some(ref cb) = on_update_mfa {
                            cb(new_policy);
                        }
                    }
                }
                TerminalEvent::Key(KeyEvent {
                    code: KeyCode::Char('a'),
                    ..
                }) => {
                    if panel_focus.get() == TwoPanelFocus::Detail
                        && section.get() == SettingsSection::Devices
                    {
                        device_add_state.write().show(
                            "Add Device",
                            "",
                            "Enter device name...",
                            None,
                        );
                        device_add_version.set(device_add_version.get() + 1);
                    }
                }
                TerminalEvent::Key(KeyEvent {
                    code: KeyCode::Char('d'),
                    ..
                }) => {
                    if panel_focus.get() == TwoPanelFocus::Detail
                        && section.get() == SettingsSection::Devices
                        && device_count > 0
                    {
                        let idx = device_index.get();
                        if let Some(device) = devices_for_closure.get(idx) {
                            if !device.is_current {
                                confirm_remove_state.write().show(&device.id, &device.name);
                                confirm_remove_version.set(confirm_remove_version.get() + 1);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    });

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

    element! {
        View(flex_direction: FlexDirection::Column, width: 100pct, height: 100pct) {
            // Main row layout
            View(flex_direction: FlexDirection::Row, flex_grow: 1.0, gap: 1) {
                // Left panel: Section list (fixed width in characters)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: list_border,
                    padding: 1,
                    width: 28,
                    flex_shrink: 0.0,
                ) {
                    Text(content: "Settings", weight: Weight::Bold, color: Theme::PRIMARY)
                    View(flex_direction: FlexDirection::Column, margin_top: 1) {
                        MenuItem(label: "Profile".to_string(), selected: current_section == SettingsSection::Profile)
                        MenuItem(label: "Guardian Threshold".to_string(), selected: current_section == SettingsSection::Threshold)
                        MenuItem(label: "Devices".to_string(), selected: current_section == SettingsSection::Devices)
                        MenuItem(label: "Multifactor Auth".to_string(), selected: current_section == SettingsSection::Mfa)
                    }
                }

                // Right panel: Detail view (flex grow)
                View(
                    flex_direction: FlexDirection::Column,
                    border_style: BorderStyle::Round,
                    border_color: detail_border,
                    padding: 1,
                    flex_grow: 1.0,
                ) {
                    Text(content: current_section.title(), weight: Weight::Bold, color: Theme::PRIMARY)
                    View(flex_direction: FlexDirection::Column, margin_top: 1) {
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

            // Modals
            TextInputModal(
                visible: modal_visible,
                focused: modal_visible,
                title: "Edit Display Name".to_string(),
                value: modal_value,
                placeholder: "Enter your display name...".to_string(),
                error: modal_error,
                submitting: modal_submitting,
            )
            ThresholdModal(
                visible: threshold_modal_visible,
                focused: threshold_modal_visible,
                threshold_k: threshold_modal_k,
                threshold_n: threshold_modal_n,
                has_changed: threshold_modal_has_changed,
                error: threshold_modal_error,
                submitting: threshold_modal_submitting,
            )
            TextInputModal(
                visible: device_modal_visible,
                focused: device_modal_visible,
                title: "Add Device".to_string(),
                value: device_modal_value,
                placeholder: "Enter device name...".to_string(),
                error: device_modal_error,
                submitting: device_modal_submitting,
            )
            ConfirmModal(
                visible: confirm_remove_visible,
                title: "Remove Device".to_string(),
                message: format!("Are you sure you want to remove \"{}\"?", confirm_remove_device_name),
                confirm_text: "Remove".to_string(),
                cancel_text: "Cancel".to_string(),
                confirm_focused: confirm_remove_focused,
            )
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

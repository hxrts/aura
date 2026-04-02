//! Keyboard input handling and navigation logic.
//!
//! Processes keyboard events to navigate screens, update selections, handle text
//! input, and dispatch commands across the UI model state machine.

mod chat;
mod contacts;
mod modal;
mod navigation;
mod neighborhood;
mod notifications;
mod settings;
mod wizard;

use crate::model::{
    ActiveModal, ModalState, ScreenId, SettingsSection, TextModalState, ToastState, UiModel,
};
use aura_app::frontend_primitives::ClipboardPort;
use chat::handle_chat_char;
use contacts::handle_contacts_char;
use modal::{backspace, handle_escape, handle_modal_char, handle_modal_enter, handle_modal_tab};
use navigation::{cycle_screen, cycle_screen_prev, handle_horizontal, move_selection};
use neighborhood::handle_neighborhood_char;
use settings::handle_settings_char;
use wizard::{
    can_open_guardian_setup_wizard, handle_wizard_named_key, open_add_device_wizard,
    open_guardian_setup_wizard,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedKeyAction {
    Enter,
    Escape,
    ModalTab { reverse: bool },
    MoveSelection(i32),
    Horizontal(i32),
    Backspace,
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharacterAction {
    Help,
    Screen(ScreenId),
    MoveSelection(i32),
    Horizontal(i32),
    DisabledQuit,
    ModalCopyInviteCode,
    ScreenLocal,
}

fn classify_named_key(key_name: &str) -> NamedKeyAction {
    match key_name {
        "enter" => NamedKeyAction::Enter,
        "esc" => NamedKeyAction::Escape,
        "tab" => NamedKeyAction::ModalTab { reverse: false },
        "backtab" => NamedKeyAction::ModalTab { reverse: true },
        "up" => NamedKeyAction::MoveSelection(-1),
        "down" => NamedKeyAction::MoveSelection(1),
        "left" => NamedKeyAction::Horizontal(-1),
        "right" => NamedKeyAction::Horizontal(1),
        "backspace" => NamedKeyAction::Backspace,
        _ => NamedKeyAction::Ignore,
    }
}

fn classify_character_action(ch: char) -> CharacterAction {
    match ch {
        '?' => CharacterAction::Help,
        '1' => CharacterAction::Screen(ScreenId::Neighborhood),
        '2' => CharacterAction::Screen(ScreenId::Chat),
        '3' => CharacterAction::Screen(ScreenId::Contacts),
        '4' => CharacterAction::Screen(ScreenId::Notifications),
        '5' => CharacterAction::Screen(ScreenId::Settings),
        'j' => CharacterAction::MoveSelection(1),
        'k' => CharacterAction::MoveSelection(-1),
        'h' => CharacterAction::Horizontal(-1),
        'l' => CharacterAction::Horizontal(1),
        'q' => CharacterAction::DisabledQuit,
        'c' | 'y' => CharacterAction::ModalCopyInviteCode,
        _ => CharacterAction::ScreenLocal,
    }
}

fn set_toast(model: &mut UiModel, icon: char, message: impl Into<String>) {
    model.toast_key = model.toast_key.saturating_add(1);
    model.toast = Some(ToastState {
        icon,
        message: message.into(),
    });
}

pub fn apply_text_keys(model: &mut UiModel, keys: &str, clipboard: &dyn ClipboardPort) {
    for ch in keys.chars() {
        match ch {
            '\n' | '\r' => handle_enter(model, clipboard),
            '\u{08}' | '\u{7f}' => backspace(model),
            '\u{1b}' => handle_escape(model),
            _ => apply_char(model, ch, clipboard),
        }
    }
}

pub fn apply_named_key(model: &mut UiModel, key: &str, repeat: u16, clipboard: &dyn ClipboardPort) {
    let repeat = repeat.max(1);
    let key_name = key.trim().to_ascii_lowercase();
    for _ in 0..repeat {
        if handle_wizard_named_key(model, key_name.as_str()) {
            continue;
        }
        match classify_named_key(key_name.as_str()) {
            NamedKeyAction::Enter => handle_enter(model, clipboard),
            NamedKeyAction::Escape => handle_escape(model),
            NamedKeyAction::ModalTab { reverse } => {
                if !handle_modal_tab(model, reverse) {
                    if reverse {
                        cycle_screen_prev(model);
                    } else {
                        cycle_screen(model);
                    }
                }
            }
            NamedKeyAction::MoveSelection(delta) => move_selection(model, delta),
            NamedKeyAction::Horizontal(delta) => handle_horizontal(model, delta),
            NamedKeyAction::Backspace => backspace(model),
            NamedKeyAction::Ignore => {}
        }
    }
}

fn apply_char(model: &mut UiModel, ch: char, clipboard: &dyn ClipboardPort) {
    if ch.is_control() {
        return;
    }

    if model.input_mode {
        model.input_buffer.push(ch);
        return;
    }

    if let Some(modal) = model.modal_state() {
        if handle_modal_char(model, modal, ch, clipboard) {
            return;
        }
        if matches!(modal, ModalState::CreateInvitation)
            && matches!(
                classify_character_action(ch),
                CharacterAction::ModalCopyInviteCode
            )
        {
            if let Some(code) = model.last_invite_code.clone() {
                clipboard.write(&code);
                set_toast(model, '✓', "Copied to clipboard");
                return;
            }
        }
        if model.modal_accepts_text() {
            model.append_modal_text_char(ch);
        }
        return;
    }

    match classify_character_action(ch) {
        CharacterAction::Help => {
            model.modal_hint = format!("Help - {}", model.screen.help_label());
            model.active_modal = Some(ActiveModal::Help);
            return;
        }
        CharacterAction::Screen(screen) => {
            model.set_screen(screen);
            return;
        }
        CharacterAction::MoveSelection(delta) => {
            move_selection(model, delta);
            return;
        }
        CharacterAction::Horizontal(delta) => {
            handle_horizontal(model, delta);
            return;
        }
        CharacterAction::DisabledQuit => {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "Quit is disabled in web shell".to_string(),
            });
            return;
        }
        CharacterAction::ModalCopyInviteCode | CharacterAction::ScreenLocal => {}
    };

    match model.screen {
        ScreenId::Onboarding => {}
        ScreenId::Chat => handle_chat_char(model, ch),
        ScreenId::Contacts => handle_contacts_char(model, ch),
        ScreenId::Neighborhood => handle_neighborhood_char(model, ch),
        ScreenId::Settings => handle_settings_char(model, ch),
        ScreenId::Notifications => notifications::handle_notifications_char(model, ch),
    }
}

fn handle_enter(model: &mut UiModel, clipboard: &dyn ClipboardPort) {
    if model.input_mode {
        let text = model.input_buffer.trim().to_string();
        model.input_buffer.clear();
        if !text.is_empty() {
            chat::submit_chat_input(model, &text);
        }
        return;
    }

    if let Some(modal) = model.modal_state() {
        handle_modal_enter(model, modal, clipboard);
        return;
    }

    match model.screen {
        ScreenId::Onboarding => {}
        ScreenId::Neighborhood => {
            if matches!(model.neighborhood_mode, crate::model::NeighborhoodMode::Map) {
                model.neighborhood_mode = crate::model::NeighborhoodMode::Detail;
            } else {
                model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
            }
        }
        ScreenId::Contacts => {
            model.contact_details = true;
        }
        ScreenId::Settings => match model.settings_section {
            SettingsSection::Profile => {
                model.modal_hint = "Edit Nickname".to_string();
                model.active_modal = Some(ActiveModal::EditNickname(TextModalState {
                    value: model.profile_nickname.clone(),
                }));
            }
            SettingsSection::GuardianThreshold => {
                if can_open_guardian_setup_wizard(model) {
                    open_guardian_setup_wizard(model);
                }
            }
            SettingsSection::RequestRecovery => {
                model.modal_hint = "Request Recovery".to_string();
                model.active_modal = Some(ActiveModal::RequestRecovery);
            }
            SettingsSection::Devices => {
                open_add_device_wizard(model);
            }
            SettingsSection::Authority => {
                if model.authorities.len() <= 1 {
                    set_toast(model, 'ℹ', "Only one authority available");
                } else {
                    model.modal_hint = "Switch Authority".to_string();
                    model.active_modal = Some(ActiveModal::SwitchAuthority);
                }
            }
            SettingsSection::Appearance | SettingsSection::Info => {}
        },
        ScreenId::Chat | ScreenId::Notifications => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_named_key, apply_text_keys};
    use crate::model::{
        ActiveModal, AddDeviceModalState, AddDeviceWizardStep, CreateChannelModalState,
        CreateChannelWizardStep, CreateInvitationModalState, ModalState, ScreenId, SettingsSection,
        TextModalState, ThresholdWizardModalState, ThresholdWizardStep, UiModel,
    };
    use aura_app::frontend_primitives::{ClipboardPort, MemoryClipboard};

    fn modal_state(model: &UiModel) -> Option<ModalState> {
        model.modal_state()
    }

    fn create_channel_state(model: &UiModel) -> &CreateChannelModalState {
        match model.active_modal.as_ref() {
            Some(ActiveModal::CreateChannel(state)) => state,
            _ => panic!("expected create channel modal"),
        }
    }

    fn guardian_state(model: &UiModel) -> &ThresholdWizardModalState {
        match model.active_modal.as_ref() {
            Some(ActiveModal::GuardianSetup(state)) => state,
            _ => panic!("expected guardian setup modal"),
        }
    }

    fn mfa_state(model: &UiModel) -> &ThresholdWizardModalState {
        match model.active_modal.as_ref() {
            Some(ActiveModal::MfaSetup(state)) => state,
            _ => panic!("expected mfa setup modal"),
        }
    }

    fn add_device_state(model: &UiModel) -> &AddDeviceModalState {
        match model.active_modal.as_ref() {
            Some(ActiveModal::AddDevice(state)) => state,
            _ => panic!("expected add device modal"),
        }
    }

    #[test]
    fn contacts_invite_shortcut_opens_invite_modal() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Contacts);
        apply_text_keys(&mut model, "n", &clipboard);

        assert!(matches!(
            modal_state(&model),
            Some(ModalState::CreateInvitation)
        ));
    }

    #[test]
    fn create_invitation_modal_copy_shortcut_writes_clipboard() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
            receiver_id: String::new(),
            receiver_label: None,
            message: String::new(),
            ttl_hours: 24,
            active_field: aura_app::ui::contract::FieldId::InvitationReceiver,
        }));
        model.last_invite_code = Some("INVITE-9".to_string());

        apply_text_keys(&mut model, "c", &clipboard);

        assert_eq!(clipboard.read(), "INVITE-9");
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Copied to clipboard")
        );
    }

    #[test]
    fn accept_invitation_letter_shortcuts_map_to_demo_contacts() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.active_modal = Some(ActiveModal::AcceptContactInvitation(TextModalState {
            value: "a".to_string(),
        }));
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.contacts.iter().any(|row| row.name == "Alice"));

        model.active_modal = Some(ActiveModal::AcceptContactInvitation(TextModalState {
            value: "l".to_string(),
        }));
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.contacts.iter().any(|row| row.name == "Carol"));
    }

    #[test]
    fn neighborhood_new_home_shortcut_opens_modal() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Neighborhood);
        apply_text_keys(&mut model, "n", &clipboard);

        assert!(matches!(modal_state(&model), Some(ModalState::CreateHome)));
    }

    #[test]
    fn chat_shortcuts_open_expected_actions() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);

        apply_text_keys(&mut model, "n", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::CreateChannel)
        ));

        model.dismiss_modal();
        apply_text_keys(&mut model, "i", &clipboard);
        assert!(model.input_mode);
    }

    #[test]
    fn create_channel_modal_uses_multistep_wizard_flow() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Chat);
        model.ensure_contact("Bob");
        model.ensure_contact("Carol");

        apply_text_keys(&mut model, "n", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::CreateChannel)
        ));
        assert_eq!(model.modal_hint, "New Chat Group — Step 1 of 3");
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Details
        );

        apply_text_keys(&mut model, "team-room", &clipboard);
        apply_named_key(&mut model, "tab", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Details
        );

        apply_text_keys(&mut model, "bootstrap-topic", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Members
        );
        assert_eq!(model.modal_hint, "New Chat Group — Step 2 of 3");

        apply_named_key(&mut model, "down", 1, &clipboard);
        apply_text_keys(&mut model, " ", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Threshold
        );
        assert_eq!(model.modal_hint, "New Chat Group — Step 3 of 3");

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal_state().is_none());
        assert!(model.channels.iter().any(|row| row.name == "team-room"));
        assert_eq!(model.selected_channel_topic(), "bootstrap-topic");
    }

    #[test]
    fn create_channel_enter_from_name_advances_to_members() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Chat);

        apply_text_keys(&mut model, "n", &clipboard);
        apply_text_keys(&mut model, "demo-trio-room", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Members
        );
        assert_eq!(model.modal_hint, "New Chat Group — Step 2 of 3");
    }

    #[test]
    fn chat_enter_keeps_insert_mode_after_sending_message() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        model.input_mode = true;
        model.input_buffer = "hello".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        assert!(model.input_mode);
        assert!(model.input_buffer.is_empty());
        assert_eq!(model.messages.last().map(String::as_str), Some("hello"));
    }

    #[test]
    fn chat_nhlink_command_reports_permission_denied() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        model.input_mode = true;
        model.input_buffer = "/nhlink home".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let Some(toast) = model.toast else {
            panic!("nhlink should emit a toast");
        };
        assert!(toast.message.contains("status=denied"));
        assert!(toast.message.contains("reason=permission_denied"));
        assert!(toast.message.contains("consistency=accepted"));
    }

    #[test]
    fn chat_pin_command_reports_permission_denied() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        model.input_mode = true;
        model.input_buffer = "/pin msg-1".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let Some(toast) = model.toast else {
            panic!("pin should emit a toast");
        };
        assert!(toast.message.contains("status=denied"));
        assert!(toast.message.contains("reason=permission_denied"));
    }

    #[test]
    fn chat_mode_minus_reports_enforced_success() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        model.input_mode = true;
        model.input_buffer = "/mode slash-lab -m".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let Some(toast) = model.toast else {
            panic!("mode -m should emit a toast");
        };
        assert!(toast.message.contains("status=ok"));
        assert!(toast.message.contains("consistency=enforced"));
    }

    #[test]
    fn demo_trio_channel_synthesizes_alice_and_carol_replies() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        let channel_id =
            super::chat::ensure_named_channel(&mut model, "demo-trio-room", String::new());
        model.select_channel_id(Some(&channel_id));
        model.input_mode = true;
        model.input_buffer = "demo-e2e-trio-token".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        assert!(model.messages.iter().any(|msg| msg.contains("Alice")));
        assert!(model.messages.iter().any(|msg| msg.contains("Carol")));
    }

    #[test]
    fn ensure_named_channel_reuses_existing_channel_id_for_matching_name() {
        let mut model = UiModel::new("authority-local".to_string());
        model.channels.push(crate::model::ChannelRow {
            id: "channel-123".to_string(),
            name: "Slash Lab".to_string(),
            selected: false,
            topic: String::new(),
        });

        let channel_id =
            super::chat::ensure_named_channel(&mut model, "slash lab", "updated topic".to_string());

        assert_eq!(channel_id, "channel-123");
        assert_eq!(model.channels.len(), 1);
        assert_eq!(model.channels[0].topic, "updated topic");
    }

    #[test]
    fn chat_unknown_command_reports_not_found_reason() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Chat);
        model.input_mode = true;
        model.input_buffer = "/unknowncmd".to_string();

        apply_text_keys(&mut model, "\n", &clipboard);

        let Some(toast) = model.toast else {
            panic!("unknown command should emit a toast");
        };
        assert!(toast.message.contains("status=invalid"));
        assert!(toast.message.contains("reason=not_found"));
    }

    #[test]
    fn settings_shortcuts_open_or_toast_expected_actions() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Settings);

        model.settings_section = SettingsSection::Profile;
        apply_text_keys(&mut model, "e", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::EditNickname)
        ));
        model.dismiss_modal();

        model.settings_section = SettingsSection::GuardianThreshold;
        apply_text_keys(&mut model, "t", &clipboard);
        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Add contacts first before setting up guardians")
        );

        model.settings_section = SettingsSection::RequestRecovery;
        apply_text_keys(&mut model, "s", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::RequestRecovery)
        ));
        model.dismiss_modal();

        model.settings_section = SettingsSection::Devices;
        apply_text_keys(&mut model, "a", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::AddDeviceStep1)
        ));
        model.dismiss_modal();
        apply_text_keys(&mut model, "i", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::ImportDeviceEnrollmentCode)
        ));
        model.dismiss_modal();
        apply_text_keys(&mut model, "r", &clipboard);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Cannot remove the current device")
        );

        model.settings_section = SettingsSection::Authority;
        apply_text_keys(&mut model, "s", &clipboard);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Only one authority available")
        );
        apply_text_keys(&mut model, "m", &clipboard);
        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("MFA requires at least 2 devices, but only 1 available")
        );
    }

    #[test]
    fn settings_remove_device_toast_repeats_with_new_event_key() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Devices;

        apply_text_keys(&mut model, "r", &clipboard);
        let first_key = model.toast_key;
        let first_message = model
            .toast
            .as_ref()
            .map(|toast| toast.message.clone())
            .unwrap_or_default();

        apply_text_keys(&mut model, "r", &clipboard);
        let second_key = model.toast_key;
        let second_message = model
            .toast
            .as_ref()
            .map(|toast| toast.message.clone())
            .unwrap_or_default();

        assert_eq!(first_message, "Cannot remove the current device");
        assert_eq!(second_message, "Cannot remove the current device");
        assert!(second_key > first_key);
    }

    #[test]
    fn settings_remove_device_succeeds_when_secondary_device_exists() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Devices;
        model.has_secondary_device = true;
        model.set_secondary_device_name(Some("Laptop".to_string()));

        apply_text_keys(&mut model, "r", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::SelectDeviceToRemove)
        ));
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::ConfirmRemoveDevice)
        ));
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert!(!model.has_secondary_device);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Device removal complete")
        );
    }

    #[test]
    fn guardian_setup_wizard_advances_through_steps() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::GuardianThreshold;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");
        model.ensure_contact("Carol");

        apply_text_keys(&mut model, "t", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::GuardianSetup)
        ));
        assert_eq!(guardian_state(&model).step, ThresholdWizardStep::Selection);

        apply_named_key(&mut model, "down", 2, &clipboard);
        apply_text_keys(&mut model, " ", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(guardian_state(&model).step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(guardian_state(&model).step, ThresholdWizardStep::Ceremony);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Guardian ceremony started! Waiting for 2-of-3 guardians to respond")
        );
    }

    #[test]
    fn mfa_setup_wizard_advances_through_steps() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Authority;
        model.has_secondary_device = true;

        apply_text_keys(&mut model, "m", &clipboard);
        assert!(matches!(modal_state(&model), Some(ModalState::MfaSetup)));
        assert_eq!(mfa_state(&model).step, ThresholdWizardStep::Selection);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(mfa_state(&model).step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(mfa_state(&model).step, ThresholdWizardStep::Ceremony);

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Multifactor ceremony started (2-of-2)")
        );
    }

    #[test]
    fn settings_add_device_wizard_requires_name_then_generates_code() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Devices;

        apply_text_keys(&mut model, "a", &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::AddDeviceStep1)
        ));
        assert_eq!(model.modal_hint, "Add Device — Step 1 of 3");

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::AddDeviceStep1)
        ));
        assert_eq!(add_device_state(&model).step, AddDeviceWizardStep::Name);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Device name is required")
        );

        apply_text_keys(&mut model, "Laptop", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::AddDeviceStep1)
        ));
        assert_eq!(
            add_device_state(&model).step,
            AddDeviceWizardStep::ShareCode
        );
        assert_eq!(model.modal_hint, "Add Device — Step 2 of 3");
        assert!(!add_device_state(&model).enrollment_code.is_empty());
    }

    #[test]
    fn settings_add_device_wizard_can_copy_generated_code() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Devices;

        apply_text_keys(&mut model, "aPhone\n", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        apply_text_keys(&mut model, "c", &clipboard);

        assert_eq!(clipboard.read(), add_device_state(&model).enrollment_code);
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Copied to clipboard")
        );
    }

    #[test]
    fn request_recovery_requires_guardians_like_tui() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::RequestRecovery;

        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(matches!(
            modal_state(&model),
            Some(ModalState::RequestRecovery)
        ));
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Set up guardians first before requesting recovery")
        );
    }

    #[test]
    fn request_recovery_starts_when_guardians_available() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::RequestRecovery;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");
        apply_named_key(&mut model, "enter", 1, &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);

        assert!(model.modal_state().is_none());
        assert_eq!(
            model.toast.as_ref().map(|toast| toast.message.as_str()),
            Some("Recovery process started")
        );
    }

    #[test]
    fn create_channel_escape_steps_back_like_tui() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Chat);
        model.ensure_contact("Alice");

        apply_text_keys(&mut model, "nroom\n", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Threshold
        );

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Members
        );
        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(
            create_channel_state(&model).step,
            CreateChannelWizardStep::Details
        );
    }

    #[test]
    fn guardian_setup_escape_from_threshold_returns_selection() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::GuardianThreshold;
        model.ensure_contact("Alice");
        model.ensure_contact("Bob");

        apply_text_keys(&mut model, "t", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(guardian_state(&model).step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(guardian_state(&model).step, ThresholdWizardStep::Selection);
    }

    #[test]
    fn mfa_setup_escape_from_threshold_returns_selection() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();
        model.set_screen(ScreenId::Settings);
        model.settings_section = SettingsSection::Authority;
        model.has_secondary_device = true;

        apply_text_keys(&mut model, "m", &clipboard);
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert_eq!(mfa_state(&model).step, ThresholdWizardStep::Threshold);

        apply_named_key(&mut model, "esc", 1, &clipboard);
        assert_eq!(mfa_state(&model).step, ThresholdWizardStep::Selection);
    }
}

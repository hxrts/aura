//! Keyboard input handling and navigation logic.
//!
//! Processes keyboard events to navigate screens, update selections, handle text
//! input, and dispatch commands across the UI model state machine.

use crate::clipboard::ClipboardPort;
use crate::model::{
    AccessDepth, AccessOverrideModalState, ActiveModal, AddDeviceModalState, AddDeviceWizardStep,
    CapabilityConfigModalState, CapabilityTier, ChannelRow, CreateChannelDetailsField,
    CreateChannelModalState, CreateChannelWizardStep, CreateInvitationModalState, ModalState,
    ScreenId, SelectDeviceModalState, SelectedHome, SettingsSection, TextModalState,
    ThresholdWizardModalState, ThresholdWizardStep, ToastState, UiModel,
};
use aura_app::ui::types::parse_chat_command;
use aura_app::views::chat::{NOTE_TO_SELF_CHANNEL_NAME, NOTE_TO_SELF_CHANNEL_TOPIC};

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
        match key_name.as_str() {
            "enter" => handle_enter(model, clipboard),
            "esc" => handle_escape(model),
            "tab" => {
                if !handle_modal_tab(model, false) {
                    cycle_screen(model);
                }
            }
            "backtab" => {
                if !handle_modal_tab(model, true) {
                    cycle_screen_prev(model);
                }
            }
            "up" => move_selection(model, -1),
            "down" => move_selection(model, 1),
            "left" => handle_horizontal(model, -1),
            "right" => handle_horizontal(model, 1),
            "backspace" => backspace(model),
            _ => {}
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
        if matches!(modal, ModalState::CreateInvitation) && matches!(ch, 'c' | 'y') {
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

    match ch {
        '?' => {
            model.modal_hint = format!("Help - {}", model.screen.help_label());
            model.active_modal = Some(ActiveModal::Help);
            return;
        }
        '1' => {
            model.set_screen(ScreenId::Neighborhood);
            return;
        }
        '2' => {
            model.set_screen(ScreenId::Chat);
            return;
        }
        '3' => {
            model.set_screen(ScreenId::Contacts);
            return;
        }
        '4' => {
            model.set_screen(ScreenId::Notifications);
            return;
        }
        '5' => {
            model.set_screen(ScreenId::Settings);
            return;
        }
        'j' => {
            move_selection(model, 1);
            return;
        }
        'k' => {
            move_selection(model, -1);
            return;
        }
        'h' => {
            handle_horizontal(model, -1);
            return;
        }
        'l' => {
            handle_horizontal(model, 1);
            return;
        }
        'q' => {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "Quit is disabled in web shell".to_string(),
            });
            return;
        }
        _ => {}
    }

    match model.screen {
        ScreenId::Onboarding => {}
        ScreenId::Chat => handle_chat_char(model, ch),
        ScreenId::Contacts => handle_contacts_char(model, ch),
        ScreenId::Neighborhood => handle_neighborhood_char(model, ch),
        ScreenId::Settings => handle_settings_char(model, ch),
        ScreenId::Notifications => {}
    }
}

fn handle_chat_char(model: &mut UiModel, ch: char) {
    match ch {
        'i' => {
            model.input_mode = true;
            model.input_buffer.clear();
        }
        'n' => {
            open_create_channel_wizard(model);
        }
        't' => {
            model.modal_hint = "Set Channel Topic".to_string();
            model.active_modal = Some(ActiveModal::SetChannelTopic(TextModalState {
                value: model.selected_channel_topic().to_string(),
            }));
        }
        'o' => {
            let channel = model
                .selected_channel_name()
                .unwrap_or(NOTE_TO_SELF_CHANNEL_NAME);
            model.modal_hint = format!("Channel: #{channel}");
            model.active_modal = Some(ActiveModal::ChannelInfo);
        }
        'r' => {
            model.toast = Some(ToastState {
                icon: 'ℹ',
                message: "No message selected".to_string(),
            });
        }
        _ => {}
    }
}

fn handle_contacts_char(model: &mut UiModel, ch: char) {
    match ch {
        'n' => {
            model.modal_hint = "Invite Contacts".to_string();
            model.active_modal = Some(ActiveModal::CreateInvitation(CreateInvitationModalState {
                receiver_id: model
                    .selected_contact_authority_id()
                    .map(|authority_id| authority_id.to_string())
                    .unwrap_or_default(),
                receiver_label: model.selected_contact_name().map(str::to_string),
            }));
        }
        'a' => {
            model.modal_hint = "Accept Invitation".to_string();
            model.active_modal = Some(ActiveModal::AcceptInvitation(TextModalState::default()));
        }
        'e' => {
            model.modal_hint = "Edit Nickname".to_string();
            model.active_modal = Some(ActiveModal::EditNickname(TextModalState {
                value: model
                    .selected_contact_name()
                    .unwrap_or_default()
                    .to_string(),
            }));
        }
        'g' => {
            open_guardian_setup_wizard(model);
        }
        'c' => {
            if let Some(contact) = model.selected_contact_name().map(str::to_string) {
                model.set_screen(ScreenId::Chat);
                let channel_id =
                    ensure_named_channel(model, &format!("DM: {contact}"), String::new());
                model.select_channel_id(Some(&channel_id));
            }
        }
        'd' => {}
        'p' => {}
        'r' => {
            model.modal_hint = "Remove Contact".to_string();
            model.active_modal = Some(ActiveModal::RemoveContact);
        }
        _ => {}
    }
}

fn handle_neighborhood_char(model: &mut UiModel, ch: char) {
    match ch {
        'n' => {
            model.modal_hint = "Create New Home".to_string();
            model.active_modal = Some(ActiveModal::CreateHome(TextModalState::default()));
        }
        'a' => {
            model.modal_hint = "Accept Invitation".to_string();
            model.active_modal = Some(ActiveModal::AcceptInvitation(TextModalState::default()));
        }
        'd' => {
            model.access_depth = model.access_depth.next();
            set_toast(
                model,
                'ℹ',
                format!("Access depth set to {} access", model.access_depth.label()),
            );
        }
        'm' => {}
        'v' => {}
        'L' => {}
        'g' | 'H' => {
            if model.selected_home.is_none() {
                model.selected_home = Some(SelectedHome {
                    id: "Neighborhood".to_string(),
                    name: "Neighborhood".to_string(),
                });
            }
            set_toast(model, 'ℹ', "Viewing the neighborhood map");
        }
        'b' => {
            model.access_depth = AccessDepth::Limited;
            model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
            set_toast(model, 'ℹ', "Returned to the neighborhood map");
        }
        'o' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Assign Moderator".to_string();
            model.active_modal = Some(ActiveModal::AssignModerator);
        }
        'x' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Access Override".to_string();
            model.active_modal = Some(ActiveModal::AccessOverride(
                AccessOverrideModalState::default(),
            ));
        }
        'p' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.modal_hint = "Home Capability Configuration".to_string();
            model.active_modal = Some(ActiveModal::CapabilityConfig(
                CapabilityConfigModalState::default(),
            ));
        }
        'i' if matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        ) =>
        {
            model.input_mode = true;
            model.input_buffer.clear();
        }
        _ => {}
    }
}

fn handle_settings_char(model: &mut UiModel, ch: char) {
    match ch {
        'e' if matches!(model.settings_section, SettingsSection::Profile) => {
            model.modal_hint = "Edit Nickname".to_string();
            model.active_modal = Some(ActiveModal::EditNickname(TextModalState {
                value: model.profile_nickname.clone(),
            }));
        }
        't' if matches!(model.settings_section, SettingsSection::GuardianThreshold) => {
            if can_open_guardian_setup_wizard(model) {
                open_guardian_setup_wizard(model);
            }
        }
        'a' if matches!(model.settings_section, SettingsSection::Devices) => {
            open_add_device_wizard(model);
        }
        'i' if matches!(model.settings_section, SettingsSection::Devices) => {
            model.modal_hint = "Import Device Enrollment Code".to_string();
            model.active_modal = Some(ActiveModal::ImportDeviceEnrollmentCode(
                TextModalState::default(),
            ));
        }
        'r' if matches!(model.settings_section, SettingsSection::Devices) => {
            if model.has_secondary_device {
                open_remove_device_selection(model);
            } else {
                set_toast(model, 'ℹ', "Cannot remove the current device");
            }
        }
        's' if matches!(model.settings_section, SettingsSection::RequestRecovery) => {
            model.modal_hint = "Request Recovery".to_string();
            model.active_modal = Some(ActiveModal::RequestRecovery);
        }
        's' if matches!(model.settings_section, SettingsSection::Authority) => {
            if model.authorities.len() <= 1 {
                set_toast(model, 'ℹ', "Only one authority available");
            } else {
                model.modal_hint = "Switch Authority".to_string();
                model.active_modal = Some(ActiveModal::SwitchAuthority);
            }
        }
        'm' if matches!(model.settings_section, SettingsSection::Authority) => {
            if can_open_mfa_setup_wizard(model) {
                open_mfa_setup_wizard(model);
            }
        }
        _ => {}
    }
}

fn handle_enter(model: &mut UiModel, clipboard: &dyn ClipboardPort) {
    if model.input_mode {
        let text = model.input_buffer.trim().to_string();
        model.input_buffer.clear();
        if !text.is_empty() {
            submit_chat_input(model, &text);
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

fn handle_modal_enter(model: &mut UiModel, modal: ModalState, clipboard: &dyn ClipboardPort) {
    match modal {
        ModalState::Help | ModalState::ChannelInfo => {
            dismiss_modal(model);
        }
        ModalState::CreateInvitation => {
            model.invite_counter = model.invite_counter.saturating_add(1);
            let code = format!("INVITE-{}", model.invite_counter);
            model.last_invite_code = Some(code.clone());
            clipboard.write(&code);
            model.ensure_contact("Bob");
            model.toast = Some(ToastState {
                icon: '✓',
                message: format!("Invitation Created {code}"),
            });
            dismiss_modal(model);
        }
        ModalState::AcceptInvitation => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::AcceptInvitation(state)) => state.value.trim(),
                _ => "",
            };
            if !value.is_empty() {
                let contact_name = match value {
                    "1" => "Alice",
                    "2" => "Carol",
                    _ => "Alice",
                };
                model.ensure_contact(contact_name);
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "membership updated".to_string(),
                });
            }
            dismiss_modal(model);
        }
        ModalState::CreateHome => {
            let name = match model.active_modal.as_ref() {
                Some(ActiveModal::CreateHome(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            if !name.is_empty() {
                model.select_home(
                    format!("home-{}", name.to_lowercase().replace(' ', "-")),
                    name.clone(),
                );
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: format!("Home '{name}' created"),
                });
            }
            dismiss_modal(model);
        }
        ModalState::CreateChannel => {
            if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
                match state.step {
                    CreateChannelWizardStep::Details => {
                        let channel = state.name.trim().trim_start_matches('#').to_string();
                        if channel.is_empty() {
                            set_toast(model, '✗', "Channel name is required");
                            return;
                        }
                        state.name = channel;
                        state.step = CreateChannelWizardStep::Members;
                        state.member_focus = 0;
                        model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
                    }
                    CreateChannelWizardStep::Members => {
                        let selected_count = state.selected_members.len();
                        let participants = selected_count.saturating_add(1);
                        state.threshold = participants.max(1) as u8;
                        state.step = CreateChannelWizardStep::Threshold;
                        model.modal_hint = "New Chat Group — Step 3 of 3".to_string();
                    }
                    CreateChannelWizardStep::Threshold => {
                        let max_threshold = (state.selected_members.len().saturating_add(1)) as u8;
                        state.threshold = state.threshold.clamp(1, max_threshold.max(1));
                        let channel = state.name.trim().to_string();
                        let topic = state.topic.clone();
                        let channel_id = ensure_named_channel(model, &channel, topic.clone());
                        model.select_channel_id(Some(&channel_id));
                        if !topic.trim().is_empty() {
                            model.set_selected_channel_topic(topic);
                        }
                        model.toast = Some(ToastState {
                            icon: '✓',
                            message: format!("Created '{channel}'."),
                        });
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::SetChannelTopic => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::SetChannelTopic(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            model.set_selected_channel_topic(value);
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Topic updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::EditNickname => {
            let value = match model.active_modal.as_ref() {
                Some(ActiveModal::EditNickname(state)) => state.value.trim().to_string(),
                _ => String::new(),
            };
            if model.screen == ScreenId::Settings {
                if !value.is_empty() {
                    model.profile_nickname = value;
                }
            } else if !value.is_empty() {
                model.set_selected_contact_name(value);
            }
            dismiss_modal(model);
        }
        ModalState::RemoveContact => {
            if !model.contacts.is_empty() {
                if let Some(selected_index) = model.selected_contact_index() {
                    model.contacts.remove(selected_index);
                    if model.contacts.is_empty() {
                        model.selected_contact_id = None;
                    } else {
                        model.set_selected_contact_index(selected_index.saturating_sub(1));
                    }
                }
            }
            model.toast = Some(ToastState {
                icon: '✓',
                message: "membership updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::GuardianSetup => {
            if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        let available = model.contacts.len();
                        if available < 2 {
                            set_toast(
                                model,
                                '✗',
                                format!(
                                    "Need at least 2 contacts for this threshold, but only {available} available"
                                ),
                            );
                            return;
                        }
                        if state.selected_indices.len() < 2 {
                            set_toast(model, '✗', "Select at least 2 guardians");
                            return;
                        }
                        let selected = state.selected_indices.len() as u8;
                        state.selected_count = selected;
                        state.threshold_k = state.threshold_k.clamp(1, selected.max(1));
                        state.threshold_input = state.threshold_k.to_string();
                        state.step = ThresholdWizardStep::Threshold;
                    }
                    ThresholdWizardStep::Threshold => {
                        let k = parse_wizard_value(&state.threshold_input, state.threshold_k)
                            .clamp(1, state.selected_count);
                        state.threshold_k = k;
                        state.threshold_input.clear();
                        state.step = ThresholdWizardStep::Ceremony;
                    }
                    ThresholdWizardStep::Ceremony => {
                        let threshold_k = state.threshold_k;
                        let selected_count = state.selected_count;
                        set_toast(
                            model,
                            'ℹ',
                            format!(
                                "Guardian ceremony started! Waiting for {threshold_k}-of-{selected_count} guardians to respond"
                            ),
                        );
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::RequestRecovery => {
            let required = match model.active_modal.as_ref() {
                Some(ActiveModal::GuardianSetup(state)) => state.threshold_k.max(1) as usize,
                _ => 1,
            };
            let available = model.contacts.len();
            if available == 0 {
                set_toast(
                    model,
                    '✗',
                    "Set up guardians first before requesting recovery",
                );
                dismiss_modal(model);
                return;
            }
            if available < required {
                set_toast(
                    model,
                    '✗',
                    format!(
                        "Need {required} guardians for recovery, but only {available} configured"
                    ),
                );
                dismiss_modal(model);
                return;
            }
            set_toast(model, 'ℹ', "Recovery process started");
            dismiss_modal(model);
        }
        ModalState::AddDeviceStep1 => {
            if let Some(ActiveModal::AddDevice(state)) = model.active_modal.as_mut() {
                match state.step {
                    AddDeviceWizardStep::Name => {
                        let name = state.name_input.trim().to_string();
                        if name.is_empty() {
                            set_toast(model, '✗', "Device name is required");
                            return;
                        }

                        state.device_name = name;
                        model.device_enrollment_counter =
                            model.device_enrollment_counter.saturating_add(1);
                        state.enrollment_code =
                            format!("DEVICE-ENROLL-{}", model.device_enrollment_counter);
                        state.name_input.clear();
                        state.step = AddDeviceWizardStep::ShareCode;
                        model.modal_hint = "Add Device — Step 2 of 3".to_string();
                    }
                    AddDeviceWizardStep::ShareCode => {
                        state.step = AddDeviceWizardStep::Confirm;
                        model.modal_hint = "Add Device — Step 3 of 3".to_string();
                    }
                    AddDeviceWizardStep::Confirm => {
                        set_toast(model, 'ℹ', "Device enrollment started");
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::ImportDeviceEnrollmentCode => {
            let code = match model.active_modal.as_ref() {
                Some(ActiveModal::ImportDeviceEnrollmentCode(state)) => state.value.trim(),
                _ => "",
            };
            if code.is_empty() {
                set_toast(model, '✗', "Enrollment code is required");
                return;
            }
            model.has_secondary_device = true;
            if model.secondary_device_name().is_none() {
                let fallback = match model.active_modal.as_ref() {
                    Some(ActiveModal::AddDevice(state)) if !state.device_name.trim().is_empty() => {
                        state.device_name.clone()
                    }
                    _ => "Mobile".to_string(),
                };
                model.set_secondary_device_name(Some(fallback));
            }
            set_toast(model, '✓', "Device enrollment complete");
            dismiss_modal(model);
        }
        ModalState::SelectDeviceToRemove => {
            let candidate_name = match model.active_modal.as_ref() {
                Some(ActiveModal::SelectDeviceToRemove(state)) => state.candidate_name.clone(),
                _ => model
                    .secondary_device_name()
                    .unwrap_or("Secondary device")
                    .to_string(),
            };
            model.modal_hint = "Confirm Device Removal".to_string();
            model.active_modal = Some(ActiveModal::ConfirmRemoveDevice(SelectDeviceModalState {
                candidate_name,
            }));
        }
        ModalState::ConfirmRemoveDevice => {
            if model.has_secondary_device {
                model.has_secondary_device = false;
                model.set_secondary_device_name(None);
                set_toast(model, '✓', "Device removal complete");
            } else {
                set_toast(model, 'ℹ', "Cannot remove the current device");
            }
            dismiss_modal(model);
        }
        ModalState::MfaSetup => {
            if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
                match state.step {
                    ThresholdWizardStep::Selection => {
                        if state.selected_indices.is_empty() {
                            set_toast(model, '✗', "Select at least 1 device");
                            return;
                        }
                        let selected = state.selected_indices.len() as u8;
                        state.selected_count = selected;
                        state.threshold_k = state.threshold_k.clamp(1, state.selected_count);
                        state.threshold_input = state.threshold_k.to_string();
                        state.step = ThresholdWizardStep::Threshold;
                    }
                    ThresholdWizardStep::Threshold => {
                        let k = parse_wizard_value(&state.threshold_input, state.threshold_k)
                            .clamp(1, state.selected_count);
                        state.threshold_k = k;
                        state.threshold_input.clear();
                        state.step = ThresholdWizardStep::Ceremony;
                    }
                    ThresholdWizardStep::Ceremony => {
                        let threshold_k = state.threshold_k;
                        let selected_count = state.selected_count;
                        set_toast(
                            model,
                            'ℹ',
                            format!(
                                "Multifactor ceremony started ({threshold_k}-of-{selected_count})"
                            ),
                        );
                        dismiss_modal(model);
                    }
                }
            }
        }
        ModalState::AssignModerator => {
            if model.contacts.is_empty() {
                model.toast = Some(ToastState {
                    icon: '✗',
                    message: "Cannot designate non-member".to_string(),
                });
            } else {
                model.toast = Some(ToastState {
                    icon: '✓',
                    message: "moderator granted".to_string(),
                });
            }
            dismiss_modal(model);
        }
        ModalState::SwitchAuthority => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Authority switch requested".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::AccessOverride => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Access override updated".to_string(),
            });
            dismiss_modal(model);
        }
        ModalState::CapabilityConfig => {
            model.toast = Some(ToastState {
                icon: '✓',
                message: "Capability config saved".to_string(),
            });
            dismiss_modal(model);
        }
    }
}

fn submit_chat_input(model: &mut UiModel, text: &str) {
    if let Some(command_input) = text.strip_prefix('/') {
        let raw = format!("/{command_input}");
        match parse_chat_command(&raw) {
            Ok(command) => {
                let command_name = command.name();
                match command {
                    aura_app::ui::types::ChatCommand::Join { channel } => {
                        let channel = channel.trim_start_matches('#').to_string();
                        let channel_id = ensure_named_channel(model, &channel, String::new());
                        model.select_channel_id(Some(&channel_id));
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "replicated",
                            &format!("joined #{channel}"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Leave => {
                        if let Some(current) = model.selected_channel_name().map(str::to_string) {
                            model.channels.retain(|row| row.name != current);
                            if model.channels.is_empty() {
                                model.channels.push(crate::model::ChannelRow {
                                    id: NOTE_TO_SELF_CHANNEL_NAME.to_string(),
                                    name: NOTE_TO_SELF_CHANNEL_NAME.to_string(),
                                    selected: true,
                                    topic: NOTE_TO_SELF_CHANNEL_TOPIC.to_string(),
                                });
                                model.selected_channel =
                                    Some(NOTE_TO_SELF_CHANNEL_NAME.to_string());
                            } else {
                                let selected_name = model.channels[0].name.clone();
                                model.selected_channel = Some(selected_name.clone());
                                for row in &mut model.channels {
                                    row.selected = row.name == selected_name;
                                }
                            }
                        }
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "replicated",
                            "left channel",
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Help { command } => {
                        let detail = if let Some(command) = command {
                            format!("/{command} <user> [reason]")
                        } else {
                            "Use ? for TUI help".to_string()
                        };
                        model.toast = Some(ToastState {
                            icon: 'ℹ',
                            message: detail,
                        });
                    }
                    aura_app::ui::types::ChatCommand::Whois { target } => {
                        model.toast = Some(ToastState {
                            icon: 'ℹ',
                            message: format!("User: {target}"),
                        });
                    }
                    aura_app::ui::types::ChatCommand::Kick { .. } => {
                        model.toast = Some(command_toast(
                            '✗',
                            "denied",
                            "permission_denied",
                            "accepted",
                            "kick permission denied",
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Ban { .. }
                    | aura_app::ui::types::ChatCommand::Mute { .. }
                    | aura_app::ui::types::ChatCommand::Unmute { .. }
                    | aura_app::ui::types::ChatCommand::Unban { .. }
                    | aura_app::ui::types::ChatCommand::Pin { .. }
                    | aura_app::ui::types::ChatCommand::Unpin { .. }
                    | aura_app::ui::types::ChatCommand::Op { .. }
                    | aura_app::ui::types::ChatCommand::Deop { .. } => {
                        model.toast = Some(command_toast(
                            '✗',
                            "denied",
                            "permission_denied",
                            "accepted",
                            "permission denied",
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Mode { flags, .. } => {
                        let trimmed = flags.trim();
                        if trimmed.starts_with('-') {
                            model.toast = Some(command_toast(
                                '✓',
                                "ok",
                                "none",
                                "enforced",
                                &format!("command {command_name} applied"),
                            ));
                        } else {
                            model.toast = Some(command_toast(
                                '✗',
                                "denied",
                                "permission_denied",
                                "accepted",
                                "permission denied",
                            ));
                        }
                    }
                    aura_app::ui::types::ChatCommand::Msg { text, .. } => {
                        let channel_id = ensure_dm_channel(model);
                        model.select_channel_id(Some(&channel_id));
                        model.messages.push(text);
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "accepted",
                            &format!("command {command_name} applied"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Me { action: text } => {
                        model.messages.push(text);
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "accepted",
                            &format!("command {command_name} applied"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Topic { text } => {
                        model.set_selected_channel_topic(text);
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "accepted",
                            &format!("command {command_name} applied"),
                        ));
                    }
                    aura_app::ui::types::ChatCommand::Invite { .. } => {
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "enforced",
                            "invitation sent",
                        ));
                    }
                    aura_app::ui::types::ChatCommand::HomeInvite { .. } => {
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "enforced",
                            "home invitation sent",
                        ));
                    }
                    aura_app::ui::types::ChatCommand::NhLink { .. } => {
                        model.toast = Some(command_toast(
                            '✗',
                            "denied",
                            "permission_denied",
                            "accepted",
                            "nhlink permission denied",
                        ));
                    }
                    _ => {
                        model.toast = Some(command_toast(
                            '✓',
                            "ok",
                            "none",
                            "replicated",
                            &format!("command {command_name} applied"),
                        ));
                    }
                }
            }
            Err(error) => {
                let reason_code = parse_reason_code_for_command_error(&error.to_string());
                model.toast = Some(command_toast(
                    '✗',
                    "invalid",
                    reason_code,
                    "accepted",
                    &error.to_string(),
                ));
            }
        }
        return;
    }

    model.messages.push(text.to_string());
    model.logs.push(format!("message:{text}"));

    if model
        .selected_channel_name()
        .is_some_and(|channel| channel.eq_ignore_ascii_case("demo-trio-room"))
    {
        model.messages.push(format!("Alice: echo {text}"));
        model.messages.push(format!("Carol: echo {text}"));
    }
}

fn handle_escape(model: &mut UiModel) {
    if model.input_mode {
        model.input_mode = false;
        model.input_buffer.clear();
        return;
    }
    if let Some(modal) = model.modal_state() {
        if handle_modal_escape(model, modal) {
            return;
        }
        dismiss_modal(model);
        return;
    }
    if model.screen == ScreenId::Contacts && model.contact_details {
        model.contact_details = false;
        return;
    }
    if model.screen == ScreenId::Neighborhood
        && matches!(
            model.neighborhood_mode,
            crate::model::NeighborhoodMode::Detail
        )
    {
        model.neighborhood_mode = crate::model::NeighborhoodMode::Map;
        return;
    }
    model.toast = None;
}

fn ensure_dm_channel(model: &mut UiModel) -> String {
    ensure_named_channel(model, "dm", String::new())
}

fn ensure_named_channel(model: &mut UiModel, channel_name: &str, topic: String) -> String {
    if let Some(row) = model
        .channels
        .iter_mut()
        .find(|row| row.name.eq_ignore_ascii_case(channel_name))
    {
        if !topic.trim().is_empty() {
            row.topic = topic;
        }
        return row.id.clone();
    }
    let channel_id = channel_name.to_string();
    model.channels.push(ChannelRow {
        id: channel_id.clone(),
        name: channel_name.to_string(),
        selected: false,
        topic,
    });
    channel_id
}

fn parse_reason_code_for_command_error(message: &str) -> &'static str {
    let lowered = message.to_ascii_lowercase();
    if lowered.contains("unknown command") || lowered.contains("unrecognized command") {
        "not_found"
    } else {
        "invalid_argument"
    }
}

fn dismiss_modal(model: &mut UiModel) {
    model.dismiss_modal();
}

fn handle_modal_tab(model: &mut UiModel, reverse: bool) -> bool {
    let channel_details_mode = matches!(
        model.active_modal,
        Some(ActiveModal::CreateChannel(CreateChannelModalState {
            step: CreateChannelWizardStep::Details,
            ..
        }))
    );
    if channel_details_mode {
        save_create_channel_details_buffer(model);
        if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
            state.active_field = if reverse {
                CreateChannelDetailsField::Name
            } else {
                match state.active_field {
                    CreateChannelDetailsField::Name => CreateChannelDetailsField::Topic,
                    CreateChannelDetailsField::Topic => CreateChannelDetailsField::Name,
                }
            };
        }
        return true;
    }

    if matches!(model.modal_state(), Some(ModalState::CapabilityConfig)) {
        save_capability_config_buffer(model);
        if let Some(ActiveModal::CapabilityConfig(state)) = model.active_modal.as_mut() {
            state.active_tier = if reverse {
                state.active_tier.prev()
            } else {
                state.active_tier.next()
            };
        }
        return true;
    }

    if matches!(model.modal_state(), Some(ModalState::AccessOverride)) {
        if let Some(ActiveModal::AccessOverride(state)) = model.active_modal.as_mut() {
            state.level = state.level.toggle();
        }
        return true;
    }

    false
}

fn modal_accepts_text(model: &UiModel, modal: ModalState) -> bool {
    let _ = modal;
    model.modal_accepts_text()
}

fn backspace(model: &mut UiModel) {
    if model.input_mode {
        model.input_buffer.pop();
    } else if let Some(modal) = model.modal_state() {
        if modal_accepts_text(model, modal) {
            model.pop_modal_text_char();
        }
    }
}

fn handle_modal_char(
    model: &mut UiModel,
    modal: ModalState,
    ch: char,
    clipboard: &dyn ClipboardPort,
) -> bool {
    match modal {
        ModalState::AddDeviceStep1 => {
            handle_add_device_modal_char(model, ch, clipboard);
            true
        }
        ModalState::CreateChannel => match model.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state))
                if matches!(state.step, CreateChannelWizardStep::Members) =>
            {
                if matches!(ch, ' ') {
                    toggle_create_channel_member(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if ch == 'k' {
                        state.member_focus = state.member_focus.saturating_sub(1);
                    } else {
                        state.member_focus = (state.member_focus + 1).min(max);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        },
        ModalState::GuardianSetup => {
            if matches!(
                model.active_modal,
                Some(ActiveModal::GuardianSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Selection,
                    ..
                }))
            ) {
                if matches!(ch, ' ') {
                    toggle_guardian_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    if model.contacts.is_empty() {
                        return true;
                    }
                    let max = model.contacts.len().saturating_sub(1);
                    if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
                        if ch == 'k' {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        ModalState::MfaSetup => {
            if matches!(
                model.active_modal,
                Some(ActiveModal::MfaSetup(ThresholdWizardModalState {
                    step: ThresholdWizardStep::Selection,
                    ..
                }))
            ) {
                if matches!(ch, ' ') {
                    toggle_mfa_selection(model);
                    true
                } else if matches!(ch, 'j' | 'k') {
                    let max = available_device_count(model).saturating_sub(1) as usize;
                    if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
                        if ch == 'k' {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
        ModalState::ConfirmRemoveDevice => {
            if matches!(ch, 'y' | 'Y') {
                handle_modal_enter(model, ModalState::ConfirmRemoveDevice, clipboard);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn handle_modal_escape(model: &mut UiModel, modal: ModalState) -> bool {
    match modal {
        ModalState::CreateChannel => match model.active_modal.as_mut() {
            Some(ActiveModal::CreateChannel(state)) => match state.step {
                CreateChannelWizardStep::Details => false,
                CreateChannelWizardStep::Members => {
                    state.step = CreateChannelWizardStep::Details;
                    state.active_field = CreateChannelDetailsField::Name;
                    model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
                    true
                }
                CreateChannelWizardStep::Threshold => {
                    state.step = CreateChannelWizardStep::Members;
                    model.modal_hint = "New Chat Group — Step 2 of 3".to_string();
                    true
                }
            },
            _ => false,
        },
        ModalState::GuardianSetup => match model.active_modal.as_mut() {
            Some(ActiveModal::GuardianSetup(state)) => match state.step {
                ThresholdWizardStep::Selection => false,
                ThresholdWizardStep::Threshold => {
                    state.step = ThresholdWizardStep::Selection;
                    state.threshold_input.clear();
                    model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
                    true
                }
                ThresholdWizardStep::Ceremony => false,
            },
            _ => false,
        },
        ModalState::MfaSetup => match model.active_modal.as_mut() {
            Some(ActiveModal::MfaSetup(state)) => match state.step {
                ThresholdWizardStep::Selection => false,
                ThresholdWizardStep::Threshold => {
                    state.step = ThresholdWizardStep::Selection;
                    state.threshold_input.clear();
                    model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
                    true
                }
                ThresholdWizardStep::Ceremony => false,
            },
            _ => false,
        },
        _ => false,
    }
}

fn open_create_channel_wizard(model: &mut UiModel) {
    model.modal_hint = "New Chat Group — Step 1 of 3".to_string();
    model.active_modal = Some(ActiveModal::CreateChannel(
        CreateChannelModalState::default(),
    ));
}

fn save_create_channel_details_buffer(model: &mut UiModel) {
    let value = model.modal_text_value().unwrap_or_default();
    if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
        match state.active_field {
            CreateChannelDetailsField::Name => state.name = value,
            CreateChannelDetailsField::Topic => state.topic = value,
        }
    }
}

fn toggle_create_channel_member(model: &mut UiModel) {
    if model.contacts.is_empty() {
        return;
    }
    if let Some(ActiveModal::CreateChannel(state)) = model.active_modal.as_mut() {
        let idx = state
            .member_focus
            .min(model.contacts.len().saturating_sub(1));
        if let Some(position) = state
            .selected_members
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_members.remove(position);
        } else {
            state.selected_members.push(idx);
            state.selected_members.sort_unstable();
        }
    }
}

fn open_add_device_wizard(model: &mut UiModel) {
    model.modal_hint = "Add Device — Step 1 of 3".to_string();
    model.active_modal = Some(ActiveModal::AddDevice(AddDeviceModalState::default()));
}

fn open_guardian_setup_wizard(model: &mut UiModel) {
    let mut selected_indices: Vec<usize> = model
        .contacts
        .iter()
        .enumerate()
        .filter(|(_, contact)| contact.is_guardian)
        .map(|(idx, _)| idx)
        .collect();
    if selected_indices.is_empty() {
        let selected = model.contacts.len().min(2);
        selected_indices = (0..selected).collect();
    }
    let selected_count = selected_indices.len().max(1) as u8;
    let threshold_k = selected_count.clamp(1, 2);
    model.modal_hint = "Guardian Setup — Step 1 of 3".to_string();
    let mut state = ThresholdWizardModalState::with_defaults(selected_count, threshold_k);
    state.selected_indices = selected_indices;
    model.active_modal = Some(ActiveModal::GuardianSetup(state));
}

fn can_open_guardian_setup_wizard(model: &mut UiModel) -> bool {
    if model.contacts.is_empty() {
        set_toast(model, '✗', "Add contacts first before setting up guardians");
        return false;
    }
    true
}

fn open_remove_device_selection(model: &mut UiModel) {
    let candidate_name = model
        .secondary_device_name()
        .unwrap_or("Secondary device")
        .to_string();
    model.modal_hint = "Select Device to Remove".to_string();
    model.active_modal = Some(ActiveModal::SelectDeviceToRemove(SelectDeviceModalState {
        candidate_name,
    }));
}

fn open_mfa_setup_wizard(model: &mut UiModel) {
    let selected_indices = (0..available_device_count(model) as usize).collect::<Vec<_>>();
    let selected_count = selected_indices.len().max(1) as u8;
    let threshold_k = selected_count.clamp(1, 2);
    model.modal_hint = "Multifactor Setup — Step 1 of 3".to_string();
    let mut state = ThresholdWizardModalState::with_defaults(selected_count, threshold_k);
    state.selected_indices = selected_indices;
    model.active_modal = Some(ActiveModal::MfaSetup(state));
}

fn save_capability_config_buffer(model: &mut UiModel) {
    let value = model.modal_text_value().unwrap_or_default();
    if let Some(ActiveModal::CapabilityConfig(state)) = model.active_modal.as_mut() {
        match state.active_tier {
            CapabilityTier::Full => state.full_caps = value,
            CapabilityTier::Partial => state.partial_caps = value,
            CapabilityTier::Limited => state.limited_caps = value,
        }
    }
}

fn can_open_mfa_setup_wizard(model: &mut UiModel) -> bool {
    let available = available_device_count(model) as usize;
    if available < 2 {
        set_toast(
            model,
            '✗',
            format!("MFA requires at least 2 devices, but only {available} available"),
        );
        return false;
    }
    true
}

fn handle_add_device_modal_char(model: &mut UiModel, ch: char, clipboard: &dyn ClipboardPort) {
    match model.active_modal.as_mut() {
        Some(ActiveModal::AddDevice(state)) if matches!(state.step, AddDeviceWizardStep::Name) => {
            state.name_input.push(ch);
        }
        Some(ActiveModal::AddDevice(state))
            if matches!(
                state.step,
                AddDeviceWizardStep::ShareCode | AddDeviceWizardStep::Confirm
            ) =>
        {
            if matches!(ch, 'c' | 'y') && !state.enrollment_code.is_empty() {
                clipboard.write(&state.enrollment_code);
                state.code_copied = true;
                set_toast(model, '✓', "Copied to clipboard");
            }
        }
        _ => {}
    }
}

fn handle_wizard_named_key(model: &mut UiModel, key_name: &str) -> bool {
    match model.modal_state() {
        Some(ModalState::CreateChannel) => {
            if matches!(key_name, "up" | "down") {
                match model.active_modal.as_mut() {
                    Some(ActiveModal::CreateChannel(state))
                        if matches!(state.step, CreateChannelWizardStep::Members) =>
                    {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        if key_name == "up" {
                            state.member_focus = state.member_focus.saturating_sub(1);
                        } else {
                            state.member_focus = (state.member_focus + 1).min(max);
                        }
                        return true;
                    }
                    Some(ActiveModal::CreateChannel(state))
                        if matches!(state.step, CreateChannelWizardStep::Threshold) =>
                    {
                        let current =
                            parse_wizard_value(&state.threshold.to_string(), state.threshold);
                        let max = (state.selected_members.len().saturating_add(1)) as u8;
                        let adjusted = if key_name == "up" {
                            current.saturating_add(1).min(max.max(1))
                        } else {
                            current.saturating_sub(1).max(1)
                        };
                        state.threshold = adjusted;
                        return true;
                    }
                    _ => {}
                }
            }
        }
        Some(ModalState::GuardianSetup) => {
            if matches!(key_name, "up" | "down") {
                match model.active_modal.as_mut() {
                    Some(ActiveModal::GuardianSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Selection) =>
                    {
                        if model.contacts.is_empty() {
                            return true;
                        }
                        let max = model.contacts.len().saturating_sub(1);
                        if key_name == "up" {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    Some(ActiveModal::GuardianSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Threshold) =>
                    {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, true, delta);
                    }
                    _ => {}
                }
                return true;
            }
        }
        Some(ModalState::MfaSetup) => {
            if matches!(key_name, "up" | "down") {
                let max = available_device_count(model).saturating_sub(1) as usize;
                match model.active_modal.as_mut() {
                    Some(ActiveModal::MfaSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Selection) =>
                    {
                        if key_name == "up" {
                            state.focus_index = state.focus_index.saturating_sub(1);
                        } else {
                            state.focus_index = (state.focus_index + 1).min(max);
                        }
                    }
                    Some(ActiveModal::MfaSetup(state))
                        if matches!(state.step, ThresholdWizardStep::Threshold) =>
                    {
                        let delta = if key_name == "up" { 1 } else { -1 };
                        adjust_threshold_wizard_input(model, false, delta);
                    }
                    _ => {}
                }
                return true;
            }
        }
        Some(ModalState::SelectDeviceToRemove) => {
            if matches!(key_name, "up" | "down") {
                return true;
            }
        }
        Some(ModalState::SwitchAuthority) => {
            if matches!(key_name, "up" | "down") {
                if model.authorities.is_empty() {
                    return true;
                }
                let max = model.authorities.len().saturating_sub(1);
                let selected_index = model.selected_authority_index().unwrap_or_default();
                if key_name == "up" {
                    model.set_selected_authority_index(selected_index.saturating_sub(1));
                } else {
                    model.set_selected_authority_index((selected_index + 1).min(max));
                }
                return true;
            }
        }
        Some(ModalState::AccessOverride) => {
            if matches!(key_name, "up" | "down") {
                if model.contacts.is_empty() {
                    return true;
                }
                let max = model.contacts.len().saturating_sub(1);
                let selected_index = model.selected_contact_index().unwrap_or_default();
                if key_name == "up" {
                    model.set_selected_contact_index(selected_index.saturating_sub(1));
                } else {
                    model.set_selected_contact_index((selected_index + 1).min(max));
                }
                return true;
            }
        }
        _ => {}
    }
    false
}

fn adjust_threshold_wizard_input(model: &mut UiModel, guardian: bool, delta: i8) {
    match model.active_modal.as_mut() {
        Some(ActiveModal::GuardianSetup(state)) if guardian => {
            let current = parse_wizard_value(&state.threshold_input, state.threshold_k);
            let adjusted =
                (current as i16 + delta as i16).clamp(1, state.selected_count.max(1) as i16) as u8;
            state.threshold_input = adjusted.to_string();
        }
        Some(ActiveModal::MfaSetup(state)) if !guardian => {
            let current = parse_wizard_value(&state.threshold_input, state.threshold_k);
            let adjusted =
                (current as i16 + delta as i16).clamp(1, state.selected_count.max(1) as i16) as u8;
            state.threshold_input = adjusted.to_string();
        }
        _ => {}
    }
}

fn parse_wizard_value(value: &str, fallback: u8) -> u8 {
    value.trim().parse::<u8>().unwrap_or(fallback.max(1))
}

fn available_device_count(model: &UiModel) -> u8 {
    if model.has_secondary_device {
        2
    } else {
        1
    }
}

fn toggle_guardian_selection(model: &mut UiModel) {
    if model.contacts.is_empty() {
        return;
    }
    if let Some(ActiveModal::GuardianSetup(state)) = model.active_modal.as_mut() {
        let idx = state
            .focus_index
            .min(model.contacts.len().saturating_sub(1));
        if let Some(position) = state
            .selected_indices
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_indices.remove(position);
        } else {
            state.selected_indices.push(idx);
            state.selected_indices.sort_unstable();
        }
    }
}

fn toggle_mfa_selection(model: &mut UiModel) {
    let available = available_device_count(model) as usize;
    if available == 0 {
        return;
    }
    if let Some(ActiveModal::MfaSetup(state)) = model.active_modal.as_mut() {
        let idx = state.focus_index.min(available.saturating_sub(1));
        if let Some(position) = state
            .selected_indices
            .iter()
            .position(|selected| *selected == idx)
        {
            state.selected_indices.remove(position);
        } else {
            state.selected_indices.push(idx);
            state.selected_indices.sort_unstable();
        }
    }
}

fn cycle_screen(model: &mut UiModel) {
    let next = match model.screen {
        ScreenId::Onboarding => ScreenId::Neighborhood,
        ScreenId::Neighborhood => ScreenId::Chat,
        ScreenId::Chat => ScreenId::Contacts,
        ScreenId::Contacts => ScreenId::Notifications,
        ScreenId::Notifications => ScreenId::Settings,
        ScreenId::Settings => ScreenId::Neighborhood,
    };
    model.set_screen(next);
}

fn cycle_screen_prev(model: &mut UiModel) {
    let next = match model.screen {
        ScreenId::Onboarding => ScreenId::Settings,
        ScreenId::Neighborhood => ScreenId::Settings,
        ScreenId::Chat => ScreenId::Neighborhood,
        ScreenId::Contacts => ScreenId::Chat,
        ScreenId::Notifications => ScreenId::Contacts,
        ScreenId::Settings => ScreenId::Notifications,
    };
    model.set_screen(next);
}

fn move_selection(model: &mut UiModel, delta: i32) {
    match model.screen {
        ScreenId::Onboarding => {}
        ScreenId::Settings => {
            let current = model.settings_section.index() as i32;
            let mut next = current + delta;
            if next < 0 {
                next = 0;
            }
            if next >= SettingsSection::ALL.len() as i32 {
                next = SettingsSection::ALL.len() as i32 - 1;
            }
            model.settings_section = SettingsSection::from_index(next as usize);
        }
        ScreenId::Contacts => {
            if model.contacts.is_empty() {
                return;
            }
            let max = model.contacts.len() as i32 - 1;
            let mut next = model.selected_contact_index().unwrap_or_default() as i32 + delta;
            if next < 0 {
                next = 0;
            }
            if next > max {
                next = max;
            }
            model.set_selected_contact_index(next as usize);
        }
        ScreenId::Chat => {
            model.move_channel_selection(delta);
        }
        ScreenId::Notifications => {
            if model.notifications.is_empty() {
                model.selected_notification_id = None;
                return;
            }
            let max = model.notifications.len() as i32 - 1;
            let mut next = model.selected_notification_index().unwrap_or_default() as i32 + delta;
            if next < 0 {
                next = 0;
            }
            if next > max {
                next = max;
            }
            model.set_selected_notification_index(next as usize, model.notifications.len());
        }
        ScreenId::Neighborhood => {}
    }
}

fn handle_horizontal(model: &mut UiModel, _delta: i32) {
    if model.screen == ScreenId::Contacts {
        model.contact_details = !model.contact_details;
    }
}

fn command_toast(
    icon: char,
    status: &str,
    reason: &str,
    consistency: &str,
    detail: &str,
) -> ToastState {
    ToastState {
        icon,
        message: format!("{detail} status={status} reason={reason} consistency={consistency}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_named_key, apply_text_keys};
    use crate::clipboard::{ClipboardPort, MemoryClipboard};
    use crate::model::{
        ActiveModal, AddDeviceModalState, AddDeviceWizardStep, CreateChannelModalState,
        CreateChannelWizardStep, CreateInvitationModalState, ModalState, ScreenId, SettingsSection,
        TextModalState, ThresholdWizardModalState, ThresholdWizardStep, UiModel,
    };

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
    fn accept_invitation_digit_shortcuts_map_to_demo_contacts() {
        let mut model = UiModel::new("authority-local".to_string());
        let clipboard = MemoryClipboard::default();

        model.active_modal = Some(ActiveModal::AcceptInvitation(TextModalState {
            value: "1".to_string(),
        }));
        apply_named_key(&mut model, "enter", 1, &clipboard);
        assert!(model.contacts.iter().any(|row| row.name == "Alice"));

        model.active_modal = Some(ActiveModal::AcceptInvitation(TextModalState {
            value: "2".to_string(),
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
            crate::keyboard::ensure_named_channel(&mut model, "demo-trio-room", String::new());
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

        let channel_id = crate::keyboard::ensure_named_channel(
            &mut model,
            "slash lab",
            "updated topic".to_string(),
        );

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
